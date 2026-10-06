//! The link client, against a fake BandBridge on the link it uses on each
//! system: a Unix socket on the Mac, and a named pipe on Windows. The install,
//! and the state the title bar shows. Every case runs on both.

use band_editor::install::{BridgeStatus, Heard, Stop, install_via, status_via};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// A fake BandBridge's end of the link, which takes one caller.
#[cfg(unix)]
mod fake {
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::{Path, PathBuf};

    pub type Stream = UnixStream;
    pub struct Listening(UnixListener);

    /// A place for a link in `dir`. A socket path must stay under about 104
    /// bytes on macOS, which `short_dir` sees to.
    pub fn place(dir: &Path, name: &str) -> PathBuf {
        dir.join(format!("{name}.sock"))
    }

    pub fn listen(place: &Path) -> Listening {
        Listening(UnixListener::bind(place).unwrap())
    }

    impl Listening {
        pub fn accept(self) -> Stream {
            self.0.accept().unwrap().0
        }
    }

    /// The fake's end is done. What it wrote stays readable after it closes.
    pub fn finish(stream: Stream) {
        drop(stream);
    }

    /// A socket file that nothing listens on, as a BandBridge that quit
    /// without removing it leaves behind.
    pub fn stale(place: &Path) {
        drop(UnixListener::bind(place).unwrap());
        assert!(place.exists());
    }
}

/// A fake BandBridge's end of the link, which takes one caller.
#[cfg(windows)]
mod fake {
    use std::fs::File;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU32, Ordering};
    use windows::Win32::Foundation::{ERROR_PIPE_CONNECTED, HANDLE};
    use windows::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
    use windows::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_WAIT,
    };
    use windows::core::PCWSTR;

    pub type Stream = File;
    pub struct Listening(File);

    /// A pipe of its own for each link a test makes, so a test never meets a
    /// BandBridge that runs, or another test.
    pub fn place(_dir: &Path, name: &str) -> PathBuf {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        PathBuf::from(format!(
            r"\\.\pipe\bandbridge-editor-test-{}-{}-{name}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ))
    }

    pub fn listen(place: &Path) -> Listening {
        let name: Vec<u16> = place.as_os_str().encode_wide().chain([0]).collect();
        // SAFETY: a name that lives for the call, and the default security.
        let pipe = unsafe {
            CreateNamedPipeW(
                PCWSTR(name.as_ptr()),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                1,
                4096,
                4096,
                0,
                None,
            )
        };
        assert!(!pipe.is_invalid(), "the test pipe was not made");
        // SAFETY: the pipe just made, owned by the File from here.
        Listening(unsafe { File::from_raw_handle(pipe.0) })
    }

    impl Listening {
        pub fn accept(self) -> Stream {
            // SAFETY: the pipe this fake owns, waited on with no overlap.
            match unsafe { ConnectNamedPipe(HANDLE(self.0.as_raw_handle()), None) } {
                Ok(()) => {}
                // The caller connected before the wait began.
                Err(e) if e.code() == ERROR_PIPE_CONNECTED.to_hresult() => {}
                Err(e) => panic!("no caller came: {e}"),
            }
            self.0
        }
    }

    /// The fake's end is done. It waits until the caller has read what it
    /// wrote, so closing the pipe drops no answer.
    pub fn finish(stream: Stream) {
        let _ = stream.sync_all();
        drop(stream);
    }
}

/// Take one caller, record its request line, send the replies, and close.
fn fake_bandbridge<R: AsRef<str> + Send + 'static>(
    place: PathBuf,
    replies: Vec<R>,
) -> std::thread::JoinHandle<String> {
    let listening = fake::listen(&place);
    std::thread::spawn(move || {
        let stream = listening.accept();
        let mut request = String::new();
        BufReader::new(stream.try_clone().unwrap()).read_line(&mut request).unwrap();
        let mut writer = stream;
        for reply in replies {
            writer.write_all(reply.as_ref().as_bytes()).unwrap();
            writer.write_all(b"\n").unwrap();
        }
        fake::finish(writer);
        request
    })
}

/// A folder of its own for a test. A socket path must stay under about 104
/// bytes on macOS, so use /tmp there.
fn short_dir() -> tempfile::TempDir {
    #[cfg(unix)]
    return tempfile::Builder::new().prefix("be").tempdir_in("/tmp").unwrap();
    #[cfg(not(unix))]
    return tempfile::tempdir().unwrap();
}

