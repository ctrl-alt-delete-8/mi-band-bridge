//! Record keys, in the action editor: the keys a person presses, heard by the
//! system's own hook for keys, in this process. On macOS that is an active
//! session event tap, as B-009b found, and on Windows a low-level keyboard
//! hook: see `mac.rs` and `windows.rs`.
//!
//! The hook exists only while a recording runs. It sees keys before any app
//! does, so it hears a shortcut another app owns, such as command shift 4 on
//! the Mac or Win Shift S on Windows, and the media keys, which the page never
//! hears. It keeps the key it records from acting: it swallows that key's down
//! and up, and passes every other event on as it came. It leaves alone the
//! keys this program posts itself, so a band press or Try it now during a
//! recording is not recorded. When the hook cannot hear the keys, the page
//! listens instead, and hears only what reaches the window: see [`Deaf`].
//!
//! A recording runs only while the window has the focus: a key typed with
//! another app in front is that app's. It begins only in a window that has
//! the focus, and the window stops every recording as it loses the focus, or
//! closes, whatever the page does. A stop from the page names the recording
//! it ends, so a stop that comes before its recording began ends it as it
//! begins.
//!
//! The window's alone, as every command about actions is (D9).

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bandbridge::config::Keystroke;
use serde::Serialize;
use tokio::sync::oneshot;

#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "macos")]
use mac::{deaf_now, listen};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use self::windows::{deaf_now, listen, rearm};
/// Install the Windows keyboard hook once, at startup. `main` calls it.
#[cfg(windows)]
pub use self::windows::install;

/// How long a recording waits for a key. The hook is in the way of every
/// key typed while it exists, so it does not wait for long.
pub const RECORD_LIMIT: Duration = Duration::from_secs(30);

/// How long the hook stays, once it took a key, for that key to come up. It
/// swallows the up as well, so no app gets half a key. Both the Mac's tap and
/// the Windows hook wait for the up.
#[cfg(any(target_os = "macos", windows))]
const RELEASE_LIMIT: Duration = Duration::from_secs(2);

/// Why a recording that waited too long stopped.
const NO_KEY: &str = "no key came in 30 seconds, so Record keys stopped";

/// What Record keys answers, when it has something to say.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Recorded {
    /// The keys the tap heard, in the file's shape:
    /// `{"heard": {"key_code": "4", "modifiers": ["command", "shift"]}}`, or
    /// `{"heard": {"consumer_key_code": "volume_increment"}}`.
    Heard(Keystroke),
    /// The tap cannot hear the keys, so the page listens itself, and why:
    /// `{"page": "permission"}`.
    Page(Deaf),
}

/// Why the tap or the hook cannot hear the keys, so the page listens itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Deaf {
    /// This program has no input permission, or no Input Monitoring. A tap
    /// without them hears no key.
    #[cfg(target_os = "macos")]
    Permission,
    /// Another program has turned on Secure Event Input, which keeps keys
    /// from every tap.
    #[cfg(target_os = "macos")]
    SecureInput,
    /// The system gave no tap on the Mac, or no keyboard hook on Windows.
    NoTap,
}

/// How a recording ended, for the command that waits on it.
#[derive(Debug)]
enum Answer {
    /// A key was heard: by the Mac's tap, or by the Windows keyboard hook.
    Heard(Result<Keystroke, String>),
    /// The tap cannot hear, so the page listens instead. The Mac's tap alone
    /// answers this: on Windows a hook that did not install is caught by
    /// [`deaf_now`] before a recording begins, so this never travels there.
    #[cfg_attr(windows, allow(dead_code))]
    Deaf(Deaf),
    Stopped,
}

/// One recording, shared by the command that waits for it and the thread
/// that holds its tap.
struct Session {
    /// Set when the recording is over for any reason: its tap then goes.
    stop: AtomicBool,
    /// Whoever says first how the recording ended.
    answer: Mutex<Option<oneshot::Sender<Answer>>>,
}

impl Session {
    fn new() -> (Arc<Self>, oneshot::Receiver<Answer>) {
        let (tx, rx) = oneshot::channel();
        let session = Arc::new(Self {
            stop: AtomicBool::new(false),
            answer: Mutex::new(Some(tx)),
        });
        (session, rx)
    }

    /// Say how the recording ended, unless that was said already.
    fn answer(&self, answer: Answer) {
        let sender = match self.answer.lock() {
            Ok(mut held) => held.take(),
            Err(poisoned) => poisoned.into_inner().take(),
        };
        if let Some(sender) = sender {
            let _ = sender.send(answer);
        }
    }

