//! Record keys on Windows: a low-level keyboard hook, on a thread of its own,
//! installed once at startup and kept for the app's life.
//!
//! The hook runs before any window gets a key, so a focused WebView2 view
//! takes no key ahead of it. A PC keyboard sends its media keys as keys, so
//! the hook hears them as it hears any other key, and a shortcut another app
//! owns as well. The key events this program sends carry its mark
//! ([`keys::SENT_BY_BANDBRIDGE`]) and the injected bit, and the hook leaves
//! them alone, so a band press or Try it now during a recording is not
//! recorded.
//!
//! The hook is installed once, at startup, not for each recording. The R-006
//! review measured a `WH_KEYBOARD_LL` hook installed at process start, on its
//! own thread with a `GetMessageW` loop, hearing keys another program sent
//! while the WebView2 window was in front. A hook installed for each
//! recording is not proven to hear keys yet: Task 8 checks it with a real key
//! press. So the thread is started once, from `main`, and it pumps for the
//! app's life. Between recordings its callback passes every key on. A
//! recording sets the current session ([`CURRENT`]); the callback then runs
//! the listener's rules for that session, and clears it when the recording
//! ends.
//!
//! Windows removes a low-level hook whose thread answers too slowly, and says
//! nothing. A recording that hears no key in its time may have met that, so
//! it installs the hook again, on the same thread: see [`rearm`].
//!
//! Windows asks no permission for a hook, so the page listens itself only
//! when the startup hook did not install.
//!
//! The modifiers held with the key a recording takes still reach the system,
//! and the key does not. A modifier let go with no key between acts alone:
//! Win opens Start, Alt the menu bar, and Shift or Ctrl may switch the input
//! language. So when the hook takes a key while a modifier is down, it taps
//! a key no key has, marked as this program's own, so the modifiers' release
//! is part of a shortcut, and none of them acts alone.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bandbridge::config::Keystroke;
use bandbridge::keys::{self, FLAG_COMMAND, FLAG_CONTROL, FLAG_OPTION, FLAG_SHIFT};
use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::PCWSTR;

use super::{Answer, Deaf, RECORD_LIMIT, RELEASE_LIMIT, Session};

/// How often a waiting recording looks at its state, in milliseconds: whether
/// its key has come up, it was stopped, or it ran out of time. The callback
/// answers a heard key at once, apart from this.
const LOOK_EVERY: u64 = 20;

/// Whether the hook is installed: by [`install`] at startup, and again by
/// each [`rearm`]. When it is not, the page listens instead, as the Mac's
/// window does without its tap.
static HOOK_UP: AtomicBool = AtomicBool::new(false);

/// The thread that holds the hook, which [`rearm`] asks to install it again.
/// Zero until [`install`] starts it.
static HOOK_THREAD: AtomicU32 = AtomicU32::new(0);

/// How many times the hook was installed: once at startup, and once for each
/// [`rearm`].
static ARMS: AtomicU32 = AtomicU32::new(0);

/// The message that asks the hook's thread to install the hook again.
const REARM: u32 = WM_APP + 1;

/// The recording that runs, if any, for the callback to serve: its session,
/// the listener's state for it, and when its key was taken. The callback
/// locks this for each key, briefly. `None` between recordings, when the
/// callback passes every key on.
static CURRENT: Mutex<Option<Active>> = Mutex::new(None);

/// The recording the callback serves now.
struct Active {
    session: Arc<Session>,
    listener: Listener,
    /// When the key was taken, to give up on its up after a while.
    took: Option<Instant>,
}

