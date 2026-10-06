//! Doing what an action says, when its signal arrives.
//!
//! A keystroke is instant, so it is posted on the spot. A shell command, a
//! script, a shortcut, or a web request can take seconds, or hang, so each one
//! runs in a task of its own with a deadline. The band link never waits for
//! any of them.
//!
//! Each one ends with an [`Outcome`]: the HTTP status for a web request, the
//! exit code for a shell command or a script, done for a keystroke or a
//! shortcut, or the error. The window's event log shows it beside the press.
//! On Windows a script that is not a .ps1 opens as a double-click opens it,
//! which waits for no exit code, and is done once it has started.

use crate::config::{Does, Keystroke, WebRequest};
use crate::platform::actions;
use serde::Serialize;
use std::ffi::OsStr;
use std::fmt;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;
#[cfg(target_os = "macos")]
use tokio::process::Command;
use tokio::task::JoinHandle;

/// How long an action may take before it is given up on.
///
/// A wrist button is a thing you press and walk away from, so a command that
/// hangs must not pile up one stuck task per press.
pub const DEADLINE: Duration = Duration::from_secs(10);

/// Why a keystroke that was posted went nowhere.
const NOT_ALLOWED: &str = "macOS does not let this program post key events, so the keystroke \
     went nowhere. Allow it in System Settings, Privacy & Security, Device Control and Data \
     Access.";

/// One client for every call, so a second press reuses the open connection.
fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(DEADLINE)
            .build()
            .unwrap_or_default()
    })
}

/// How an action went: what the window's event log shows beside a press.
///
/// The page gets it as JSON, with `type` and `ok` always, `code` for a
/// status or an exit, and `error` for an error, and for an exit that failed
/// and said why: `{"type":"status","ok":true,"code":204}`. See the
/// [`Serialize`] impl.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// A keystroke was posted, or a shortcut ran to its end. On Windows, or
    /// a file opened as a double-click opens it.
    Done,
    /// A web request was answered, with this HTTP status.
    Status(u16),
    /// A shell command or a script ended, with this exit code. One that
    /// failed brings the first line it wrote to stderr, when it wrote one.
    Exit { code: i32, error: Option<String> },
    /// The action could not run, or did not finish: why, in words fit to
    /// show.
    Error(String),
}

impl Outcome {
    /// Whether the action did what it says: done, an answer below 400, or an
    /// exit code of 0.
    pub fn ok(&self) -> bool {
        match self {
            Outcome::Done => true,
            Outcome::Status(code) => *code < 400,
            Outcome::Exit { code, .. } => *code == 0,
            Outcome::Error(_) => false,
        }
    }

    /// The word the wire uses for the kind of outcome.
    pub fn word(&self) -> &'static str {
        match self {
            Outcome::Done => "done",
            Outcome::Status(_) => "status",
            Outcome::Exit { .. } => "exit",
            Outcome::Error(_) => "error",
        }
    }
}

/// For the terminal: `done`, `204 No Content`, `exit 3: no such thing`, or
/// the error. A command that exits with 0 reads `done`, as it always has.
impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Outcome::Done | Outcome::Exit { code: 0, .. } => f.write_str("done"),
            Outcome::Status(code) => {
                let reason = reqwest::StatusCode::from_u16(*code)
                    .ok()
                    .and_then(|status| status.canonical_reason());
                match reason {
                    Some(reason) => write!(f, "{code} {reason}"),
                    None => write!(f, "{code}"),
                }
            }
            Outcome::Exit { code, error } => {
                f.write_str(&with_reason(format!("exit {code}"), error.clone()))
            }
            Outcome::Error(why) => f.write_str(why),
        }
    }
}

/// `{"type":"exit","ok":false,"code":3,"error":"no such thing"}`. The page
/// reads `ok` for the tone, and never has to know which codes are good.
impl Serialize for Outcome {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("type", self.word())?;
        map.serialize_entry("ok", &self.ok())?;
        match self {
            Outcome::Done => {}
            Outcome::Status(code) => map.serialize_entry("code", code)?,
            Outcome::Exit { code, error } => {
                map.serialize_entry("code", code)?;
                if let Some(error) = error {
                    map.serialize_entry("error", error)?;
                }
            }
            Outcome::Error(why) => map.serialize_entry("error", why)?,
        }
        map.end()
    }
}

/// An action, once started.
#[derive(Debug)]
pub enum Started {
    /// Over already, as a keystroke is: how it went.
    Finished(Outcome),
    /// Running in a task of its own, which ends within [`DEADLINE`] and
    /// gives how it went.
    Running(JoinHandle<Outcome>),
}