    /// End the recording, and its tap with it.
    fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        self.answer(Answer::Stopped);
    }

    /// Whether the recording has ended. The Mac's tap and the Windows
    /// media-key thread both read this to know when to let go.
    fn stopped(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }
}

/// The page's number for one recording. A stop names the recording it ends
/// by it, so it can never end a recording that began after it.
pub type Ticket = u64;

/// How many stops that came before their recording are kept. One is all a
/// page needs, since it runs one recording at a time. A stop whose recording
/// already ended is kept as well, and ends nothing.
const EARLY_STOPS_KEPT: usize = 8;

/// The recording that runs, if any, and the stops that came before their
/// recording began. One recording at a time: a new one takes the place of
/// the one before, which answers nothing.
#[derive(Default)]
pub struct Recordings(Mutex<Held>);

#[derive(Default)]
struct Held {
    running: Option<(Ticket, Arc<Session>)>,
    /// Each ends the recording it names as that recording begins.
    stopped_early: VecDeque<Ticket>,
}

impl Recordings {
    /// Take `session` as the recording that runs, unless a stop for its
    /// ticket came first: then it is stopped, and false says so.
    fn begin(&self, ticket: Ticket, session: Arc<Session>) -> bool {
        let mut held = self.held();
        if let Some(at) = held.stopped_early.iter().position(|early| *early == ticket) {
            held.stopped_early.remove(at);
            drop(held);
            session.stop();
            return false;
        }
        let before = held.running.replace((ticket, session));
        drop(held);
        if let Some((_, before)) = before {
            before.stop();
        }
        true
    }

    /// Stop the recording `ticket` names: the editor closed, Record keys was
    /// pressed again, or the page lost the focus. Its tap goes at once. When
    /// that recording has not begun yet, it stops as it begins.
    pub fn stop(&self, ticket: Ticket) {
        let mut held = self.held();
        if held
            .running
            .as_ref()
            .is_some_and(|(running, _)| *running == ticket)
        {
            let (_, session) = held.running.take().expect("checked above");
            drop(held);
            session.stop();
            return;
        }
        if held.stopped_early.len() == EARLY_STOPS_KEPT {
            held.stopped_early.pop_front();
        }
        held.stopped_early.push_back(ticket);
    }

    /// Stop the recording that runs, whichever it is: the window lost the
    /// focus or closed, or a page loaded that runs none. Its tap goes at once.
    pub fn stop_all(&self) {
        let running = self.held().running.take();
        if let Some((_, session)) = running {
            session.stop();
        }
    }

    /// Forget this recording, unless another has taken its place.
    fn end(&self, session: &Arc<Session>) {
        let mut held = self.held();
        if held
            .running
            .as_ref()
            .is_some_and(|(_, running)| Arc::ptr_eq(running, session))
        {
            held.running = None;
        }
    }