/// Install the keyboard hook once, at startup, on a thread that pumps for the
/// app's life. Waits until the hook is up or has failed, so a recording that
/// starts right after knows whether to listen in the page instead. Called
/// once from `main` on Windows.
pub fn install() {
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let (tx, rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("record keys hook".to_string())
        .spawn(move || {
            // The thread's message queue, made now, so a rearm posted to it
            // from the first moment is kept.
            let mut message = MSG::default();
            // SAFETY: looks at this thread's own queue, and takes nothing.
            let _ = unsafe { PeekMessageW(&mut message, None, WM_USER, WM_USER, PM_NOREMOVE) };
            // SAFETY: a plain query about this thread.
            HOOK_THREAD.store(unsafe { GetCurrentThreadId() }, Ordering::SeqCst);
            let mut hook = arm();
            let _ = tx.send(hook.is_some());
            // A low-level hook is called only while its thread retrieves
            // messages, so this loop retrieves them for the app's life.
            // SAFETY: retrieves this thread's messages, and calls the hook for
            // each key before it returns. `message` lives for each call.
            while unsafe { GetMessageW(&mut message, None, 0, 0) }.0 > 0 {
                if message.message == REARM {
                    if let Some(old) = hook.take() {
                        // Windows may have removed it already, which is why
                        // it is installed again.
                        // SAFETY: the hook this thread set, taken away once.
                        let _ = unsafe { UnhookWindowsHookEx(old) };
                    }
                    hook = arm();
                }
            }
        });
    match spawned {
        Ok(_) => {
            let _ = rx.recv();
        }
        Err(err) => tracing::warn!("the keyboard hook thread did not start: {err}"),
    }
}

/// Install the hook on this thread, and say whether it is up.
fn arm() -> Option<HHOOK> {
    // SAFETY: `heard` has a hook's signature, and this thread pumps the
    // messages that call it for the app's life.
    let hook = unsafe {
        let module = GetModuleHandleW(PCWSTR::null()).ok().map(HINSTANCE::from);
        SetWindowsHookExW(WH_KEYBOARD_LL, Some(heard), module, 0)
    };
    match hook {
        Ok(hook) => {
            HOOK_UP.store(true, Ordering::SeqCst);
            ARMS.fetch_add(1, Ordering::SeqCst);
            Some(hook)
        }
        Err(err) => {
            HOOK_UP.store(false, Ordering::SeqCst);
            tracing::warn!("Windows gave no keyboard hook, so the page listens: {err}");
            None
        }
    }
}

/// Install the hook again, on its own thread, after a recording that heard
/// no key in its time. Windows removes a low-level hook whose thread answers
/// too slowly, and says nothing, so such a recording may have had no hook.
/// Without the thread, which [`install`] starts, this does nothing.
pub(super) fn rearm() {
    let thread = HOOK_THREAD.load(Ordering::SeqCst);
    if thread == 0 {
        return;
    }
    // SAFETY: a message with no pointers in it, to the hook's thread.
    if let Err(err) = unsafe { PostThreadMessageW(thread, REARM, WPARAM(0), LPARAM(0)) } {
        tracing::warn!("the keyboard hook was not asked to install again: {err}");
    }
}

/// When the startup hook did not install, the page listens, as the Mac's
/// window does without its tap.
pub(super) fn deaf_now() -> Option<Deaf> {
    if HOOK_UP.load(Ordering::SeqCst) {
        None
    } else {
        Some(Deaf::NoTap)
    }
}

/// Lock [`CURRENT`], through a poisoned lock as well: a panic in a callback
/// holding it must not stop every later recording.
fn current() -> std::sync::MutexGuard<'static, Option<Active>> {
    CURRENT.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// One key, as the hook names it: its virtual key, and whether it carries
/// the extended bit, which tells keypad enter from return.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Key {
    code: u16,
    extended: bool,
}

/// What the hook saw, as much of it as naming a key needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Seen {
    /// A key went down: the key, the modifiers Windows holds down as it
    /// comes, as bits of [`keys`], and whether the key was down already,
    /// which makes this a repeat of a key held from before.
    Down { key: Key, held: u64, repeat: bool },
    /// A key came up.
    Up { key: Key },
}