/// Start doing what an action says, and return at once.
///
/// A keystroke is posted here, on the spot. Anything slower carries on in a
/// task of its own, with a deadline, and the caller awaits its outcome.
pub fn start(does: &Does) -> Started {
    match does {
        Does::Keystroke(keystroke) => Started::Finished(press_keys(keystroke)),
        _ => Started::Running(spawn_within(does.clone(), DEADLINE)),
    }
}

/// Do what an action says, once, now, and wait for how it went: Try it now,
/// in the window's editor. It runs as a press runs it, with the same
/// deadline, and ends in the same [`Outcome`].
///
/// A keystroke goes at once to the app in front, so the window counts down
/// before it asks (D8).
pub async fn run_once(does: &Does) -> Outcome {
    match start(does) {
        Started::Finished(outcome) => outcome,
        Started::Running(task) => finished(task).await,
    }
}

/// How an action running in a task of its own went. A task that stopped part
/// way is the error.
pub async fn finished(task: JoinHandle<Outcome>) -> Outcome {
    task.await
        .unwrap_or_else(|err| Outcome::Error(format!("the action stopped part way: {err}")))
}

/// The shortcuts in Shortcuts, by the names a `shortcut` action runs them
/// by: the window's picker. Windows has no Shortcuts, and says so.
pub use crate::platform::actions::shortcut_names;

/// Post a keystroke, and say how it went.
fn press_keys(keystroke: &Keystroke) -> Outcome {
    let posted = crate::keys::fire(keystroke);
    keystroke_outcome(posted, crate::keys::trusted(false))
}

/// How a keystroke went, from whether it was posted and whether macOS lets
/// this program post keys. A post cannot tell the second: without the
/// permission, macOS drops the keystroke and says nothing.
fn keystroke_outcome(posted: Result<(), String>, allowed: bool) -> Outcome {
    match posted {
        Err(why) => Outcome::Error(why),
        Ok(()) if !allowed => Outcome::Error(NOT_ALLOWED.to_string()),
        Ok(()) => Outcome::Done,
    }
}

/// Run an action that takes time in a task of its own, and give up on it
/// after `deadline`.
fn spawn_within(does: Does, deadline: Duration) -> JoinHandle<Outcome> {
    tokio::spawn(async move {
        tokio::time::timeout(deadline, perform(does))
            .await
            .unwrap_or_else(|_| gave_up(deadline))
    })
}

fn gave_up(after: Duration) -> Outcome {
    Outcome::Error(format!("gave up after {} seconds", after.as_secs()))
}

/// Carry out one action that takes time, and say how it went.
///
/// A shell command, a script and a shortcut run the way this system runs
/// them: see [`crate::platform::actions`].
async fn perform(does: Does) -> Outcome {
    match does {
        // Posted on the spot by `start`, never in a task.
        Does::Keystroke(_) => Outcome::Error("a keystroke is posted on the spot".to_string()),
        Does::Shell(line) => actions::shell(line).await,
        Does::Script(path) => actions::script(path).await,
        Does::Shortcut(name) => actions::shortcut(name).await,
        Does::WebRequest(request) => call(request).await,
    }
}

/// A leading `~` means the home folder, as it does in a terminal.
///
/// Only `~` alone, or `~` and a separator: `~name` is another person's home,
/// which this does not look up, so it is left as written. Windows takes `\`
/// as a separator as well as `/`.
pub(crate) fn expand_home(path: &str, home: Option<&OsStr>) -> PathBuf {
    match (path.strip_prefix('~'), home) {
        (Some(rest), Some(home)) if rest.is_empty() || rest.starts_with(std::path::is_separator) => {
            let mut full = home.to_os_string();
            full.push(rest);
            PathBuf::from(full)
        }
        _ => PathBuf::from(path),
    }
}

/// How a command ended: its exit code, which is None when a signal stopped
/// it, and the first line of what it wrote to stderr.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Ended {
    pub(crate) code: Option<i32>,
    pub(crate) complaint: Option<String>,
}

/// Run a command to its end. Err when it could not be run at all.
///
/// The Mac's. Windows reads what PowerShell wrote from a file instead: see
/// [`crate::platform::actions`].
#[cfg(target_os = "macos")]
pub(crate) async fn wait_for(mut command: Command) -> Result<Ended, String> {
    // Without this, giving up on the task leaves the command running.
    command.kill_on_drop(true);
    let output = command
        .output()
        .await
        .map_err(|e| format!("cannot run it: {e}"))?;
    Ok(Ended {
        code: output.status.code(),
        complaint: first_line(&output.stderr),
    })
}

