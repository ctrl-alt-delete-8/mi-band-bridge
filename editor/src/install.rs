//! Talk to BandBridge: ask where its band stands, and install a built app.
//!
//! BandBridge holds the band and listens on its link: a Unix socket on the
//! Mac, and on Windows a named pipe for the user who runs it (W6). Both take
//! one JSON request per line, and answer with JSON lines until `done` or
//! `failed`. `status` is the request BandBridge's own window makes, and
//! `install` is the one `bandbridge install` sends. An install answers as it
//! goes, with a line such as `62%` each time the upload moves, and the editor
//! hands each line on as it arrives.
//!
//! The editor does not use the `bandbridge` crate, which would bring the whole
//! band stack into the editor. So it finds the link by BandBridge's own rule,
//! written again here, and `tests/repo_bandbridge.rs` checks that the two
//! rules agree.

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Where BandBridge stands, as the title bar says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BridgeStatus {
    /// BandBridge answers, and its band is connected.
    Connected,
    /// BandBridge answers, and its band is not connected: it connects, it
    /// tries again, or it waits for a person to ask. An install fails now.
    NotConnected,
    /// Nothing answers on the link.
    NotRunning,
    /// BandBridge took the question and did not answer in time. It answers
    /// one request at a time, and an install holds it for about a minute, so
    /// this says nothing new about the band. The window keeps what it showed.
    Busy,
}

/// How long [`status`] waits for an answer. BandBridge answers at once,
/// unless it is busy with another request.
const STATUS_WAIT: Duration = Duration::from_secs(2);

/// How long an install waits for each line. An install takes about a minute,
/// so the editor gives up only after ten.
const INSTALL_WAIT: Duration = Duration::from_secs(600);

/// Where BandBridge listens: `bandbridge.sock` in its folder, which is
/// `BandBridge` inside the system's config folder. This is the rule
/// `bandbridge::config::folder` follows.
#[cfg(unix)]
pub fn socket_path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("BandBridge").join("bandbridge.sock"))
}

/// Where BandBridge listens on Windows: the pipe named by the SID of the user
/// this process runs as, `\\.\pipe\bandbridge-S-1-5-21-…` (W6). This is the
/// rule `bandbridge::platform::link::pipe_name` follows. A BandBridge another
/// user runs has a pipe of its own, which this user cannot open.
#[cfg(windows)]
pub fn pipe_path() -> Option<PathBuf> {
    Some(PathBuf::from(format!(
        r"\\.\pipe\bandbridge-{}",
        link::current_user_sid().ok()?
    )))
}

/// Where BandBridge listens on this system: its socket, or its pipe.
pub fn link_path() -> Option<PathBuf> {
    #[cfg(unix)]
    return socket_path();
    #[cfg(windows)]
    return pipe_path();
    #[cfg(not(any(unix, windows)))]
    return None;
}

/// Why an install cannot find BandBridge's link at all.
#[cfg(unix)]
const NO_LINK: &str = "There is no home folder, so Mi Band Bridge's socket cannot be found.";
#[cfg(not(unix))]
const NO_LINK: &str = "The editor cannot find Mi Band Bridge's pipe.";

/// Ask BandBridge where its band stands.
pub fn status() -> BridgeStatus {
    match link_path() {
        Some(link) => status_via(&link, STATUS_WAIT),
        None => BridgeStatus::NotRunning,
    }
}