/// What the hook does with one event, and what the recording heard in it.
#[derive(Debug, PartialEq, Eq)]
struct Verdict {
    /// No app gets the event.
    swallow: bool,
    /// The keys the recording heard, or why they cannot be sent. Once.
    heard: Option<Result<Keystroke, String>>,
    /// A modifier is down as the key is taken: tap the blank key, so the
    /// modifier does not act alone when it comes up.
    blank: bool,
}

impl Verdict {
    fn pass() -> Self {
        Self {
            swallow: false,
            heard: None,
            blank: false,
        }
    }

    fn swallow() -> Self {
        Self {
            swallow: true,
            heard: None,
            blank: false,
        }
    }
}

/// The modifier each virtual key holds, as a bit of [`keys`]: either Ctrl,
/// either Alt, either Shift, and either Win key. A keyboard sends a side's
/// own key, and a program may send the key for both sides.
const MODIFIER_KEYS: &[(VIRTUAL_KEY, u64)] = &[
    (VK_CONTROL, FLAG_CONTROL),
    (VK_LCONTROL, FLAG_CONTROL),
    (VK_RCONTROL, FLAG_CONTROL),
    (VK_MENU, FLAG_OPTION),
    (VK_LMENU, FLAG_OPTION),
    (VK_RMENU, FLAG_OPTION),
    (VK_SHIFT, FLAG_SHIFT),
    (VK_LSHIFT, FLAG_SHIFT),
    (VK_RSHIFT, FLAG_SHIFT),
    (VK_LWIN, FLAG_COMMAND),
    (VK_RWIN, FLAG_COMMAND),
];

/// The modifier a virtual key holds, or None for any other key.
fn modifier_bit(code: u16) -> Option<u64> {
    MODIFIER_KEYS
        .iter()
        .find(|(key, _)| key.0 == code)
        .map(|(_, bit)| *bit)
}

/// Whether an event is for no key a person presses: 0 and 0xFF, which some
/// laptops send for fn, and 0xE8, which programs tap to keep a modifier from
/// acting alone, as this one does. A recording waits past them.
fn no_key(code: u16) -> bool {
    matches!(code, 0x00 | 0xE8 | 0xFF)
}

/// The recording's rules, one event at a time, apart from any hook, so they
/// can be tested.
///
/// It leaves alone what this program sends itself. It follows the modifiers,
/// and waits for the first key a person presses that is not a modifier, not
/// a key held down from before, and not a key no key has. It names that key
/// with the modifiers held, and swallows its down, its repeats and its up.
/// It passes everything else on as it came.
#[derive(Debug, Default)]
struct Listener {
    /// The key taken, once there is one.
    taken: Option<Key>,
    /// The key taken has come up: the hook is no longer needed.
    released: bool,
    /// The modifiers seen to go down, and not yet up, since the hook began.
    /// Another program's hook may keep a modifier from Windows, and a person
    /// still pressed it.
    down: u64,
}

impl Listener {
    /// What to do with an event, which `own` says this program sent.
    fn hear(&mut self, own: bool, seen: Seen) -> Verdict {
        if own || self.released {
            return Verdict::pass();
        }
        let (Seen::Down { key, .. } | Seen::Up { key }) = seen;
        if let Some(bit) = modifier_bit(key.code) {
            match seen {
                Seen::Down { .. } => self.down |= bit,
                Seen::Up { .. } => self.down &= !bit,
            }
            return Verdict::pass();
        }
        match (self.taken, seen) {
            (None, Seen::Down { key, held, repeat }) => {
                // Caps lock is a modifier in its own right, and holds none.
                let modifier =
                    keys::key_name(key.code, key.extended).is_some_and(keys::is_modifier_key);
                if repeat || modifier || no_key(key.code) {
                    return Verdict::pass();
                }
                self.taken = Some(key);
                let held = held | self.down;
                let heard = match keys::heard_media_key(key.code) {
                    Some(media) => Ok(media),
                    None => keys::heard_key(key.code, key.extended, held)
                        .ok_or_else(|| unnamed(key.code)),
                };
                Verdict {
                    swallow: true,
                    heard: Some(heard),
                    blank: held != 0,
                }
            }
            (Some(taken), Seen::Down { key, .. }) if key == taken => Verdict::swallow(),
            (Some(taken), Seen::Up { key }) if key == taken => {
                self.released = true;
                Verdict::swallow()
            }
            _ => Verdict::pass(),
        }
    }
}

