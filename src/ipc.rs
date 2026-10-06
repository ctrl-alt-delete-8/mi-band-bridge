//! Reaching the process that holds the band.
//!
//! A Bluetooth link belongs to one process, so the window and the command line
//! cannot both hold the band. Whichever holds it listens on a socket, and the
//! other sends its request there. The socket carries requests only. Both sides
//! call the same functions in this crate, so a button and a command do the same
//! work by the same path.
//!
//! On Windows a named pipe takes the socket's place, with the same messages
//! (W6). [`crate::platform::link`] holds both, and this module speaks the
//! protocol over whichever this system has.
//!
//! With no socket, a command does the work itself. That is the plain case, and
//! it is what happens when no window is open.
//!
//! Some requests are the window's alone: pairing, forgetting, what a forget
//! kept, and the key. The window makes them on its own channel to the band
//! task, and the socket refuses them, so no other program can pair a band,
//! forget one, or put its key on the clipboard.

use crate::config::Setup;
use crate::error::BandError;
use crate::platform::link;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot};

/// An auth key as a person gave it, before it is checked.
///
/// It travels inside a pair request and nowhere else: never in a reply, a log
/// line, an error or an event. Its Debug form says so rather than show it, so
/// a request printed for debugging carries no key.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct KeyText(String);

impl KeyText {
    pub fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    /// The text, for [`crate::config::AuthKey::from_hex`] to check.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for KeyText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("KeyText(redacted)")
    }
}

/// What one side asks the holder to do.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "request", rename_all = "snake_case")]
pub enum Request {
    /// Report the connection state.
    Status,
    /// Connect, and stay connected.
    Connect,
    /// Disconnect, and stay disconnected.
    Disconnect,
    /// Read or arrange one of the band's screen lists.
    Screens {
        list: String,
        /// The entries to show, in this order. An empty list shows nothing,
        /// and None leaves what the band shows, less `hide`.
        set: Option<Vec<String>>,
        hide: Vec<String>,
        tidy: bool,
    },
    /// Install an app package.
    Install { file: String },
    /// List the apps on the band, or remove one. `all` shows the packages
    /// that were not installed from here as well.
    Apps { remove: Option<String>, all: bool },
    /// Follow band events until the caller goes away.
    Follow,
    /// One screen list, as data for a UI to show and reorder.
    ScreenList { list: String },
    /// The apps on the band, as data for a UI to show and remove.
    AppList { all: bool },
    /// What this host knows about the band: its address, what it is running,
    /// and its battery. Data for a UI to show.
    Device,
    /// Pair the band that takes this key, and call it by this name. `setup`
    /// says what happens to the actions and settings a forget kept: keep them,
    /// which is the default, or delete them and start fresh. The choice
    /// applies only once a band has taken the key.
    Pair {
        key: KeyText,
        name: String,
        #[serde(default)]
        setup: Setup,
    },
    /// Forget the paired band: its key leaves the system's secret store, and
    /// `[device]` leaves the config. `setup` keeps or deletes the actions and
    /// settings. There is no default: deleting cannot be undone.
    Forget { setup: Setup },
    /// The paired band, and what a forget kept, as data for a UI: see
    /// [`crate::config::SavedSetup`].
    SavedSetup,
    /// Put the paired band's key on the clipboard. The window's Copy key
    /// button does this itself, and the band task has no clipboard, so
    /// nothing answers it but a refusal. No reply ever holds the key.
    CopyKey,
}

impl Request {
    /// The request's name, when only the BandBridge window may make it.
    ///
    /// These pair a band, forget one, say what a forget kept, and reach the
    /// key. The window sends them on its own channel to the band task, and
    /// the socket refuses them.
    pub fn window_only(&self) -> Option<&'static str> {
        match self {
            Request::Pair { .. } => Some("pair"),
            Request::Forget { .. } => Some("forget"),
            Request::SavedSetup => Some("saved_setup"),
            Request::CopyKey => Some("copy_key"),
            _ => None,
        }
    }
}

/// What the holder answers. A request may answer with several lines.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "reply", rename_all = "snake_case")]
pub enum Reply {
    /// A line meant for a person to read.
    Line { text: String },
    /// A structured answer, as JSON, for a caller that is not a person.
    Data { json: String },
    /// The request is finished.
    Done,
    /// The request failed, with a reason fit to show.
    Failed { reason: String },
}

/// One request, and the channel its answers go back on.
pub struct Incoming {
    pub request: Request,
    pub reply: mpsc::Sender<Reply>,
}

/// Listen for requests, and hand each one to the holder, except those only
/// the window may make, which are refused by name.
///
/// See [`link::listen`] for what happens to a socket or a pipe a process left
/// behind.
pub async fn serve(requests: mpsc::Sender<Incoming>) -> Result<(), BandError> {
    serve_on(link::listen().await?, requests).await
}