/// Ask the BandBridge on `link` where its band stands, and wait at most
/// `wait` for each part of the answer.
///
/// BandBridge answers `status` with a line for a person, then where the
/// connection stands as data, then `done`. Only the data counts: the band is
/// connected when its `state` is `connected`, and not connected in every
/// other state, or when BandBridge answers without it.
pub fn status_via(link: &Path, wait: Duration) -> BridgeStatus {
    let mut talk = match link::open(link) {
        link::Opened::Open(talk) => talk,
        link::Opened::NotRunning => return BridgeStatus::NotRunning,
        link::Opened::Busy => return BridgeStatus::Busy,
    };
    if talk.send("{\"request\":\"status\"}\n", wait).is_err() {
        return BridgeStatus::NotRunning;
    }

    let mut band = BridgeStatus::NotConnected;
    loop {
        let line = match talk.next_line(wait) {
            link::Read::Line(line) => line,
            link::Read::TimedOut(_) => return BridgeStatus::Busy,
            // BandBridge closed the connection before it finished: it is
            // going away.
            link::Read::Closed | link::Read::Failed(_) => return BridgeStatus::NotRunning,
        };
        let Ok(reply) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        match reply["reply"].as_str() {
            Some("data") => {
                // The standing travels as a JSON string inside the reply.
                let standing = reply["json"]
                    .as_str()
                    .and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok());
                if standing.is_some_and(|s| s["state"] == "connected") {
                    band = BridgeStatus::Connected;
                }
            }
            // A failed answer means BandBridge runs but has no band task to
            // ask, so the band is not connected either.
            Some("done") | Some("failed") => return band,
            _ => {}
        }
    }
}

/// What BandBridge says while it installs an app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Heard {
    /// A line for a person, as BandBridge wrote it.
    Line(String),
    /// How much of the upload has gone, in whole percent. BandBridge says it
    /// in a line of its own, such as `62%`, each time it moves.
    Progress(u8),
}

/// Why an install ended before BandBridge said it was done.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stop {
    /// Nothing answers on the link.
    NotRunning,
    /// BandBridge answered that its band is not connected, in these words.
    NotConnected(String),
    /// BandBridge refused the install, or the band did, in BandBridge's words.
    Refused(String),
    /// The install could not be sent, or BandBridge went away during it. The
    /// words are the editor's.
    Lost(String),
}

/// A line BandBridge sent: its progress when it is only a percentage, and
/// otherwise the line as it came.
fn heard(text: String) -> Heard {
    let number = text.trim().strip_suffix('%').unwrap_or_default();
    let digits =
        !number.is_empty() && number.len() <= 3 && number.bytes().all(|b| b.is_ascii_digit());
    match number.parse::<u8>() {
        Ok(percent) if digits && percent <= 100 => Heard::Progress(percent),
        _ => Heard::Line(text),
    }
}

/// A failed answer, told apart by what the window does next. BandBridge
/// fails an install with "the band is not connected" when it holds no link
/// to the band: see `install_here` in `src/supervisor.rs`.
fn refused(reason: String) -> Stop {
    if reason.contains("not connected") {
        Stop::NotConnected(reason)
    } else {
        Stop::Refused(reason)
    }
}

/// Install a built app through BandBridge, and hand on what it says as it
/// says it.
pub fn install(file: &Path, each: impl FnMut(Heard)) -> Result<(), Stop> {
    let link = link_path().ok_or_else(|| Stop::Lost(NO_LINK.to_string()))?;
    install_via(&link, file, each)
}