/// Why a key cannot be recorded: this program has no name for it, and so
/// cannot send it.
fn unnamed(code: u16) -> String {
    format!(
        "Mi Band Bridge has no name for that key (virtual key 0x{code:02X}), so it cannot send \
         it. Pick a key from the list."
    )
}

/// Serve one recording with the startup hook, until it is over: a key was
/// taken and came up, the recording stopped, or it ran out of time.
///
/// The hook is already installed, so this sets the recording as the current
/// one and waits for it to end. It does not touch the hook. When the startup
/// hook did not install, `record` never calls this: [`deaf_now`] tells the
/// page to listen instead.
pub(super) fn listen(session: Arc<Session>) {
    // Stopped before it began: it takes no place.
    if session.stopped() {
        session.answer(Answer::Stopped);
        return;
    }
    // Become the recording the callback serves. A recording before this one
    // was already ended by `record`, which answers only the newest.
    {
        let mut current = current();
        *current = Some(Active {
            session: session.clone(),
            listener: Listener::default(),
            took: None,
        });
    }
    let deadline = Instant::now() + RECORD_LIMIT + Duration::from_secs(1);
    loop {
        std::thread::sleep(Duration::from_millis(LOOK_EVERY));
        let (released, took) = {
            let current = current();
            current
                .as_ref()
                .filter(|active| Arc::ptr_eq(&active.session, &session))
                .map(|active| (active.listener.released, active.took))
                .unwrap_or((true, None))
        };
        let gave_up = took.is_some_and(|at| at.elapsed() > RELEASE_LIMIT);
        if session.stopped() || released || gave_up || Instant::now() > deadline {
            break;
        }
    }
    // Stop serving this recording, unless another already took its place.
    {
        let mut current = current();
        if current
            .as_ref()
            .is_some_and(|active| Arc::ptr_eq(&active.session, &session))
        {
            *current = None;
        }
    }
    // Nobody waits for this recording any more.
    session.answer(Answer::Stopped);
}

/// The startup hook's callback, for the app's life. Between recordings, and
/// for the keys this program sends, it passes every key on. During a
/// recording it runs the listener's rules for the current session: it answers
/// the key once, and swallows the recorded key's down, repeats and up.
unsafe extern "system" fn heard(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        // SAFETY: for HC_ACTION, lparam points to the event, which lives for
        // this call.
        let event = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        if take_in(event) {
            return LRESULT(1);
        }
    }
    // SAFETY: the arguments this callback was given, passed on.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// Let the current recording hear one event, and say whether to swallow it.
fn take_in(event: &KBDLLHOOKSTRUCT) -> bool {
    let verdict = {
        let Ok(mut current) = CURRENT.try_lock() else {
            // The lock is held elsewhere for a moment: pass the key on rather
            // than wait in a hook callback.
            return false;
        };
        let Some(active) = current.as_mut() else {
            // No recording runs: the key is not ours to take.
            return false;
        };
        // A recording that was stopped takes nothing more.
        if active.session.stopped() {
            return false;
        }
        let own = own_key(event);
        let mut verdict = active.listener.hear(own, read(event));
        if let Some(heard) = verdict.heard.take() {
            active.took = Some(Instant::now());
            active.session.answer(Answer::Heard(heard));
        }
        verdict
    };
    // The tap goes into the input after the key it follows, before the
    // modifier comes up. It carries this program's mark, so the callback lets
    // it pass. The lock above is released first, since the tap sends key
    // events that call this callback again.
    if verdict.blank {
        if let Err(err) = keys::tap_blank() {
            tracing::warn!("the key that keeps a modifier from acting alone was not sent: {err}");
        }
    }
    verdict.swallow
}