/// An app to install, three bytes long, in a short folder of its own.
fn a_built_app() -> (tempfile::TempDir, PathBuf) {
    let dir = short_dir();
    let file = dir.path().join("slides.bin");
    std::fs::write(&file, b"bin").unwrap();
    (dir, file)
}

/// Install `file` through `link`, and keep what BandBridge said on the way.
fn install_hearing(link: &Path, file: &Path) -> (Result<(), Stop>, Vec<Heard>) {
    let mut heard = Vec::new();
    let result = install_via(link, file, |h| heard.push(h));
    (result, heard)
}

fn line(text: &str) -> Heard {
    Heard::Line(text.to_string())
}

/// What BandBridge says as an install goes, as its holder answers: the
/// package's path and size, a line each time the upload's percentage moves,
/// then Installed. The size is that of `a_built_slides`.
const AN_INSTALL: [&str; 7] = [
    r#"{"reply":"line","text":"/tmp/be/slides/dist/slides.bin: 48213 bytes"}"#,
    r#"{"reply":"line","text":"0%"}"#,
    r#"{"reply":"line","text":"1%"}"#,
    r#"{"reply":"line","text":"62%"}"#,
    r#"{"reply":"line","text":"100%"}"#,
    r#"{"reply":"line","text":"Installed."}"#,
    r#"{"reply":"done"}"#,
];

#[test]
fn it_sends_an_absolute_path_and_hands_on_each_line() {
    let (dir, file) = a_built_app();
    let socket = fake::place(dir.path(), "s");
    let server = fake_bandbridge(
        socket.clone(),
        vec![
            r#"{"reply":"line","text":"Installed."}"#,
            r#"{"reply":"done"}"#,
        ],
    );

    let (result, heard) = install_hearing(&socket, &file);
    assert_eq!(result, Ok(()));
    assert_eq!(heard, vec![line("Installed.")]);

    let request: serde_json::Value = serde_json::from_str(&server.join().unwrap()).unwrap();
    assert_eq!(request["request"], "install");
    let sent = PathBuf::from(request["file"].as_str().unwrap());
    assert!(sent.is_absolute());
}

/// The progress lines become the percentage. Every other line is handed on
/// as it came, in order.
#[test]
fn progress_lines_become_the_percentage() {
    let (dir, file) = a_built_app();
    let socket = fake::place(dir.path(), "s");
    let _server = fake_bandbridge(socket.clone(), AN_INSTALL.to_vec());

    let (result, heard) = install_hearing(&socket, &file);
    assert_eq!(result, Ok(()));
    assert_eq!(
        heard,
        vec![
            line("/tmp/be/slides/dist/slides.bin: 48213 bytes"),
            Heard::Progress(0),
            Heard::Progress(1),
            Heard::Progress(62),
            Heard::Progress(100),
            line("Installed."),
        ]
    );
}

/// Each line reaches the window while the install still runs, not when it
/// ends: this BandBridge answers done only once the window has heard its
/// first line, and gives up after five seconds.
#[test]
fn each_line_arrives_while_the_install_runs() {
    let (dir, file) = a_built_app();
    let socket = fake::place(dir.path(), "s");
    let listening = fake::listen(&socket);
    let (heard_tx, heard_rx) = std::sync::mpsc::channel::<()>();
    let server = std::thread::spawn(move || {
        let stream = listening.accept();
        let mut request = String::new();
        BufReader::new(stream.try_clone().unwrap())
            .read_line(&mut request)
            .unwrap();
        let mut writer = stream;
        writer
            .write_all(b"{\"reply\":\"line\",\"text\":\"62%\"}\n")
            .unwrap();
        let last = match heard_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(()) => r#"{"reply":"done"}"#,
            Err(_) => r#"{"reply":"failed","reason":"the window heard nothing before the end"}"#,
        };
        writer.write_all(format!("{last}\n").as_bytes()).unwrap();
        fake::finish(writer);
    });

    let result = install_via(&socket, &file, |_| {
        let _ = heard_tx.send(());
    });
    server.join().unwrap();
    assert_eq!(result, Ok(()));
}