/// Install a built app through the BandBridge on `link`. Each line BandBridge
/// sends reaches `each` as it arrives, so the window can show an install as
/// it goes, which takes about a minute.
pub fn install_via(link: &Path, file: &Path, mut each: impl FnMut(Heard)) -> Result<(), Stop> {
    // BandBridge resolves a path against its own working folder.
    let file =
        absolute(file).map_err(|e| Stop::Lost(format!("Cannot find {}: {e}", file.display())))?;
    let mut talk = match link::open(link) {
        link::Opened::Open(talk) => talk,
        link::Opened::NotRunning => return Err(Stop::NotRunning),
        link::Opened::Busy => {
            return Err(Stop::Lost(
                "Mi Band Bridge is busy. Try again.".to_string(),
            ));
        }
    };

    let request = serde_json::json!({ "request": "install", "file": file.display().to_string() });
    talk.send(&format!("{request}\n"), INSTALL_WAIT)
        .map_err(|e| Stop::Lost(format!("Cannot reach Mi Band Bridge: {e}")))?;

    loop {
        let line = match talk.next_line(INSTALL_WAIT) {
            link::Read::Line(line) => line,
            link::Read::TimedOut(e) | link::Read::Failed(e) => {
                return Err(Stop::Lost(format!(
                    "Mi Band Bridge stopped answering during the install: {e}"
                )));
            }
            link::Read::Closed => {
                return Err(Stop::Lost(
                    "Mi Band Bridge closed the connection before the install finished.".to_string(),
                ));
            }
        };
        let reply: serde_json::Value = serde_json::from_str(&line).map_err(|e| {
            Stop::Lost(format!(
                "Mi Band Bridge sent a reply this editor cannot read: {e}"
            ))
        })?;
        match reply["reply"].as_str() {
            Some("line") => each(heard(
                reply["text"].as_str().unwrap_or_default().to_string(),
            )),
            Some("data") => each(Heard::Line(
                reply["json"].as_str().unwrap_or_default().to_string(),
            )),
            Some("done") => return Ok(()),
            Some("failed") => {
                return Err(refused(
                    reply["reason"]
                        .as_str()
                        .unwrap_or("The install failed.")
                        .to_string(),
                ));
            }
            _ => {}
        }
    }
}

/// The file's full path, as BandBridge reads it. On Windows the system's own
/// answer starts with `\\?\`, which BandBridge reads but a person does not
/// write, so a path on a drive loses it.
fn absolute(file: &Path) -> std::io::Result<PathBuf> {
    let full = std::fs::canonicalize(file)?;
    #[cfg(windows)]
    {
        let text = full.to_string_lossy();
        if let Some(plain) = text.strip_prefix(r"\\?\") {
            if plain.as_bytes().get(1) == Some(&b':') {
                return Ok(PathBuf::from(plain));
            }
        }
    }
    Ok(full)
}

/// One conversation with BandBridge on its link: a request goes out, then
/// its answer comes back a line at a time, each within a wait. The Mac's
/// socket and Windows' pipe differ only here.
mod link {
    use std::io;

    /// What opening the link found.
    pub(super) enum Opened {
        Open(Talk),
        /// Nothing listens there.
        NotRunning,
        /// Something listens, and takes no new caller now. Only a pipe says
        /// so: a socket takes every caller.
        #[cfg_attr(not(windows), allow(dead_code))]
        Busy,
    }

    /// One read from the link.
    pub(super) enum Read {
        /// A line, without its end.
        Line(String),
        /// No line came within the wait.
        TimedOut(io::Error),
        /// BandBridge closed its end.
        Closed,
        /// The read failed another way.
        Failed(io::Error),
    }

    pub(super) use platform::{Talk, open};

    #[cfg(windows)]
    pub(super) use platform::current_user_sid;

    #[cfg(all(test, windows))]
    pub(super) use platform::win32_code;

    #[cfg(unix)]
    mod platform {
        use super::{Opened, Read};
        use std::io::{self, BufRead, BufReader, ErrorKind, Write};
        use std::os::unix::net::UnixStream;
        use std::path::Path;
        use std::time::Duration;

        pub(in super::super) struct Talk(BufReader<UnixStream>);

        /// A socket file that nothing listens on is left by a BandBridge that
        /// quit without removing it. Connecting to it is refused, and reads
        /// as nothing listening.
        pub(in super::super) fn open(place: &Path) -> Opened {
            match UnixStream::connect(place) {
                Ok(stream) => Opened::Open(Talk(BufReader::new(stream))),
                Err(_) => Opened::NotRunning,
            }
        }

        impl Talk {
            pub(in super::super) fn send(&mut self, line: &str, wait: Duration) -> io::Result<()> {
                let stream = self.0.get_mut();
                let _ = stream.set_write_timeout(Some(wait));
                stream.write_all(line.as_bytes())
            }