/// A shell command or a script: its exit code, with why when it failed.
pub(crate) fn exited(ended: Result<Ended, String>) -> Outcome {
    match ended {
        Ok(Ended { code: Some(0), .. }) => Outcome::Exit {
            code: 0,
            error: None,
        },
        Ok(Ended {
            code: Some(code),
            complaint,
        }) => Outcome::Exit {
            code,
            error: complaint,
        },
        Ok(Ended {
            code: None,
            complaint,
        }) => Outcome::Error(with_reason("stopped by a signal".to_string(), complaint)),
        Err(why) => Outcome::Error(why),
    }
}

/// `head: reason`, or the head alone.
pub(crate) fn with_reason(head: String, reason: Option<String>) -> String {
    match reason {
        Some(reason) => format!("{head}: {reason}"),
        None => head,
    }
}

/// The first line of a command's complaint, which is the part worth showing.
pub(crate) fn first_line(stderr: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(stderr);
    let line = text.lines().find(|l| !l.trim().is_empty())?;
    Some(line.trim().to_string())
}

async fn call(request: WebRequest) -> Outcome {
    let method = request.method();
    let Ok(method) = reqwest::Method::from_bytes(method.as_bytes()) else {
        return Outcome::Error(format!("{method} is not a method"));
    };
    let mut outgoing = client().request(method, &request.url);
    for header in &request.headers {
        // The loader has already checked the shape of every header.
        if let Some((name, value)) = header.split_once(':') {
            outgoing = outgoing.header(name.trim(), value.trim());
        }
    }
    if let Some(body) = request.body {
        outgoing = outgoing.body(body);
    }
    match outgoing.send().await {
        Ok(response) => Outcome::Status(response.status().as_u16()),
        Err(e) => Outcome::Error(format!("failed: {}", deepest_cause(&e))),
    }
}