    fn held(&self) -> std::sync::MutexGuard<'_, Held> {
        match self.0.lock() {
            Ok(held) => held,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

/// Record the next key a person presses: its keystroke, in the file's
/// shape. See [`Recorded`].
///
/// `ticket` is the page's number for this recording, and `in_focus` says
/// whether the window has the focus. A recording begins only in a window
/// that has it. The focus is asked once the recording holds its place, so a
/// focus lost at any moment ends it: before that, here, and after it, by the
/// window's own guard, which stops every recording as the focus goes.
///
/// Null when the recording was stopped, or another took its place. An error
/// for a key this program has no name for, and for a recording that heard no
/// key in [`RECORD_LIMIT`].
pub async fn record(
    recordings: &Recordings,
    ticket: Ticket,
    in_focus: impl FnOnce() -> bool,
) -> Result<Option<Recorded>, String> {
    let (session, answered) = Session::new();
    if !recordings.begin(ticket, session.clone()) {
        return Ok(None);
    }
    if !in_focus() {
        recordings.end(&session);
        session.stop();
        return Ok(None);
    }

    if let Some(deaf) = deaf_now() {
        recordings.end(&session);
        return Ok(Some(Recorded::Page(deaf)));
    }

    let listening = session.clone();
    let started = std::thread::Builder::new()
        .name("record keys".to_string())
        .spawn(move || listen(listening));
    if let Err(err) = started {
        recordings.end(&session);
        return Err(format!("Record keys could not start: {err}"));
    }

    let answer = tokio::time::timeout(RECORD_LIMIT, answered).await;
    recordings.end(&session);
    match answer {
        Ok(Ok(Answer::Heard(heard))) => heard.map(|keys| Some(Recorded::Heard(keys))),
        Ok(Ok(Answer::Deaf(deaf))) => Ok(Some(Recorded::Page(deaf))),
        Ok(Ok(Answer::Stopped)) | Ok(Err(_)) => Ok(None),
        Err(_) => {
            session.stop();
            // On Windows the hook may be what heard nothing: Windows removes
            // a low-level hook that answers too slowly. Install it again, so
            // the next recording hears keys.
            #[cfg(windows)]
            rearm();
            Err(NO_KEY.to_string())
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    fn keystroke(key: &str, modifiers: &[&str]) -> Keystroke {
        Keystroke {
            key_code: Some(key.to_string()),
            consumer_key_code: None,
            modifiers: modifiers.iter().map(|m| m.to_string()).collect(),
        }
    }

    /// The recording that runs, by its ticket.
    fn running(recordings: &Recordings) -> Option<Ticket> {
        recordings
            .held()
            .running
            .as_ref()
            .map(|(ticket, _)| *ticket)
    }

    /// One recording at a time: a new one stops the one before, which
    /// answers nothing, and a stop ends the one it names.
    #[tokio::test]
    async fn one_recording_at_a_time() {
        let recordings = Recordings::default();
        let (first, mut first_answer) = Session::new();
        assert!(recordings.begin(1, first.clone()));
        let (second, mut second_answer) = Session::new();
        assert!(recordings.begin(2, second.clone()));
        assert!(first.stopped());
        assert!(matches!(first_answer.try_recv(), Ok(Answer::Stopped)));
        assert!(!second.stopped());

        // The first ends late, and leaves the second in its place.
        recordings.end(&first);
        assert_eq!(running(&recordings), Some(2));

        // A heard key is answered once: a stop after it says nothing more.
        second.answer(Answer::Heard(Ok(keystroke("k", &[]))));
        recordings.stop(2);
        assert!(second.stopped(), "its tap goes");
        assert!(matches!(second_answer.try_recv(), Ok(Answer::Heard(Ok(_)))));
        assert_eq!(running(&recordings), None);
    }

    /// A stop can reach the app before the recording it ends has begun. It
    /// then ends that recording as it begins, before any tap is made, and
    /// ends nothing else: the next recording runs.
    #[tokio::test]
    async fn a_stop_that_comes_first_ends_the_recording_it_names() {
        let recordings = Recordings::default();
        recordings.stop(7);
        let answer = record(&recordings, 7, || {
            panic!("a recording that was stopped asked for the focus")
        })
        .await;
        assert_eq!(answer, Ok(None));
        assert_eq!(running(&recordings), None);

        // The stop was for 7 alone, and it is spent.
        let (next, _answer) = Session::new();
        assert!(recordings.begin(8, next.clone()));
        assert!(!next.stopped());
        assert_eq!(running(&recordings), Some(8));
        let (again, _answer) = Session::new();
        assert!(recordings.begin(7, again.clone()), "7 stops once");

        // A stop for a recording that is not the one running leaves it be.
        recordings.stop(6);
        assert!(!again.stopped());
        assert_eq!(running(&recordings), Some(7));
    }

    /// A recording begins only in a window that has the focus, and takes no
    /// place when it does not.
    #[tokio::test]
    async fn a_recording_begins_only_in_a_window_with_the_focus() {
        let recordings = Recordings::default();
        let answer = record(&recordings, 1, || false).await;
        assert_eq!(answer, Ok(None));
        assert_eq!(running(&recordings), None);
    }

    /// The window losing the focus, or closing, stops the recording that
    /// runs, whichever it is. A stop that came early still waits for its
    /// recording.
    #[tokio::test]
    async fn the_window_stops_whatever_records() {
        let recordings = Recordings::default();
        recordings.stop(3);
        let (session, mut answer) = Session::new();
        assert!(recordings.begin(2, session.clone()));
        recordings.stop_all();
        assert!(session.stopped(), "its tap goes");
        assert!(matches!(answer.try_recv(), Ok(Answer::Stopped)));
        assert_eq!(running(&recordings), None);
        recordings.stop_all();

        let (late, _answer) = Session::new();
        assert!(!recordings.begin(3, late.clone()));
        assert!(late.stopped());
    }

    /// Only so many early stops are kept, the oldest going first. A page
    /// sends one for each recording it stops.
    #[test]
    fn early_stops_are_kept_to_a_few() {
        let recordings = Recordings::default();
        for ticket in 0..(EARLY_STOPS_KEPT as Ticket + 2) {
            recordings.stop(ticket);
        }
        let kept: Vec<Ticket> = recordings.held().stopped_early.iter().copied().collect();
        assert_eq!(kept.len(), EARLY_STOPS_KEPT);
        assert_eq!(kept.first(), Some(&2));
    }
}