/// BandBridge runs, and its band is not connected. The window says so in
/// its own card, so this stop is told apart from the others. The reason is
/// the real one: see `an_install_that_failed_logs_nothing` in
/// `src/supervisor.rs`.
#[test]
fn a_band_that_is_not_connected_stops_the_install() {
    let (dir, file) = a_built_app();
    let socket = fake::place(dir.path(), "s");
    let _server = fake_bandbridge(
        socket.clone(),
        vec![
            r#"{"reply":"failed","reason":"install failed: the band is not connected. Connect first."}"#,
        ],
    );
    let (result, heard) = install_hearing(&socket, &file);
    assert_eq!(
        result,
        Err(Stop::NotConnected(
            "install failed: the band is not connected. Connect first.".to_string()
        ))
    );
    assert!(heard.is_empty());
}

/// Any other failed answer stops the install with BandBridge's own reason,
/// after the lines it said before it.
#[test]
fn a_refusal_stops_the_install_with_bandbridges_words() {
    let (dir, file) = a_built_app();
    let socket = fake::place(dir.path(), "s");
    let _server = fake_bandbridge(
        socket.clone(),
        vec![
            r#"{"reply":"line","text":"/tmp/be/slides.bin: 3 bytes"}"#,
            r#"{"reply":"line","text":"38%"}"#,
            r#"{"reply":"failed","reason":"install failed: the band stopped replying"}"#,
        ],
    );
    let (result, heard) = install_hearing(&socket, &file);
    assert_eq!(
        result,
        Err(Stop::Refused(
            "install failed: the band stopped replying".to_string()
        ))
    );
    assert_eq!(
        heard,
        vec![line("/tmp/be/slides.bin: 3 bytes"), Heard::Progress(38)]
    );
}

#[test]
fn no_link_says_bandbridge_is_not_running() {
    let (dir, file) = a_built_app();
    let (result, heard) = install_hearing(&fake::place(dir.path(), "none"), &file);
    assert_eq!(result, Err(Stop::NotRunning));
    assert!(heard.is_empty());

    // BandBridge quit without removing its socket: nothing listens on it. A
    // pipe goes with its last handle, so Windows has no such leftover.
    #[cfg(unix)]
    {
        let stale = fake::place(dir.path(), "stale");
        fake::stale(&stale);
        assert_eq!(install_hearing(&stale, &file).0, Err(Stop::NotRunning));
    }
}

