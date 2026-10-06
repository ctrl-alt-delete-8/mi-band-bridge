//! Record keys on macOS: an active session event tap in this process, as
//! B-009b found. It hears a shortcut another app owns, such as command shift
//! 4, and the media keys, which come as system-defined events.
//!
//! The tap needs the input permission, which a keystroke action needs anyway.
//! Without it, while Secure Event Input is on, or when macOS gives no tap,
//! the page listens instead, and hears only what reaches the window.

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bandbridge::config::Keystroke;
use bandbridge::keys;
use objc2_app_kit::NSEvent;
use objc2_core_foundation::{CFMachPort, CFRunLoop, kCFRunLoopCommonModes, kCFRunLoopDefaultMode};
use objc2_core_graphics::{
    CGEvent, CGEventField, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventTapProxy, CGEventType,
};

use super::{Answer, Deaf, RECORD_LIMIT, RELEASE_LIMIT, Session};

/// How often the tap's thread looks up from its run loop, to see whether the
/// recording is over.
const LOOK_EVERY: f64 = 0.05;

/// A system-defined event: a media key is one, with subtype 8.
const SYSTEM_DEFINED: CGEventType = CGEventType(14);

/// The events the tap listens for: a key down, a key up, and system-defined
/// events. A modifier going down is a flags-changed event, which the tap
/// does not need: a key carries the modifiers held with it in its flags.
const LISTENS_FOR: u64 =
    (1 << CGEventType::KeyDown.0) | (1 << CGEventType::KeyUp.0) | (1 << SYSTEM_DEFINED.0);

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    /// Whether a program has turned on Secure Event Input, such as a
    /// password field or Terminal's Secure Keyboard Entry. While it is on, no
    /// tap hears a key typed.
    fn IsSecureEventInputEnabled() -> u8;
}

/// Why the tap cannot hear the keys now, or None when it can: see
/// [`deafness`].
pub(super) fn deaf_now() -> Option<Deaf> {
    // SAFETY: a plain query, with no argument.
    let secure_input = unsafe { IsSecureEventInputEnabled() } != 0;
    // An active tap needs the input permission, and hears keys only with
    // Input Monitoring as well. On macOS 27 the one answers the other, but a
    // tap made without it would be made, and deaf, so both are asked.
    let allowed = keys::trusted(false) && objc2_core_graphics::CGPreflightListenEventAccess();
    deafness(allowed, secure_input)
}

/// Why the tap cannot hear the keys now, from whether this program has the
/// input permission and whether Secure Event Input is on. None when it can.
fn deafness(allowed: bool, secure_input: bool) -> Option<Deaf> {
    if !allowed {
        Some(Deaf::Permission)
    } else if secure_input {
        Some(Deaf::SecureInput)
    } else {
        None
    }
}

/// What the tap saw, as much of it as naming a key needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Seen {
    /// A key went down: its keycode, the event's flags, and whether it is a
    /// key held down that repeats.
    KeyDown { code: u16, flags: u64, repeat: bool },
    /// A key came up.
    KeyUp { code: u16 },
    /// A media key went down or came up: its NX_KEYTYPE.
    Media { code: u16, down: bool },
    /// Anything else, which the tap passes on.
    Other,
}

/// The one key a recording takes, by its code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Taken {
    Key(u16),
    Media(u16),
}

/// What the tap does with one event, and what the recording heard in it.
#[derive(Debug, PartialEq, Eq)]
struct Verdict {
    /// No app gets the event.
    swallow: bool,
    /// The keys the recording heard, or why they cannot be posted. Once.
    heard: Option<Result<Keystroke, String>>,
}

impl Verdict {
    fn pass() -> Self {
        Self {
            swallow: false,
            heard: None,
        }
    }

    fn swallow() -> Self {
        Self {
            swallow: true,
            heard: None,
        }
    }
}

/// The recording's rules, one event at a time, apart from any tap, so they
/// can be tested.
///
/// It leaves alone what this program posts itself. It waits for the first
/// key a person presses that is not a modifier, and not a key held down from
/// before, then names it. It swallows that key's down, its repeats and its
/// up, and passes everything else on as it came.
#[derive(Debug)]
struct Listener {
    /// This program's pid. An event it posted carries it as its source.
    own: i64,
    /// The key taken, once there is one.
    taken: Option<Taken>,
    /// The key taken has come up: the tap is no longer needed.
    released: bool,
}