/// [`serve`], on a link already open.
pub(crate) async fn serve_on(
    mut listener: link::Listener,
    requests: mpsc::Sender<Incoming>,
) -> Result<(), BandError> {
    loop {
        let Ok(stream) = listener.accept().await else {
            continue;
        };
        let requests = requests.clone();
        tokio::spawn(async move {
            if let Err(err) = handle(stream, requests).await {
                tracing::debug!("a caller went away: {err}");
            }
        });
    }
}

/// Answer one caller: a request a line, and each answer a line.
async fn handle<S>(stream: S, requests: mpsc::Sender<Incoming>) -> Result<(), BandError>
where
    S: AsyncRead + AsyncWrite,
{
    let (read, mut write) = tokio::io::split(stream);
    let mut lines = BufReader::new(read).lines();

    while let Ok(Some(line)) = lines.next_line().await {
        let request: Request = match serde_json::from_str(&line) {
            Ok(request) => request,
            Err(err) => {
                send(&mut write, &Reply::Failed { reason: unreadable(&err) }).await?;
                continue;
            }
        };
        if let Some(name) = request.window_only() {
            send(&mut write, &Reply::Failed { reason: window_only(name) }).await?;
            continue;
        }

        let (tx, mut rx) = mpsc::channel(32);
        if requests.send(Incoming { request, reply: tx }).await.is_err() {
            send(
                &mut write,
                &Reply::Failed {
                    reason: "the holder is gone".to_string(),
                },
            )
            .await?;
            return Ok(());
        }
        while let Some(reply) = rx.recv().await {
            let last = matches!(reply, Reply::Done | Reply::Failed { .. });
            send(&mut write, &reply).await?;
            if last {
                break;
            }
        }
    }
    Ok(())
}

/// Why a request could not be read, fit to send back.
///
/// serde quotes the value it could not take, and a request can carry a key: a
/// key in the wrong field, or written as a number, would come back whole. So
/// the reason loses every long run of hex digits, as a config message does.
fn unreadable(err: &serde_json::Error) -> String {
    crate::config::redact(&format!("bad request: {err}"))
}

/// Why the socket, or the pipe on Windows, refuses a request only the window
/// may make.
pub(crate) fn window_only(name: &str) -> String {
    format!(
        "{name} is for the Mi Band Bridge window only, and the {} refuses it. Open \
         Mi Band Bridge, and use its window.",
        link::CALLED
    )
}

async fn send<W: AsyncWriteExt + Unpin>(write: &mut W, reply: &Reply) -> Result<(), BandError> {
    let mut line = serde_json::to_string(reply)
        .map_err(|err| BandError::Config(format!("cannot encode a reply: {err}")))?;
    line.push('\n');
    write
        .write_all(line.as_bytes())
        .await
        .map_err(|err| BandError::Config(format!("cannot answer: {err}")))?;
    Ok(())
}

/// Send a request to the holder, printing each answer as it arrives.
///
/// Returns `Ok(false)` when nothing is holding the band, so the caller can do
/// the work itself.
pub async fn forward(request: &Request) -> Result<bool, BandError> {
    let Some(stream) = link::connect().await else {
        return Ok(false);
    };

    let (read, mut write) = tokio::io::split(stream);
    let mut line = serde_json::to_string(request)
        .map_err(|err| BandError::Config(format!("cannot encode the request: {err}")))?;
    line.push('\n');
    write
        .write_all(line.as_bytes())
        .await
        .map_err(|err| BandError::Config(format!("cannot reach the holder: {err}")))?;

    let mut lines = BufReader::new(read).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        match serde_json::from_str::<Reply>(&line) {
            Ok(Reply::Line { text }) => println!("{text}"),
            // A person reading the terminal gets the JSON as-is; the window
            // parses it instead.
            Ok(Reply::Data { json }) => println!("{json}"),
            Ok(Reply::Done) => return Ok(true),
            Ok(Reply::Failed { reason }) => return Err(BandError::Config(reason)),
            Err(err) => tracing::debug!("reply not understood: {err}"),
        }
    }
    Ok(true)
}

/// A holder that answers requests, used to drive one from a caller's side.
#[derive(Clone)]
pub struct Answer(pub mpsc::Sender<Reply>);

impl Answer {
    pub async fn line(&self, text: impl Into<String>) {
        let _ = self.0.send(Reply::Line { text: text.into() }).await;
    }

    pub async fn done(&self) {
        let _ = self.0.send(Reply::Done).await;
    }

    pub async fn failed(&self, reason: impl Into<String>) {
        let _ = self
            .0
            .send(Reply::Failed {
                reason: reason.into(),
            })
            .await;
    }