            pub(in super::super) fn next_line(&mut self, wait: Duration) -> Read {
                let _ = self.0.get_ref().set_read_timeout(Some(wait));
                let mut line = String::new();
                match self.0.read_line(&mut line) {
                    Ok(0) => Read::Closed,
                    Ok(_) => {
                        let end = line.trim_end_matches(['\n', '\r']).len();
                        line.truncate(end);
                        Read::Line(line)
                    }
                    Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                        Read::TimedOut(e)
                    }
                    Err(e) => Read::Failed(e),
                }
            }
        }
    }

    #[cfg(windows)]
    mod platform {
        use super::{Opened, Read};
        use std::fs::{File, OpenOptions};
        use std::io::{self, ErrorKind, Read as _, Write};
        use std::os::windows::io::AsRawHandle;
        use std::path::Path;
        use std::time::{Duration, Instant};
        use windows::Win32::Foundation::{
            CloseHandle, ERROR_BROKEN_PIPE, ERROR_NO_DATA, ERROR_PIPE_BUSY,
            ERROR_PIPE_NOT_CONNECTED, HANDLE, HLOCAL, LocalFree,
        };
        use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
        use windows::Win32::Security::{GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser};
        use windows::Win32::System::Pipes::PeekNamedPipe;
        use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
        use windows::core::PWSTR;

        /// How many times, and how far apart, the editor tries again while
        /// every instance of the pipe is busy. BandBridge makes a new one as
        /// each caller connects, so this lasts a moment.
        const BUSY_TRIES: u32 = 50;
        const BUSY_PAUSE: Duration = Duration::from_millis(20);

        /// How often a read looks for an answer on the pipe.
        const LOOK_EVERY: Duration = Duration::from_millis(10);

        /// `SECURITY_IDENTIFICATION`: the program at the other end of the
        /// pipe may learn who the editor runs as, and may not act as that
        /// user. A pipe of that name that another account made first then
        /// cannot borrow the editor's rights. tokio's pipe client, which
        /// BandBridge's command line uses, asks the same.
        const IDENTIFY_ONLY: u32 = 0x0001_0000;

        /// The pipe, and what came on it past the last whole line. A pipe
        /// opened as a file cannot time out a read, so a read waits only for
        /// bytes the pipe already holds, and looks again until its wait ends.
        pub(in super::super) struct Talk {
            pipe: File,
            pending: Vec<u8>,
        }

        pub(in super::super) fn open(place: &Path) -> Opened {
            use std::os::windows::fs::OpenOptionsExt;
            for _ in 0..BUSY_TRIES {
                let opened = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .security_qos_flags(IDENTIFY_ONLY)
                    .open(place);
                match opened {
                    Ok(pipe) => {
                        return Opened::Open(Talk {
                            pipe,
                            pending: Vec::new(),
                        });
                    }
                    Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY.0 as i32) => {
                        std::thread::sleep(BUSY_PAUSE);
                    }
                    // No pipe of that name: nothing listens.
                    Err(_) => return Opened::NotRunning,
                }
            }
            Opened::Busy
        }

        impl Talk {
            /// A request is one short line, which the pipe takes at once.
            pub(in super::super) fn send(&mut self, line: &str, _wait: Duration) -> io::Result<()> {
                self.pipe.write_all(line.as_bytes())
            }

            pub(in super::super) fn next_line(&mut self, wait: Duration) -> Read {
                let until = Instant::now() + wait;
                loop {
                    if let Some(end) = self.pending.iter().position(|&b| b == b'\n') {
                        let line: Vec<u8> = self.pending.drain(..=end).collect();
                        return text(line);
                    }
                    match holds(&self.pipe) {
                        Ok(0) => {
                            if Instant::now() >= until {
                                return Read::TimedOut(io::Error::new(
                                    ErrorKind::TimedOut,
                                    format!("no answer in {} s", wait.as_secs()),
                                ));
                            }
                            std::thread::sleep(LOOK_EVERY);
                        }
                        Ok(count) => {
                            let mut chunk = vec![0u8; count.min(64 * 1024)];
                            match self.pipe.read(&mut chunk) {
                                Ok(0) => return self.closed(),
                                Ok(got) => self.pending.extend_from_slice(&chunk[..got]),
                                Err(e) if broken(&e) => return self.closed(),
                                Err(e) => return Read::Failed(e),
                            }
                        }
                        Err(e) if broken(&e) => return self.closed(),
                        Err(e) => return Read::Failed(e),
                    }
                }
            }

            /// BandBridge closed its end. A last line with no end still
            /// counts, as it does on the socket.
            fn closed(&mut self) -> Read {
                if self.pending.is_empty() {
                    Read::Closed
                } else {
                    text(std::mem::take(&mut self.pending))
                }
            }
        }

        /// A line as text, without its end.
        fn text(mut line: Vec<u8>) -> Read {
            while line.last().is_some_and(|b| matches!(b, b'\n' | b'\r')) {
                line.pop();
            }
            match String::from_utf8(line) {
                Ok(line) => Read::Line(line),
                Err(e) => Read::Failed(io::Error::new(ErrorKind::InvalidData, e)),
            }
        }

        /// How many bytes the pipe holds, without taking them.
        fn holds(pipe: &File) -> io::Result<usize> {
            let mut count = 0u32;
            // SAFETY: the pipe's own handle, and a count that lives for the
            // call. No bytes are copied.
            unsafe {
                PeekNamedPipe(
                    HANDLE(pipe.as_raw_handle()),
                    None,
                    0,
                    None,
                    Some(&raw mut count),
                    None,
                )
            }
            .map_err(|e| io::Error::from_raw_os_error(win32_code(e.code().0)))?;
            Ok(count as usize)
        }

        /// The Windows error number inside an HRESULT, as a read on the file
        /// reports it: `0x8007006D` is error 109, the broken pipe. Any other
        /// HRESULT stays as it is.
        pub(in super::super) fn win32_code(hresult: i32) -> i32 {
            let bits = hresult as u32;
            if bits & 0xFFFF_0000 == 0x8007_0000 {
                (bits & 0xFFFF) as i32
            } else {
                hresult
            }
        }

        /// Whether an error means the other end closed the pipe.
        fn broken(e: &io::Error) -> bool {
            [ERROR_BROKEN_PIPE, ERROR_PIPE_NOT_CONNECTED, ERROR_NO_DATA]
                .iter()
                .any(|code| e.raw_os_error() == Some(code.0 as i32))
        }

        /// The SID of the user this process runs as, as text: `S-1-5-21-…`,
        /// read from the process's token, as BandBridge reads it.
        pub(in super::super) fn current_user_sid() -> io::Result<String> {
            /// The process's token, closed when done with.
            struct Token(HANDLE);
            impl Drop for Token {
                fn drop(&mut self) {
                    // SAFETY: the token this process opened, closed once.
                    let _ = unsafe { CloseHandle(self.0) };
                }
            }

            let mut handle = HANDLE::default();
            // SAFETY: this process's own token, opened to read.
            unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut handle) }?;
            let token = Token(handle);
            let mut size = 0u32;
            // The first call only says how much room the answer needs, and
            // fails for want of it.
            // SAFETY: no buffer, so nothing is written but the size.
            let _ = unsafe { GetTokenInformation(token.0, TokenUser, None, 0, &mut size) };
            // In u64s, not bytes: the answer holds a pointer, so it must be
            // aligned for one.
            let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
            // SAFETY: a buffer of the size the first call asked for.
            unsafe {
                GetTokenInformation(
                    token.0,
                    TokenUser,
                    Some(buffer.as_mut_ptr().cast()),
                    size,
                    &mut size,
                )
            }?;
            // SAFETY: the buffer now holds a TOKEN_USER, aligned for it.
            let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
            let mut text = PWSTR::null();
            // SAFETY: the SID inside the buffer, which lives for the call.
            unsafe { ConvertSidToStringSidW(user.User.Sid, &mut text) }?;
            // SAFETY: the string the call made, read once, then freed.
            let sid = unsafe { text.to_string() };
            unsafe { LocalFree(Some(HLOCAL(text.0.cast()))) };
            sid.map_err(io::Error::other)
        }
    }

    /// A caller never sees this: it names a system with no link.
    #[cfg(not(any(unix, windows)))]
    mod platform {
        use super::{Opened, Read};
        use std::io;
        use std::path::Path;
        use std::time::Duration;

        pub(in super::super) struct Talk;

        pub(in super::super) fn open(_place: &Path) -> Opened {
            Opened::NotRunning
        }

        impl Talk {
            pub(in super::super) fn send(&mut self, _line: &str, _wait: Duration) -> io::Result<()> {
                Ok(())
            }

            pub(in super::super) fn next_line(&mut self, _wait: Duration) -> Read {
                Read::Closed
            }
        }
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    /// The socket BandBridge listens on, which is not beside the editor's own
    /// files but in BandBridge's folder, and short enough to be a socket.
    #[test]
    fn the_socket_is_where_bandbridge_listens() {
        let path = super::socket_path().expect("a home folder");
        let home = std::path::PathBuf::from(std::env::var_os("HOME").unwrap());
        assert_eq!(path, home.join("Library/Application Support/BandBridge/bandbridge.sock"));
        assert!(path.as_os_str().len() < 104, "{}", path.display());
        assert_eq!(super::link_path(), Some(path));
    }
}