/// Whether this program sent the event itself: it is injected, and carries
/// this program's mark. A real key is not injected, and a key another program
/// injects does not carry the mark, so both are heard.
fn own_key(event: &KBDLLHOOKSTRUCT) -> bool {
    event.flags.0 & LLKHF_INJECTED.0 != 0 && event.dwExtraInfo == keys::SENT_BY_BANDBRIDGE
}

/// What an event is, for the listener.
fn read(event: &KBDLLHOOKSTRUCT) -> Seen {
    let key = Key {
        code: u16::try_from(event.vkCode).unwrap_or(0),
        extended: event.flags.0 & LLKHF_EXTENDED.0 != 0,
    };
    if event.flags.0 & LLKHF_UP.0 != 0 {
        return Seen::Up { key };
    }
    // Windows moves a key's own state after its hooks have run, so a key that
    // reads down already was down before this event: it repeats.
    Seen::Down {
        key,
        held: held_now(),
        repeat: is_down(key.code),
    }
}

/// Whether Windows holds a key down.
fn is_down(code: u16) -> bool {
    // SAFETY: a plain query.
    let state = unsafe { GetAsyncKeyState(i32::from(code)) };
    state as u16 & 0x8000 != 0
}

/// The modifiers Windows holds down, as bits of [`keys`].
fn held_now() -> u64 {
    [
        (VK_CONTROL, FLAG_CONTROL),
        (VK_MENU, FLAG_OPTION),
        (VK_SHIFT, FLAG_SHIFT),
        (VK_LWIN, FLAG_COMMAND),
        (VK_RWIN, FLAG_COMMAND),
    ]
    .into_iter()
    .filter(|(key, _)| is_down(key.0))
    .fold(0, |bits, (_, bit)| bits | bit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recorder::Recorded;

    fn key(code: VIRTUAL_KEY) -> Key {
        Key {
            code: code.0,
            extended: false,
        }
    }

    fn extended(code: VIRTUAL_KEY) -> Key {
        Key {
            code: code.0,
            extended: true,
        }
    }

    fn down(key: Key, held: u64) -> Seen {
        Seen::Down {
            key,
            held,
            repeat: false,
        }
    }

    fn up(key: Key) -> Seen {
        Seen::Up { key }
    }

    fn keystroke(key: &str, modifiers: &[&str]) -> Keystroke {
        Keystroke {
            key_code: Some(key.to_string()),
            consumer_key_code: None,
            modifiers: modifiers.iter().map(|m| m.to_string()).collect(),
        }
    }

    fn heard(keystroke: Keystroke, blank: bool) -> Verdict {
        Verdict {
            swallow: true,
            heard: Some(Ok(keystroke)),
            blank,
        }
    }

    /// Ctrl Shift F13, as a keyboard sends it: the modifiers go down, then
    /// the key. The key is heard with the modifiers, and it and its up are
    /// swallowed, so the app that owns the shortcut never starts. The
    /// modifiers pass, and the blank key is tapped, so they do not act alone
    /// when they come up.
    #[test]
    fn the_first_key_is_heard_with_its_modifiers_and_swallowed_down_and_up() {
        let mut listener = Listener::default();
        let both = FLAG_CONTROL | FLAG_SHIFT;
        assert_eq!(listener.hear(false, down(key(VK_LCONTROL), 0)), Verdict::pass());
        assert_eq!(
            listener.hear(false, down(key(VK_LSHIFT), FLAG_CONTROL)),
            Verdict::pass()
        );
        assert_eq!(
            listener.hear(false, down(key(VK_F13), both)),
            heard(keystroke("f13", &["shift", "control"]), true)
        );
        assert!(!listener.released);
        // Held down, it repeats: swallowed, and heard once only.
        let repeat = Seen::Down {
            key: key(VK_F13),
            held: both,
            repeat: true,
        };
        assert_eq!(listener.hear(false, repeat), Verdict::swallow());
        // Another key meanwhile passes on.
        assert_eq!(listener.hear(false, down(key(VK_K), both)), Verdict::pass());
        assert_eq!(listener.hear(false, up(key(VK_K))), Verdict::pass());
        assert_eq!(listener.hear(false, up(key(VK_F13))), Verdict::swallow());
        assert!(listener.released, "the hook can go");
        // After that, nothing is swallowed.
        assert_eq!(listener.hear(false, up(key(VK_LSHIFT))), Verdict::pass());
        assert_eq!(listener.hear(false, down(key(VK_F13), 0)), Verdict::pass());
    }

    /// Win Shift S, the Windows counterpart of the design's command shift 4:
    /// Windows holds the shortcut. The key is recorded with its modifiers,
    /// and it and its up are swallowed, so the snip tool does not open. The
    /// blank key is tapped, so Start does not open when Win comes up.
    #[test]
    fn a_windows_shortcut_records_and_does_not_act() {
        let mut listener = Listener::default();
        let both = FLAG_COMMAND | FLAG_SHIFT;
        assert_eq!(listener.hear(false, down(key(VK_LWIN), 0)), Verdict::pass());
        assert_eq!(
            listener.hear(false, down(key(VK_LSHIFT), FLAG_COMMAND)),
            Verdict::pass()
        );
        assert_eq!(
            listener.hear(false, down(key(VK_S), both)),
            heard(keystroke("s", &["command", "shift"]), true)
        );
        assert_eq!(listener.hear(false, up(key(VK_S))), Verdict::swallow());
        assert!(listener.released);
    }

    /// A modifier counts when Windows holds it, pressed before the recording
    /// began, and when the hook saw it go down, though another program's hook
    /// kept it from Windows. A key alone taps no blank key.
    #[test]
    fn a_modifier_counts_when_windows_holds_it_or_the_hook_saw_it() {
        let mut before = Listener::default();
        assert_eq!(
            before.hear(false, down(key(VK_K), FLAG_COMMAND)),
            heard(keystroke("k", &["command"]), true)
        );

        let mut seen = Listener::default();
        assert_eq!(seen.hear(false, down(key(VK_RMENU), 0)), Verdict::pass());
        assert_eq!(
            seen.hear(false, down(key(VK_K), 0)),
            heard(keystroke("k", &["option"]), true)
        );

        // One that came up again no longer counts.
        let mut gone = Listener::default();
        assert_eq!(gone.hear(false, down(key(VK_LWIN), 0)), Verdict::pass());
        assert_eq!(gone.hear(false, up(key(VK_LWIN))), Verdict::pass());
        assert_eq!(
            gone.hear(false, down(key(VK_K), 0)),
            heard(keystroke("k", &[]), false)
        );
    }

    /// A key this program sends is never recorded, nor swallowed, nor its
    /// modifiers counted: a band press or Try it now during a recording
    /// types as it should.
    #[test]
    fn a_key_this_program_sends_is_left_alone() {
        let mut listener = Listener::default();
        assert_eq!(listener.hear(true, down(key(VK_LCONTROL), 0)), Verdict::pass());
        assert_eq!(listener.hear(true, down(key(VK_K), FLAG_CONTROL)), Verdict::pass());
        assert_eq!(listener.hear(true, up(key(VK_K))), Verdict::pass());
        assert_eq!(
            listener.hear(true, down(extended(VK_VOLUME_UP), 0)),
            Verdict::pass()
        );
        assert_eq!(listener.taken, None);
        // The next key from the keyboard is the one, with no modifier.
        assert_eq!(
            listener.hear(false, down(key(VK_K), 0)),
            heard(keystroke("k", &[]), false)
        );
    }

    /// The recording waits for the first key that is not a modifier, not a
    /// key held down from before it began, such as the Enter that pressed
    /// Record keys, and not a key no key has.
    #[test]
    fn it_waits_for_a_key_that_is_not_a_modifier_or_held_from_before() {
        let mut listener = Listener::default();
        // The Enter that pressed the button comes up, then repeats.
        assert_eq!(listener.hear(false, up(key(VK_RETURN))), Verdict::pass());
        let held = Seen::Down {
            key: key(VK_RETURN),
            held: 0,
            repeat: true,
        };
        assert_eq!(listener.hear(false, held), Verdict::pass());
        // Caps lock, and what some laptops send for fn, pass.
        assert_eq!(listener.hear(false, down(key(VK_CAPITAL), 0)), Verdict::pass());
        for code in [0x00, 0xE8, 0xFF] {
            let nothing = Key {
                code,
                extended: false,
            };
            assert_eq!(listener.hear(false, down(nothing, 0)), Verdict::pass(), "{code}");
        }
        assert_eq!(listener.taken, None);
        assert_eq!(
            listener.hear(false, down(key(VK_F5), 0)),
            heard(keystroke("f5", &[]), false)
        );
    }

    /// A media key is recorded as a media key, with no modifiers, and its
    /// down and up are swallowed, so the volume does not change.
    #[test]
    fn a_media_key_is_heard_and_swallowed_down_and_up() {
        let mut listener = Listener::default();
        let volume_up = extended(VK_VOLUME_UP);
        assert_eq!(
            listener.hear(false, down(volume_up, FLAG_SHIFT)),
            Verdict {
                swallow: true,
                heard: Some(Ok(Keystroke {
                    key_code: None,
                    consumer_key_code: Some("volume_increment".to_string()),
                    modifiers: Vec::new(),
                })),
                blank: true,
            }
        );
        assert_eq!(listener.hear(false, down(volume_up, FLAG_SHIFT)), Verdict::swallow());
        assert!(!listener.released);
        assert_eq!(listener.hear(false, up(volume_up)), Verdict::swallow());
        assert!(listener.released);
    }

    /// The six media keys each record as a media keystroke, so a press of any
    /// is taken in the same hook, with no `RegisterHotKey`.
    #[test]
    fn the_six_media_keys_each_record() {
        let media = [
            (VK_VOLUME_UP, "volume_increment"),
            (VK_VOLUME_DOWN, "volume_decrement"),
            (VK_VOLUME_MUTE, "mute"),
            (VK_MEDIA_PLAY_PAUSE, "play_or_pause"),
            (VK_MEDIA_NEXT_TRACK, "scan_next_track"),
            (VK_MEDIA_PREV_TRACK, "scan_previous_track"),
        ];
        for (code, name) in media {
            let mut listener = Listener::default();
            let verdict = listener.hear(false, down(extended(code), 0));
            assert_eq!(
                verdict.heard.unwrap().unwrap().consumer_key_code.as_deref(),
                Some(name),
                "{name}"
            );
            assert!(verdict.swallow);
        }
    }

    /// Return and keypad enter share a virtual key, and the extended bit
    /// tells them apart, down and up.
    #[test]
    fn return_and_keypad_enter_are_told_apart() {
        let mut keypad = Listener::default();
        assert_eq!(
            keypad.hear(false, down(extended(VK_RETURN), 0)),
            heard(keystroke("keypad_enter", &[]), false)
        );
        assert_eq!(keypad.hear(false, up(key(VK_RETURN))), Verdict::pass());
        assert_eq!(keypad.hear(false, up(extended(VK_RETURN))), Verdict::swallow());

        let mut plain = Listener::default();
        assert_eq!(
            plain.hear(false, down(key(VK_RETURN), 0)),
            heard(keystroke("return_or_enter", &[]), false)
        );
    }

    /// A key this program has no name for cannot be sent: the recording
    /// says so, and swallows it, as it would a key it could name. The keys
    /// only a PC has are named.
    #[test]
    fn a_key_with_no_name_says_why() {
        let mut listener = Listener::default();
        let mail = extended(VK_LAUNCH_MAIL);
        let verdict = listener.hear(false, down(mail, 0));
        assert!(verdict.swallow);
        let why = verdict.heard.unwrap().unwrap_err();
        assert!(
            why.contains("virtual key 0xB4") && why.contains("from the list"),
            "{why}"
        );
        assert_eq!(listener.hear(false, up(mail)), Verdict::swallow());

        let mut pc = Listener::default();
        assert_eq!(
            pc.hear(false, down(extended(VK_SNAPSHOT), FLAG_CONTROL)),
            heard(keystroke("print_screen", &["control"]), true)
        );
    }

    /// The tests that read or set whether the hook is up take turns, since
    /// the hook is one for the whole program.
    static HOOK_TESTS: Mutex<()> = Mutex::new(());

    fn one_hook_test_at_a_time() -> std::sync::MutexGuard<'static, ()> {
        HOOK_TESTS.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Windows asks no permission for a hook, so the page listens only when
    /// the hook did not install, and says so as the Mac's window does.
    #[test]
    fn the_page_listens_only_when_the_hook_did_not_install() {
        let _turn = one_hook_test_at_a_time();
        let was = HOOK_UP.load(Ordering::SeqCst);
        // With the hook up, the app hears the keys, so the page does not.
        HOOK_UP.store(true, Ordering::SeqCst);
        assert_eq!(deaf_now(), None);
        // With no hook, the page listens instead.
        HOOK_UP.store(false, Ordering::SeqCst);
        assert_eq!(deaf_now(), Some(Deaf::NoTap));
        HOOK_UP.store(was, Ordering::SeqCst);
        let json = |recorded| serde_json::to_value(recorded).unwrap();
        assert_eq!(
            json(Recorded::Page(Deaf::NoTap)),
            serde_json::json!({"page": "no_tap"})
        );
        assert_eq!(
            json(Recorded::Heard(keystroke("f13", &["shift", "control"]))),
            serde_json::json!({"heard": {"key_code": "f13", "modifiers": ["shift", "control"]}})
        );
        assert_eq!(
            json(Recorded::Heard(keys::heard_media_key(VK_MEDIA_PLAY_PAUSE.0).unwrap())),
            serde_json::json!({"heard": {"consumer_key_code": "play_or_pause"}})
        );
    }

    /// A recording with no key in its time asks for the hook again, and the
    /// hook's own thread installs it again, and keeps it up. No key is sent
    /// here: the hook passes every key on while no recording runs.
    #[test]
    fn a_rearm_installs_the_hook_again_on_its_thread() {
        let _turn = one_hook_test_at_a_time();
        install();
        assert_ne!(HOOK_THREAD.load(Ordering::SeqCst), 0, "the hook has its thread");
        let before = ARMS.load(Ordering::SeqCst);
        assert!(before >= 1, "the hook was installed at start");
        rearm();
        let asked = Instant::now();
        while ARMS.load(Ordering::SeqCst) == before && asked.elapsed() < Duration::from_secs(2) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(ARMS.load(Ordering::SeqCst), before + 1, "installed again once");
        assert!(HOOK_UP.load(Ordering::SeqCst), "and up");
        assert_eq!(deaf_now(), None);
    }

    /// A recording that stops stops serving within a look, and clears itself
    /// as the current one. One stopped before it began serves nothing. No key
    /// is sent here: a test that pressed keys would type into the window in
    /// front.
    #[test]
    fn a_stopped_recording_stops_serving_within_a_look() {
        let (early, mut early_answer) = Session::new();
        early.stop();
        listen(early);
        assert!(matches!(early_answer.try_recv(), Ok(Answer::Stopped)));
        assert!(current().is_none(), "a stopped recording serves nothing");

        let (session, _answer) = Session::new();
        let listening = session.clone();
        let thread = std::thread::spawn(move || listen(listening));
        std::thread::sleep(Duration::from_millis(100));
        assert!(current().is_some(), "the recording is the current one");
        let stopped = Instant::now();
        session.stop();
        thread.join().unwrap();
        assert!(
            stopped.elapsed() < Duration::from_millis(1000),
            "it kept serving {:?} after the stop",
            stopped.elapsed()
        );
        assert!(current().is_none(), "it cleared itself");
    }
}