    /// Finish from a result, so a handler reads as one line.
    pub async fn finish(&self, result: Result<(), BandError>) {
        match result {
            Ok(()) => self.done().await,
            Err(err) => self.failed(err.to_string()).await,
        }
    }
}

/// Reply into a oneshot, for callers that want the answers collected.
pub type Collected = oneshot::Sender<Vec<Reply>>;

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A list to show that holds nothing still travels as a list, so the band
    /// shows nothing. A request with no list only reads.
    #[test]
    fn a_list_of_nothing_is_not_the_same_as_no_list() {
        let parse = |json: &str| match serde_json::from_str::<Request>(json).unwrap() {
            Request::Screens { set, .. } => set,
            other => panic!("not a screens request: {other:?}"),
        };
        let nothing = r#"{"request":"screens","list":"shortcuts","set":[],"hide":[],"tidy":false}"#;
        assert_eq!(parse(nothing), Some(vec![]));
        let no_list = r#"{"request":"screens","list":"shortcuts","hide":[],"tidy":false}"#;
        assert_eq!(parse(no_list), None);
    }

    #[test]
    fn requests_round_trip_as_json() {
        let cases = [
            Request::Status,
            Request::Connect,
            Request::Install {
                file: "band-app/dist/band-app.bin".to_string(),
            },
            Request::Screens {
                list: "menu".to_string(),
                set: None,
                hide: vec!["music".to_string()],
                tidy: false,
            },
            Request::Pair {
                key: KeyText::new(FAKE_KEY),
                name: "Xiaomi Smart Band 7".to_string(),
                setup: Setup::Delete,
            },
            Request::Forget { setup: Setup::Keep },
            Request::SavedSetup,
            Request::CopyKey,
        ];
        for case in cases {
            let text = serde_json::to_string(&case).expect("encodes");
            let back: Request = serde_json::from_str(&text).expect("decodes");
            assert_eq!(case, back);
        }
    }

    /// A key for the tests. It opens no band.
    pub(crate) const FAKE_KEY: &str = "000102030405060708090a0b0c0d0e0f";

    /// The pairing requests as a script, or a person with nc, writes them.
    #[test]
    fn pairing_requests_read_as_they_are_written() {
        let line = format!(r#"{{"request":"pair","key":"{FAKE_KEY}","name":"Xiaomi Smart Band 7"}}"#);
        assert_eq!(
            serde_json::from_str::<Request>(&line).unwrap(),
            Request::Pair {
                key: KeyText::new(FAKE_KEY),
                name: "Xiaomi Smart Band 7".to_string(),
                setup: Setup::Keep,
            },
            "with no choice, a pair keeps what a forget kept"
        );
        let line = format!(r#"{{"request":"pair","key":"{FAKE_KEY}","name":"Band","setup":"delete"}}"#);
        assert!(matches!(
            serde_json::from_str::<Request>(&line).unwrap(),
            Request::Pair { setup: Setup::Delete, .. }
        ));

        assert_eq!(
            serde_json::from_str::<Request>(r#"{"request":"forget","setup":"delete"}"#).unwrap(),
            Request::Forget { setup: Setup::Delete }
        );
        let err = serde_json::from_str::<Request>(r#"{"request":"forget"}"#).unwrap_err();
        assert!(err.to_string().contains("setup"), "a forget must say keep or delete: {err}");

        for (line, request) in [
            (r#"{"request":"saved_setup"}"#, Request::SavedSetup),
            (r#"{"request":"copy_key"}"#, Request::CopyKey),
        ] {
            assert_eq!(serde_json::from_str::<Request>(line).unwrap(), request);
        }
    }

    /// A request printed for debugging carries no key.
    #[test]
    fn a_pair_request_never_shows_its_key() {
        let request = Request::Pair {
            key: KeyText::new(FAKE_KEY),
            name: "Band".to_string(),
            setup: Setup::Keep,
        };
        let shown = format!("{request:?}");
        assert!(!shown.contains(&FAKE_KEY[..16]), "{shown}");
        assert!(shown.contains("KeyText(redacted)"), "{shown}");
    }

    /// serde quotes the value it could not take. A key in the wrong place
    /// must not come back in the reply.
    #[test]
    fn a_request_that_cannot_be_read_never_echoes_a_key() {
        for line in [
            format!(r#"{{"request":"forget","setup":"{FAKE_KEY}"}}"#),
            format!(r#"{{"request":"{FAKE_KEY}"}}"#),
            format!(r#"{{"request":"pair","key":["{FAKE_KEY}"],"name":"Band"}}"#),
            r#"{"request":"pair","key":12345678901234567890123456789012,"name":"Band"}"#.to_string(),
        ] {
            let err = serde_json::from_str::<Request>(&line).unwrap_err();
            let reason = unreadable(&err);
            assert!(reason.starts_with("bad request: "), "{reason}");
            assert!(longest_hex_run(&reason) < 16, "the reply quoted the key: {reason}");
        }
    }

    /// Pairing, forgetting, what a forget kept, and the key are the window's
    /// alone. The socket refuses each by name, with no key in the reason, and
    /// hands none of them to the holder. What the band app editor sends still
    /// reaches the holder, because the editor installs through the socket.
    ///
    /// On Windows the pipe's own test checks the same over a real pipe.
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn the_socket_refuses_what_only_the_window_may_ask() {
        use tokio::net::UnixStream;
        let (holder, mut asked) = mpsc::channel::<Incoming>(8);
        let (caller, server) = UnixStream::pair().expect("a connected pair of sockets");
        let serving = tokio::spawn(handle(server, holder));
        let (read, mut write) = caller.into_split();
        let mut replies = BufReader::new(read).lines();
        // A request that got through would wait on a holder that never
        // answers, so a reply that does not come is a failure, not a hang.
        let limit = std::time::Duration::from_secs(5);

        for (name, line) in [
            ("pair", format!(r#"{{"request":"pair","key":"{FAKE_KEY}","name":"Band"}}"#)),
            ("forget", r#"{"request":"forget","setup":"delete"}"#.to_string()),
            ("saved_setup", r#"{"request":"saved_setup"}"#.to_string()),
            ("copy_key", r#"{"request":"copy_key"}"#.to_string()),
        ] {
            write.write_all(format!("{line}\n").as_bytes()).await.unwrap();
            let reply = tokio::time::timeout(limit, replies.next_line())
                .await
                .unwrap_or_else(|_| panic!("the socket handed {name} to the holder"))
                .unwrap()
                .expect("a reply");
            match serde_json::from_str::<Reply>(&reply).unwrap() {
                Reply::Failed { reason } => {
                    assert_eq!(reason, window_only(name));
                    assert!(
                        reason
                            .starts_with(&format!("{name} is for the Mi Band Bridge window only")),
                        "{reason}"
                    );
                    assert!(longest_hex_run(&reason) < 16, "the reply quoted the key: {reason}");
                }
                other => panic!("the socket took {name}: {other:?}"),
            }
        }
        assert!(asked.try_recv().is_err(), "a request only the window may make reached the holder");

        let install = Request::Install {
            file: "/tmp/band-app.bin".to_string(),
        };
        let line = serde_json::to_string(&install).unwrap();
        write.write_all(format!("{line}\n").as_bytes()).await.unwrap();
        let incoming = tokio::time::timeout(limit, asked.recv())
            .await
            .expect("the socket kept the install from the holder")
            .expect("the install reached the holder");
        assert_eq!(incoming.request, install);
        incoming.reply.send(Reply::Done).await.unwrap();
        let reply = tokio::time::timeout(limit, replies.next_line())
            .await
            .expect("the holder's answer came back")
            .unwrap()
            .expect("a reply");
        assert_eq!(serde_json::from_str::<Reply>(&reply).unwrap(), Reply::Done);

        drop(write);
        serving.await.unwrap().expect("the caller went away cleanly");
    }

    /// Only those four are the window's alone.
    #[test]
    fn every_other_request_is_open_to_the_socket() {
        for request in [
            Request::Status,
            Request::Connect,
            Request::Disconnect,
            Request::Follow,
            Request::Device,
            Request::AppList { all: false },
            Request::ScreenList {
                list: "menu".to_string(),
            },
            Request::Install {
                file: "band-app.bin".to_string(),
            },
        ] {
            assert_eq!(request.window_only(), None, "{request:?}");
        }
    }

    /// The longest run of hex digits in a text. Sixteen is half a key.
    pub(crate) fn longest_hex_run(text: &str) -> usize {
        let (mut run, mut longest) = (0, 0);
        for c in text.chars() {
            run = if c.is_ascii_hexdigit() { run + 1 } else { 0 };
            longest = longest.max(run);
        }
        longest
    }

    #[test]
    fn replies_round_trip_as_json() {
        for case in [
            Reply::Line {
                text: "ready".to_string(),
            },
            Reply::Done,
            Reply::Failed {
                reason: "band not found".to_string(),
            },
        ] {
            let text = serde_json::to_string(&case).expect("encodes");
            assert_eq!(serde_json::from_str::<Reply>(&text).expect("decodes"), case);
        }
    }

    /// The tag keeps the wire readable, so a person can watch the socket with
    /// nc and understand it.
    #[test]
    fn the_wire_is_readable() {
        let text = serde_json::to_string(&Request::Status).expect("encodes");
        assert_eq!(text, r#"{"request":"status"}"#);
        let text = serde_json::to_string(&Reply::Line { text: "hi".into() }).expect("encodes");
        assert_eq!(text, r#"{"reply":"line","text":"hi"}"#);
    }
}