/// BandBridge went away in the middle. What it said before stays said.
#[test]
fn a_close_before_done_stops_the_install() {
    let (dir, file) = a_built_app();
    let socket = fake::place(dir.path(), "s");
    let _server = fake_bandbridge(socket.clone(), vec![r#"{"reply":"line","text":"12%"}"#]);
    let (result, heard) = install_hearing(&socket, &file);
    assert_eq!(
        result,
        Err(Stop::Lost(
            "Mi Band Bridge closed the connection before the install finished.".to_string()
        ))
    );
    assert_eq!(heard, vec![Heard::Progress(12)]);
}

/// How long a status test lets the client wait. A fake answers at once.
const WAIT: Duration = Duration::from_secs(2);

/// Where the connection stands, as BandBridge sends it for `status`: its
/// word for a person, the standing as data, then done. The data line is the
/// real one, as a running BandBridge sent it.
fn standing(word: &'static str, state: &'static str) -> Vec<String> {
    let data = format!(
        r#"{{"reply":"data","json":"{{\"state\":\"{state}\",\"reason\":\"you\",\"since\":\"2026-09-29T10:00:00-04:00\"}}"}}"#
    );
    vec![
        format!(r#"{{"reply":"line","text":"state {word}"}}"#),
        data,
        r#"{"reply":"done"}"#.to_string(),
    ]
}

#[test]
fn status_asks_bandbridge_and_reads_a_connected_band() {
    let dir = short_dir();
    let socket = fake::place(dir.path(), "s");
    let server = fake_bandbridge(socket.clone(), standing("ready", "connected"));

    assert_eq!(status_via(&socket, WAIT), BridgeStatus::Connected);

    let request: serde_json::Value = serde_json::from_str(&server.join().unwrap()).unwrap();
    assert_eq!(request, serde_json::json!({ "request": "status" }));
}

/// Connecting, trying again, and letting the band go all leave the band
/// unable to take an install, so the title bar says it is not connected.
#[test]
fn status_reads_every_other_state_as_not_connected() {
    for (word, state) in [
        ("scanning", "connecting"),
        ("backoff (attempt 2)", "retrying"),
        ("disconnected", "disconnected"),
    ] {
        let dir = short_dir();
        let socket = fake::place(dir.path(), "s");
        let _server = fake_bandbridge(socket.clone(), standing(word, state));
        assert_eq!(
            status_via(&socket, WAIT),
            BridgeStatus::NotConnected,
            "{state}"
        );
    }
}

#[test]
fn status_with_nothing_on_the_link_reads_not_running() {
    let dir = short_dir();
    assert_eq!(
        status_via(&fake::place(dir.path(), "none"), WAIT),
        BridgeStatus::NotRunning
    );

    // BandBridge quit without removing its socket: the file is there, and
    // nothing listens on it. A pipe goes with its last handle, so Windows has
    // no such leftover.
    #[cfg(unix)]
    {
        let stale = fake::place(dir.path(), "stale");
        fake::stale(&stale);
        assert_eq!(status_via(&stale, WAIT), BridgeStatus::NotRunning);
    }

    // BandBridge went away before it finished its answer.
    let socket = fake::place(dir.path(), "s");
    let _server = fake_bandbridge(
        socket.clone(),
        vec![r#"{"reply":"line","text":"state ready"}"#],
    );
    assert_eq!(status_via(&socket, WAIT), BridgeStatus::NotRunning);
}

/// BandBridge answers one request at a time, so it cannot answer while it
/// installs an app. The client gives up after its wait, and says so, rather
/// than say BandBridge is gone.
#[test]
fn status_that_gets_no_answer_in_time_reads_busy() {
    let dir = short_dir();
    let socket = fake::place(dir.path(), "s");
    let listening = fake::listen(&socket);
    let (done, finished) = std::sync::mpsc::channel::<()>();
    let server = std::thread::spawn(move || {
        let stream = listening.accept();
        let mut request = String::new();
        BufReader::new(stream.try_clone().unwrap())
            .read_line(&mut request)
            .unwrap();
        // Hold the connection open, and answer nothing, until the test ends.
        let _ = finished.recv();
        drop(stream);
    });

    let asked = std::time::Instant::now();
    assert_eq!(
        status_via(&socket, Duration::from_millis(200)),
        BridgeStatus::Busy
    );
    assert!(
        asked.elapsed() < Duration::from_secs(2),
        "waited {:?}",
        asked.elapsed()
    );
    done.send(()).unwrap();
    server.join().unwrap();
}

/// A failed answer comes from a BandBridge that runs but has no band task
/// to ask, so its band is not connected.
#[test]
fn status_that_fails_reads_not_connected() {
    let dir = short_dir();
    let socket = fake::place(dir.path(), "s");
    let _server = fake_bandbridge(
        socket.clone(),
        vec![r#"{"reply":"failed","reason":"the holder is gone"}"#],
    );
    assert_eq!(status_via(&socket, WAIT), BridgeStatus::NotConnected);
}

// --- The install as the Output drawer tells it -------------------------------

use band_editor::output::{INSTALL_STOPPED, Installed, Said, Tone, install_app_via};

/// An apps folder whose app "slides" was built: its .bin is 48,213 bytes.
fn a_built_slides() -> (tempfile::TempDir, PathBuf) {
    let dir = short_dir();
    let app = dir.path().join("slides");
    std::fs::create_dir_all(app.join("dist")).unwrap();
    std::fs::write(app.join("dist/slides.bin"), vec![0u8; 48_213]).unwrap();
    (dir, app)
}

fn installing(app: &std::path::Path, socket: &std::path::Path) -> (Installed, Vec<Said>) {
    let mut said = Vec::new();
    let ended = install_app_via(app, socket, &mut |s| said.push(s));
    (ended, said)
}

fn texts(said: &[Said]) -> Vec<(Tone, &str, Option<u8>)> {
    said.iter()
        .map(|s| (s.tone, s.text.as_str(), s.percent))
        .collect()
}

/// The drawer tells the install as it goes: the step, the upload on one
/// line that follows its percentage, and the end. BandBridge's line with
/// the package's path and size, and its "Installed.", repeat the step and
/// the end, so the log leaves them out.
#[test]
fn an_install_says_each_step_and_its_progress() {
    let (dir, app) = a_built_slides();
    let socket = fake::place(dir.path(), "s");
    let _server = fake_bandbridge(socket.clone(), AN_INSTALL.to_vec());

    let (ended, said) = installing(&app, &socket);
    assert_eq!(ended, Installed::Installed);
    let (last, lines) = said.split_last().unwrap();
    assert_eq!(
        texts(lines),
        [
            (
                Tone::Step,
                "Install: sending slides.bin to Mi Band Bridge",
                None
            ),
            (
                Tone::Progress,
                "Uploading to the band… 0% · 0.0 of 48.2 KB",
                Some(0)
            ),
            (
                Tone::Progress,
                "Uploading to the band… 1% · 0.5 of 48.2 KB",
                Some(1)
            ),
            (
                Tone::Progress,
                "Uploading to the band… 62% · 29.9 of 48.2 KB",
                Some(62)
            ),
            (
                Tone::Progress,
                "Uploading to the band… 100% · 48.2 of 48.2 KB",
                Some(100)
            ),
        ]
    );
    assert_eq!(last.tone, Tone::Good);
    assert!(
        last.text.starts_with("Install finished in "),
        "{}",
        last.text
    );
}

/// Review Focus 3: with no BandBridge, the log says only that the install
/// stopped, and the window shows the card, which says why.
#[test]
fn an_install_with_no_bandbridge_stops_and_leaves_the_why_to_the_card() {
    let (dir, app) = a_built_slides();
    let (ended, said) = installing(&app, &fake::place(dir.path(), "none"));
    assert_eq!(ended, Installed::NotRunning);
    assert_eq!(
        texts(&said),
        [
            (
                Tone::Step,
                "Install: sending slides.bin to Mi Band Bridge",
                None
            ),
            (Tone::Bad, INSTALL_STOPPED, None),
        ]
    );
    assert_eq!(INSTALL_STOPPED, "Install stopped.");
}

/// Every other line BandBridge says stays in the log, in its own words.
#[test]
fn an_install_keeps_bandbridges_other_lines() {
    let (dir, app) = a_built_slides();
    let socket = fake::place(dir.path(), "s");
    let _server = fake_bandbridge(
        socket.clone(),
        vec![
            AN_INSTALL[0],
            r#"{"reply":"line","text":"The band asked to wait."}"#,
            r#"{"reply":"line","text":"62%"}"#,
            r#"{"reply":"line","text":"Installed."}"#,
            r#"{"reply":"done"}"#,
        ],
    );
    let (ended, said) = installing(&app, &socket);
    assert_eq!(ended, Installed::Installed);
    let (last, lines) = said.split_last().unwrap();
    assert_eq!(
        texts(lines)[1..],
        [
            (Tone::Note, "Mi Band Bridge: The band asked to wait.", None),
            (
                Tone::Progress,
                "Uploading to the band… 62% · 29.9 of 48.2 KB",
                Some(62)
            ),
        ]
    );
    assert!(
        last.text.starts_with("Install finished in "),
        "{}",
        last.text
    );
}

/// Review Focus 3: BandBridge runs, and its band is not connected. The log
/// says only that the install stopped, as it does when BandBridge is not
/// running, and the window shows the card, which says why.
#[test]
fn an_install_with_no_band_stops_and_leaves_the_why_to_the_card() {
    let (dir, app) = a_built_slides();
    let socket = fake::place(dir.path(), "s");
    let _server = fake_bandbridge(
        socket.clone(),
        vec![
            r#"{"reply":"failed","reason":"install failed: the band is not connected. Connect first."}"#,
        ],
    );
    let (ended, said) = installing(&app, &socket);
    assert_eq!(ended, Installed::NotConnected);
    assert_eq!(
        texts(&said),
        [
            (
                Tone::Step,
                "Install: sending slides.bin to Mi Band Bridge",
                None
            ),
            (Tone::Bad, INSTALL_STOPPED, None),
        ]
    );
}

/// Any other stop shows BandBridge's reason, and no card.
#[test]
fn an_install_the_band_refuses_ends_with_bandbridges_reason() {
    let (dir, app) = a_built_slides();
    let socket = fake::place(dir.path(), "s");
    let _server = fake_bandbridge(
        socket.clone(),
        vec![
            r#"{"reply":"line","text":"38%"}"#,
            r#"{"reply":"failed","reason":"install failed: the band stopped replying"}"#,
        ],
    );
    let (ended, said) = installing(&app, &socket);
    assert_eq!(ended, Installed::Stopped);
    assert_eq!(
        texts(&said)[1..],
        [
            (
                Tone::Progress,
                "Uploading to the band… 38% · 18.3 of 48.2 KB",
                Some(38)
            ),
            (
                Tone::Bad,
                "Mi Band Bridge: install failed: the band stopped replying",
                None
            ),
        ]
    );
}