/// The last error in a chain, which says what went wrong: "Connection
/// refused (os error 61)", where the first says only "client error
/// (Connect)".
fn deepest_cause(error: &dyn std::error::Error) -> String {
    let mut cause = error;
    while let Some(next) = cause.source() {
        cause = next;
    }
    cause.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Action, Config};
    use crate::platform::actions::lines;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    fn shell(line: &str) -> Does {
        Does::Shell(line.to_string())
    }

    /// Start an action that takes time, and wait for how it went.
    async fn outcome_of(does: Does) -> Outcome {
        match start(&does) {
            Started::Running(task) => task.await.expect("the task ran to its end"),
            Started::Finished(outcome) => panic!("{does:?} ended at once: {outcome:?}"),
        }
    }

    /// Each outcome as the page reads it, and as a terminal reads it.
    fn read(outcome: &Outcome) -> (serde_json::Value, String) {
        (serde_json::to_value(outcome).unwrap(), outcome.to_string())
    }

    // --- each result, on the wire and in the terminal -----------------------

    #[test]
    fn a_keystroke_or_a_shortcut_is_done() {
        let (json, shown) = read(&Outcome::Done);
        assert_eq!(json, serde_json::json!({"type": "done", "ok": true}));
        assert_eq!(shown, "done");
    }

    /// The page shows the code. The terminal adds the reason, as before.
    #[test]
    fn a_web_request_is_its_http_status() {
        let (json, shown) = read(&Outcome::Status(204));
        assert_eq!(
            json,
            serde_json::json!({"type": "status", "ok": true, "code": 204})
        );
        assert_eq!(shown, "204 No Content");

        let (json, shown) = read(&Outcome::Status(404));
        assert_eq!(
            json,
            serde_json::json!({"type": "status", "ok": false, "code": 404})
        );
        assert_eq!(shown, "404 Not Found");

        // A redirect that was not followed is an answer, not a failure.
        assert!(Outcome::Status(304).ok());
        assert!(!Outcome::Status(500).ok());
        assert_eq!(Outcome::Status(599).to_string(), "599");
    }

    /// A command is its exit code on the wire. In the terminal, 0 still
    /// reads `done`, and a failure reads as it always has.
    #[test]
    fn a_shell_command_or_a_script_is_its_exit_code() {
        let worked = Outcome::Exit {
            code: 0,
            error: None,
        };
        let (json, shown) = read(&worked);
        assert_eq!(
            json,
            serde_json::json!({"type": "exit", "ok": true, "code": 0})
        );
        assert_eq!(shown, "done");

        let failed = Outcome::Exit {
            code: 3,
            error: Some("no such thing".to_string()),
        };
        let (json, shown) = read(&failed);
        assert_eq!(
            json,
            serde_json::json!({"type": "exit", "ok": false, "code": 3, "error": "no such thing"})
        );
        assert_eq!(shown, "exit 3: no such thing");

        let quiet = Outcome::Exit {
            code: 1,
            error: None,
        };
        let (json, shown) = read(&quiet);
        assert_eq!(
            json,
            serde_json::json!({"type": "exit", "ok": false, "code": 1})
        );
        assert_eq!(shown, "exit 1");
    }

    #[test]
    fn anything_that_did_not_run_or_finish_is_the_error() {
        let (json, shown) = read(&gave_up(DEADLINE));
        assert_eq!(
            json,
            serde_json::json!({"type": "error", "ok": false, "error": "gave up after 10 seconds"})
        );
        assert_eq!(shown, "gave up after 10 seconds");
    }

    // --- each kind of action ------------------------------------------------

    /// A keystroke is done once it is posted and macOS takes it. Without the
    /// permission, macOS drops it without a word, so the permission decides.
    /// These never post a key: a test that pressed keys would type into
    /// whatever window is in front.
    #[test]
    fn a_keystroke_is_done_or_says_why_not() {
        assert_eq!(keystroke_outcome(Ok(()), true), Outcome::Done);
        assert_eq!(
            keystroke_outcome(Ok(()), false),
            Outcome::Error(NOT_ALLOWED.to_string())
        );
        let unmade = "macOS could not make a key event for code 21".to_string();
        assert_eq!(
            keystroke_outcome(Err(unmade.clone()), true),
            Outcome::Error(unmade)
        );
    }

    #[tokio::test]
    async fn a_command_that_works_is_exit_0() {
        assert_eq!(
            outcome_of(shell("exit 0")).await,
            Outcome::Exit {
                code: 0,
                error: None
            }
        );
        // What it says on stderr does not matter once it worked.
        assert_eq!(
            outcome_of(shell(&lines::to_stderr("fine"))).await,
            Outcome::Exit {
                code: 0,
                error: None
            }
        );
    }

    /// The exit code alone does not say what went wrong, so the first line of
    /// the complaint comes with it.
    #[tokio::test]
    async fn a_command_that_fails_gives_its_code_and_why() {
        assert_eq!(
            outcome_of(shell(&format!("{}; exit 3", lines::to_stderr("no such thing")))).await,
            Outcome::Exit {
                code: 3,
                error: Some("no such thing".to_string())
            }
        );
    }

    #[tokio::test]
    async fn a_missing_program_is_reported_rather_than_silent() {
        let outcome = outcome_of(shell("this-program-does-not-exist")).await;
        assert!(
            matches!(
                outcome,
                Outcome::Exit {
                    code: lines::NOT_FOUND,
                    error: Some(_)
                }
            ),
            "{outcome:?}"
        );
        assert!(!outcome.ok());
    }

    /// A shell that never returns must not hold a task forever.
    #[tokio::test]
    async fn a_command_that_hangs_is_given_up_on() {
        let task = spawn_within(shell("sleep 30"), Duration::from_millis(200));
        let outcome = tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .expect("the deadline ended it")
            .unwrap();
        assert_eq!(outcome, gave_up(Duration::from_millis(200)));
        assert!(
            outcome.to_string().starts_with("gave up after"),
            "{outcome}"
        );
    }

    /// A command that a signal stopped has no exit code, so it is an error.
    #[test]
    fn a_command_stopped_by_a_signal_is_an_error() {
        let ended = Ended {
            code: None,
            complaint: Some("Killed".to_string()),
        };
        assert_eq!(
            exited(Ok(ended)),
            Outcome::Error("stopped by a signal: Killed".to_string())
        );
    }

    #[test]
    fn a_leading_tilde_is_the_home_folder() {
        let home = Some(OsStr::new("/Users/someone"));
        assert_eq!(
            expand_home("~/scripts/lamp.sh", home),
            PathBuf::from("/Users/someone/scripts/lamp.sh")
        );
        assert_eq!(expand_home("~", home), PathBuf::from("/Users/someone"));
        for unchanged in ["/opt/lamp.sh", "lamp.sh", "~other/lamp.sh"] {
            assert_eq!(expand_home(unchanged, home), PathBuf::from(unchanged));
        }
        assert_eq!(expand_home("~/lamp.sh", None), PathBuf::from("~/lamp.sh"));
    }

    /// One socket, one canned reply, so the test needs no server crate.
    ///
    /// The request is read whole before the reply. Windows resets a
    /// connection that closes with bytes left unread, and the client would
    /// see the reset in place of the reply.
    async fn stub(status_line: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            read_request(&mut socket).await;
            let reply = format!("HTTP/1.1 {status_line}\r\ncontent-length: 0\r\n\r\n");
            let _ = socket.write_all(reply.as_bytes()).await;
            let _ = socket.flush().await;
        });
        format!("http://{address}/hook")
    }

    /// Read one HTTP request: its head, and as much body as it says it has.
    async fn read_request(socket: &mut TcpStream) {
        let mut seen = Vec::new();
        let mut chunk = [0u8; 1024];
        loop {
            if let Some(end) = seen.windows(4).position(|four| four == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&seen[..end]).to_ascii_lowercase();
                let body = head
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length:"))
                    .and_then(|length| length.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                if seen.len() >= end + 4 + body {
                    return;
                }
            }
            match socket.read(&mut chunk).await {
                Ok(0) | Err(_) => return,
                Ok(read) => seen.extend_from_slice(&chunk[..read]),
            }
        }
    }

    #[tokio::test]
    async fn a_call_gives_the_status_the_server_gave() {
        let request = WebRequest {
            url: stub("204 No Content").await,
            body: Some("{\"on\": true}".to_string()),
            headers: vec!["Content-Type: application/json".to_string()],
            ..WebRequest::default()
        };
        let action = Action {
            name: "Lamp".to_string(),
            signal: "bb04".to_string(),
            does: Does::WebRequest(request.clone()),
        };
        let config = Config {
            actions: vec![action],
            ..Config::default()
        };
        assert!(config.check().is_ok(), "the loader accepts it");
        assert_eq!(
            outcome_of(Does::WebRequest(request)).await,
            Outcome::Status(204)
        );
    }

    /// An answer that is not a success is still the status: the page shows
    /// it, and `ok` says it failed.
    #[tokio::test]
    async fn a_call_the_server_refuses_gives_its_status() {
        let request = WebRequest {
            url: stub("404 Not Found").await,
            ..WebRequest::default()
        };
        let outcome = outcome_of(Does::WebRequest(request)).await;
        assert_eq!(outcome, Outcome::Status(404));
        assert!(!outcome.ok());
    }

    /// The error names what went wrong, not only that the client failed.
    #[tokio::test]
    async fn a_server_that_is_not_there_is_the_error() {
        // Port 1 on loopback answers nothing, and nothing listens there.
        let request = WebRequest {
            url: "http://127.0.0.1:1/hook".to_string(),
            ..WebRequest::default()
        };
        let outcome = outcome_of(Does::WebRequest(request)).await;
        match &outcome {
            Outcome::Error(why) => {
                assert!(why.starts_with("failed: "), "{why}");
                // macOS says the connection was refused, and Windows that the
                // machine actively refused it.
                assert!(why.to_lowercase().contains("refused"), "{why}");
            }
            other => panic!("expected the error, got {other:?}"),
        }
    }

    // --- Try it now ------------------------------------------------------------

    /// A try runs the action once and waits for it, and ends in the result a
    /// press would give, so the editor shows it as the event log would. No
    /// keystroke here: it would type into the window in front.
    #[tokio::test]
    async fn a_try_runs_the_action_once_and_waits_for_how_it_went() {
        let folder = tempfile::tempdir().unwrap();
        let ran = folder.path().join("ran");
        let once = lines::append("x", &ran);
        assert_eq!(
            run_once(&shell(&once)).await,
            Outcome::Exit {
                code: 0,
                error: None
            }
        );
        assert_eq!(std::fs::read_to_string(&ran).unwrap(), "x\n", "it ran once");

        assert_eq!(
            run_once(&shell(&format!("{}; exit 4", lines::to_stderr("the lamp is away")))).await,
            Outcome::Exit {
                code: 4,
                error: Some("the lamp is away".to_string())
            }
        );
        let request = WebRequest {
            url: stub("204 No Content").await,
            ..WebRequest::default()
        };
        assert_eq!(
            run_once(&Does::WebRequest(request)).await,
            Outcome::Status(204)
        );
    }
}