impl Listener {
    fn new(own: i64) -> Self {
        Self {
            own,
            taken: None,
            released: false,
        }
    }

    /// What to do with an event from the process `source`.
    fn hear(&mut self, source: i64, seen: Seen) -> Verdict {
        if source == self.own || self.released {
            return Verdict::pass();
        }
        match (self.taken, seen) {
            (
                None,
                Seen::KeyDown {
                    code,
                    flags,
                    repeat,
                },
            ) => {
                // A key held from before the recording repeats, and a
                // modifier is part of the key it goes with.
                let modifier = keys::key_name(code).is_some_and(keys::is_modifier_key);
                if repeat || modifier {
                    return Verdict::pass();
                }
                self.taken = Some(Taken::Key(code));
                Verdict {
                    swallow: true,
                    heard: Some(keys::heard_key(code, flags).ok_or_else(|| unnamed(code))),
                }
            }
            (None, Seen::Media { code, down: true }) => match keys::heard_media_key(code) {
                Some(keystroke) => {
                    self.taken = Some(Taken::Media(code));
                    Verdict {
                        swallow: true,
                        heard: Some(Ok(keystroke)),
                    }
                }
                // Caps lock sends one of these too, and no media key this
                // program can post does: wait on for a key.
                None => Verdict::pass(),
            },
            (Some(Taken::Key(taken)), Seen::KeyDown { code, .. }) if code == taken => {
                Verdict::swallow()
            }
            (Some(Taken::Key(taken)), Seen::KeyUp { code }) if code == taken => {
                self.released = true;
                Verdict::swallow()
            }
            (Some(Taken::Media(taken)), Seen::Media { code, down }) if code == taken => {
                self.released = !down;
                Verdict::swallow()
            }
            _ => Verdict::pass(),
        }
    }
}

/// Why a key cannot be recorded: this program has no name for it, and so
/// cannot post it.
fn unnamed(code: u16) -> String {
    format!(
        "Mi Band Bridge has no name for that key (key code {code}), so it cannot send it. Pick \
         a key from the list."
    )
}

/// What the tap's callback needs, on the tap's own thread.
struct Tap {
    listener: RefCell<Listener>,
    session: Arc<Session>,
    /// The tap itself, to turn back on if macOS turns it off. Set once the
    /// tap exists, and valid for as long as the run loop runs it.
    port: Cell<Option<NonNull<CFMachPort>>>,
    /// When the key was taken, to give up on its up after a while.
    took: Cell<Option<Instant>>,
}

/// Hold a tap for one recording, on this thread's run loop, until the
/// recording is over: a key was taken and came up, the recording stopped,
/// or it ran out of time.
pub(super) fn listen(session: Arc<Session>) {
    // Stopped before its tap was made: it makes none.
    if session.stopped() {
        return;
    }
    let own = i64::from(std::process::id());
    let tap = Box::new(Tap {
        listener: RefCell::new(Listener::new(own)),
        session: session.clone(),
        port: Cell::new(None),
        took: Cell::new(None),
    });
    let info = Box::into_raw(tap);

    // SAFETY: `heard` matches the callback's signature, and `info` stays
    // valid until the tap is gone: it is freed only after the run loop that
    // calls the callback has let the tap go.
    let port = unsafe {
        CGEvent::tap_create(
            CGEventTapLocation::SessionEventTap,
            CGEventTapPlacement::HeadInsertEventTap,
            CGEventTapOptions::Default,
            LISTENS_FOR,
            Some(heard),
            info.cast(),
        )
    };
    let Some(port) = port else {
        session.answer(Answer::Deaf(Deaf::NoTap));
        // SAFETY: from Box::into_raw above, and no tap holds it.
        drop(unsafe { Box::from_raw(info) });
        return;
    };
    // SAFETY: `info` is valid, and nothing else reads it until the run loop
    // runs the tap.
    let tap = unsafe { &*info };
    tap.port.set(Some(NonNull::from(&*port)));

    let source = CFMachPort::new_run_loop_source(None, Some(&*port), 0);
    let run_loop = CFRunLoop::current();
    // SAFETY: the modes are CoreFoundation's own constants.
    let (common, default) = unsafe { (kCFRunLoopCommonModes, kCFRunLoopDefaultMode) };
    if let (Some(source), Some(run_loop)) = (source.as_deref(), run_loop.as_deref()) {
        run_loop.add_source(Some(source), common);
        CGEvent::tap_enable(&port, true);
        let deadline = Instant::now() + RECORD_LIMIT + Duration::from_secs(1);
        while !session.stopped() {
            CFRunLoop::run_in_mode(default, LOOK_EVERY, false);
            let released = tap.listener.try_borrow().is_ok_and(|l| l.released);
            let gave_up = tap
                .took
                .get()
                .is_some_and(|at| at.elapsed() > RELEASE_LIMIT);
            if released || gave_up || Instant::now() > deadline {
                break;
            }
        }
        CGEvent::tap_enable(&port, false);
        run_loop.remove_source(Some(source), common);
    } else {
        session.answer(Answer::Deaf(Deaf::NoTap));
    }
    port.invalidate();
    tap.port.set(None);
    drop(source);
    drop(port);
    // Nobody waits for this recording any more.
    session.answer(Answer::Stopped);
    // SAFETY: from Box::into_raw above. The tap is gone, so the callback
    // can no longer run with it.
    drop(unsafe { Box::from_raw(info) });
}