#[cfg(all(test, windows))]
mod tests {
    /// Whether a text is a SID: `S-1-` and numbers.
    fn is_sid(text: &str) -> bool {
        text.strip_prefix("S-1-").is_some_and(|rest| {
            rest.split('-')
                .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
        })
    }

    /// The pipe is named by the SID of the user who runs the editor, which is
    /// the one Windows itself gives for the user, as BandBridge names it.
    #[test]
    fn the_pipe_is_where_bandbridge_listens() {
        let whoami = std::process::Command::new("whoami")
            .args(["/user", "/fo", "csv", "/nh"])
            .output()
            .expect("whoami runs");
        let line = String::from_utf8_lossy(&whoami.stdout);
        let sid = line
            .trim()
            .rsplit(',')
            .next()
            .unwrap()
            .trim_matches('"')
            .to_string();
        assert!(is_sid(&sid), "{line}");
        let pipe = super::pipe_path().expect("a SID");
        assert_eq!(pipe, std::path::PathBuf::from(format!(r"\\.\pipe\bandbridge-{sid}")));
        assert_eq!(super::link_path(), Some(pipe));
    }

    /// A closed pipe reads as closed however Windows reports it: a read on
    /// the file says error 109, and the pipe's own look says the same number
    /// inside an HRESULT.
    #[test]
    fn a_broken_pipe_is_the_same_error_both_ways() {
        let from_the_look = super::link::win32_code(0x8007_006D_u32 as i32);
        assert_eq!(from_the_look, 109);
        assert_eq!(super::link::win32_code(0x8000_4005_u32 as i32), 0x8000_4005_u32 as i32);
    }

    /// A drive path loses the `\\?\` the system puts in front of it, so
    /// BandBridge gets the path a person would write.
    #[test]
    fn a_path_on_a_drive_goes_as_a_person_writes_it() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("slides.bin");
        std::fs::write(&file, b"bin").unwrap();
        let sent = super::absolute(&file).unwrap();
        assert!(sent.is_absolute(), "{}", sent.display());
        assert!(!sent.to_string_lossy().starts_with(r"\\?\"), "{}", sent.display());
        assert_eq!(std::fs::read(&sent).unwrap(), b"bin");
    }
}