/// The tap's callback. It is in the way of every key typed while the tap
/// exists, so it does little: it reads the event, asks the listener, and
/// answers the recording once.
unsafe extern "C-unwind" fn heard(
    _proxy: CGEventTapProxy,
    kind: CGEventType,
    event: NonNull<CGEvent>,
    info: *mut c_void,
) -> *mut CGEvent {
    // SAFETY: `info` is the Tap that `listen` made for this tap, and it
    // outlives the tap.
    let tap = unsafe { &*(info as *const Tap) };
    if kind == CGEventType::TapDisabledByTimeout || kind == CGEventType::TapDisabledByUserInput {
        // macOS turns off a tap that is slow to answer. Turn it back on.
        if let Some(port) = tap.port.get() {
            // SAFETY: set by `listen` while the tap exists.
            CGEvent::tap_enable(unsafe { port.as_ref() }, true);
        }
        return event.as_ptr();
    }
    // A recording that was stopped takes nothing more. Its tap is going.
    if tap.session.stopped() {
        return event.as_ptr();
    }
    // SAFETY: macOS hands the callback a valid event.
    let cg = unsafe { event.as_ref() };
    let source = CGEvent::integer_value_field(Some(cg), CGEventField::EventSourceUnixProcessID);
    let seen = objc2::rc::autoreleasepool(|_| read(kind, cg));
    let Ok(mut listener) = tap.listener.try_borrow_mut() else {
        return event.as_ptr();
    };
    let verdict = listener.hear(source, seen);
    drop(listener);
    if let Some(heard) = verdict.heard {
        tap.took.set(Some(Instant::now()));
        tap.session.answer(Answer::Heard(heard));
    }
    if verdict.swallow {
        std::ptr::null_mut()
    } else {
        event.as_ptr()
    }
}

/// What an event is, for the listener.
fn read(kind: CGEventType, event: &CGEvent) -> Seen {
    let field = |name| CGEvent::integer_value_field(Some(event), name);
    let code = || u16::try_from(field(CGEventField::KeyboardEventKeycode)).unwrap_or(u16::MAX);
    match kind {
        CGEventType::KeyDown => Seen::KeyDown {
            code: code(),
            flags: CGEvent::flags(Some(event)).0,
            repeat: field(CGEventField::KeyboardEventAutorepeat) != 0,
        },
        CGEventType::KeyUp => Seen::KeyUp { code: code() },
        SYSTEM_DEFINED => {
            media(event).map_or(Seen::Other, |(code, down)| Seen::Media { code, down })
        }
        _ => Seen::Other,
    }
}

/// A media key in a system-defined event: its NX_KEYTYPE, and whether it
/// went down. The key is in the high 16 bits of the event's data1, and its
/// state in the next byte: 0xA down, 0xB up.
fn media(event: &CGEvent) -> Option<(u16, bool)> {
    let event = NSEvent::eventWithCGEvent(event)?;
    if event.subtype().0 != 8 {
        return None;
    }
    media_in(event.data1() as i64)
}

/// The media key and its state in a system-defined event's data1.
fn media_in(data1: i64) -> Option<(u16, bool)> {
    let code = u16::try_from((data1 >> 16) & 0xffff).ok()?;
    match (data1 >> 8) & 0xff {
        0xa => Some((code, true)),
        0xb => Some((code, false)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recorder::Recorded;

    const OWN: i64 = 4242;
    const KEYBOARD: i64 = 0;
    const SHIFT: u64 = 0x0002_0000;
    const COMMAND: u64 = 0x0010_0000;
    const FN: u64 = 0x0080_0000;

    fn down(code: u16, flags: u64) -> Seen {
        Seen::KeyDown {
            code,
            flags,
            repeat: false,
        }
    }

    fn keystroke(key: &str, modifiers: &[&str]) -> Keystroke {
        Keystroke {
            key_code: Some(key.to_string()),
            consumer_key_code: None,
            modifiers: modifiers.iter().map(|m| m.to_string()).collect(),
        }
    }

    fn heard(keystroke: Keystroke) -> Verdict {
        Verdict {
            swallow: true,
            heard: Some(Ok(keystroke)),
        }
    }

    /// Command shift 4, as a keyboard sends it: the key, with its modifiers
    /// in its flags. The key is heard, and it and its up are swallowed, so
    /// the app that owns the shortcut never starts. The rest passes.
    #[test]
    fn the_first_key_is_heard_with_its_modifiers_and_swallowed_down_and_up() {
        let mut listener = Listener::new(OWN);
        assert_eq!(
            listener.hear(KEYBOARD, down(21, SHIFT | COMMAND)),
            heard(keystroke("4", &["command", "shift"]))
        );
        assert!(!listener.released);
        // Held down, it repeats: swallowed, and heard once only.
        let repeat = Seen::KeyDown {
            code: 21,
            flags: SHIFT | COMMAND,
            repeat: true,
        };
        assert_eq!(listener.hear(KEYBOARD, repeat), Verdict::swallow());
        // Another key meanwhile passes on.
        assert_eq!(listener.hear(KEYBOARD, down(40, 0)), Verdict::pass());
        assert_eq!(
            listener.hear(KEYBOARD, Seen::KeyUp { code: 40 }),
            Verdict::pass()
        );
        assert_eq!(
            listener.hear(KEYBOARD, Seen::KeyUp { code: 21 }),
            Verdict::swallow()
        );
        assert!(listener.released, "the tap can go");
        // After that, nothing is swallowed.
        assert_eq!(listener.hear(KEYBOARD, down(21, 0)), Verdict::pass());
    }

    /// A key this program posts is never recorded, nor swallowed: a band
    /// press or Try it now during a recording types as it should.
    #[test]
    fn a_key_this_program_posts_is_left_alone() {
        let mut listener = Listener::new(OWN);
        assert_eq!(listener.hear(OWN, down(40, COMMAND)), Verdict::pass());
        assert_eq!(
            listener.hear(OWN, Seen::KeyUp { code: 40 }),
            Verdict::pass()
        );
        let media = Seen::Media {
            code: 0,
            down: true,
        };
        assert_eq!(listener.hear(OWN, media), Verdict::pass());
        assert_eq!(listener.taken, None);
        // The next key from the keyboard is the one.
        assert_eq!(
            listener.hear(KEYBOARD, down(40, 0)),
            heard(keystroke("k", &[]))
        );
    }

    /// The recording waits for the first key that is not a modifier, and not
    /// a key held down from before it began, such as the Return that pressed
    /// Record keys.
    #[test]
    fn it_waits_for_a_key_that_is_not_a_modifier_or_held_from_before() {
        let mut listener = Listener::new(OWN);
        // The Return that pressed the button comes up, then repeats.
        assert_eq!(
            listener.hear(KEYBOARD, Seen::KeyUp { code: 36 }),
            Verdict::pass()
        );
        let held = Seen::KeyDown {
            code: 36,
            flags: 0,
            repeat: true,
        };
        assert_eq!(listener.hear(KEYBOARD, held), Verdict::pass());
        // A modifier, should one come as a key, and anything else, pass.
        assert_eq!(listener.hear(KEYBOARD, down(56, SHIFT)), Verdict::pass());
        assert_eq!(listener.hear(KEYBOARD, Seen::Other), Verdict::pass());
        assert_eq!(listener.taken, None);
        assert_eq!(
            listener.hear(KEYBOARD, down(96, FN)),
            heard(keystroke("f5", &[])),
            "fn from a function key is not recorded"
        );
    }

    /// A media key is recorded as a consumer key with no modifiers, and its
    /// down and up are swallowed, so the volume does not change.
    #[test]
    fn a_media_key_is_heard_and_swallowed_down_and_up() {
        let mut listener = Listener::new(OWN);
        // Caps lock sends a system-defined event this program cannot post:
        // it passes, and the recording waits on.
        let caps = Seen::Media {
            code: 4,
            down: true,
        };
        assert_eq!(listener.hear(KEYBOARD, caps), Verdict::pass());
        let up = |down| Seen::Media { code: 0, down };
        assert_eq!(
            listener.hear(KEYBOARD, up(true)),
            heard(Keystroke {
                key_code: None,
                consumer_key_code: Some("volume_increment".to_string()),
                modifiers: Vec::new(),
            })
        );
        assert_eq!(
            listener.hear(KEYBOARD, up(true)),
            Verdict::swallow(),
            "held, it repeats"
        );
        assert!(!listener.released);
        assert_eq!(listener.hear(KEYBOARD, up(false)), Verdict::swallow());
        assert!(listener.released);
    }

    /// A key this program has no name for cannot be posted: the recording
    /// says so, and swallows it, as it would a key it could name.
    #[test]
    fn a_key_with_no_name_says_why() {
        let mut listener = Listener::new(OWN);
        let verdict = listener.hear(KEYBOARD, down(72, 0));
        assert!(verdict.swallow);
        let why = verdict.heard.unwrap().unwrap_err();
        assert!(
            why.contains("key code 72") && why.contains("from the list"),
            "{why}"
        );
        assert_eq!(
            listener.hear(KEYBOARD, Seen::KeyUp { code: 72 }),
            Verdict::swallow()
        );
    }

    /// With no input permission, the tap hears nothing, so the page listens.
    /// Secure Event Input keeps keys from every tap, so the page listens then
    /// too.
    #[test]
    fn the_page_listens_when_the_tap_cannot_hear() {
        assert_eq!(deafness(false, false), Some(Deaf::Permission));
        assert_eq!(deafness(false, true), Some(Deaf::Permission));
        assert_eq!(deafness(true, true), Some(Deaf::SecureInput));
        assert_eq!(deafness(true, false), None);
    }

    /// The answer reaches the page as the keystroke in the file's shape, or
    /// as where to listen instead.
    #[test]
    fn the_answer_is_the_keystroke_in_the_files_shape_or_where_to_listen() {
        let json = |recorded| serde_json::to_value(recorded).unwrap();
        assert_eq!(
            json(Recorded::Heard(keystroke("4", &["command", "shift"]))),
            serde_json::json!({"heard": {"key_code": "4", "modifiers": ["command", "shift"]}})
        );
        assert_eq!(
            json(Recorded::Heard(keys::heard_media_key(16).unwrap())),
            serde_json::json!({"heard": {"consumer_key_code": "play_or_pause"}})
        );
        for (deaf, word) in [
            (Deaf::Permission, "permission"),
            (Deaf::SecureInput, "secure_input"),
            (Deaf::NoTap, "no_tap"),
        ] {
            assert_eq!(
                json(Recorded::Page(deaf)),
                serde_json::json!({"page": word})
            );
        }
    }

    /// A media key's code and state, from the event's data1.
    #[test]
    fn a_media_key_is_read_from_its_data1() {
        // Volume up, down and up, as B-009b's taps read them.
        assert_eq!(media_in(0x0000_0a00), Some((0, true)));
        assert_eq!(media_in(0x0000_0b00), Some((0, false)));
        assert_eq!(media_in(0x0010_0a01), Some((16, true)), "the repeat bit");
        assert_eq!(media_in(0x0001_0c00), None);
    }
}
