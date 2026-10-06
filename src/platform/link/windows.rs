//! The link on Windows: a named pipe for the user who runs BandBridge (W6).
//!
//! The pipe is `\\.\pipe\bandbridge-` and the user's SID, and it carries the
//! same lines of JSON the Mac's socket does. Its security lets in that user
//! and nobody else, so its name and its security name one account. It takes
//! no caller from another computer.
//!
//! Windows removes a pipe with the last handle to it, so a run that ends,
//! even one that crashes, leaves no file behind to clean up, as a socket
//! does.

use crate::error::BandError;
use std::io;
use std::time::Duration;
use tokio::net::windows::named_pipe::{
    ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions,
};
use windows::Win32::Foundation::{CloseHandle, ERROR_PIPE_BUSY, HANDLE, HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{
    GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
    TokenUser,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::core::{PCWSTR, PWSTR};

/// The word a message uses for the link.
pub const CALLED: &str = "pipe";

/// How many times, and how far apart, a run looks again at a pipe name that
/// is taken. A caller of a run that ended holds the old pipe until its next
/// read finds it broken, which is at once.
const TAKEN_TRIES: u32 = 20;
const TAKEN_PAUSE: Duration = Duration::from_millis(100);

/// How many times, and how far apart, a caller tries again while every
/// instance of the pipe is busy. The holder makes a new one as each caller
/// connects, so this lasts a moment.
const BUSY_TRIES: u32 = 50;
const BUSY_PAUSE: Duration = Duration::from_millis(20);

/// The pipe's name, from the SID of the user this process runs as:
/// `\\.\pipe\bandbridge-S-1-5-21-…` (W6). Each account on the PC has a pipe
/// of its own, however it is named, and the editor finds the pipe by the
/// same rule.
pub fn pipe_name() -> io::Result<String> {
    Ok(format!(r"\\.\pipe\bandbridge-{}", current_user_sid()?))
}

/// The pipe the holder listens on.
pub struct Listener {
    name: String,
    security: Security,
    /// The instance the next caller connects to. There is always one, so a
    /// caller never finds the pipe missing while BandBridge runs.
    next: NamedPipeServer,
}

/// Listen on the pipe, for the process that holds the band.
///
/// Only one process may: a second BandBridge finds the name taken, and gets
/// the error (Review Focus 3). A pipe a run left behind is gone once the last
/// caller holding it lets go, so the name taken is waited on for a moment
/// before it counts as another BandBridge (Review Focus 4).
pub async fn listen() -> Result<Listener, BandError> {
    let name =
        pipe_name().map_err(|err| BandError::Config(format!("cannot name the pipe: {err}")))?;
    listen_at(&name).await
}

/// [`listen`], on a pipe of this name.
async fn listen_at(name: &str) -> Result<Listener, BandError> {
    let cannot = |err: io::Error| BandError::Config(format!("cannot listen on {name}: {err}"));
    let security = Security::this_user_only().map_err(cannot)?;
    let mut tries = 0;
    loop {
        match create(name, &security, true) {
            Ok(first) => {
                return Ok(Listener {
                    name: name.to_string(),
                    security,
                    next: first,
                });
            }
            Err(err) if err.kind() == io::ErrorKind::PermissionDenied && tries < TAKEN_TRIES => {
                tries += 1;
                tokio::time::sleep(TAKEN_PAUSE).await;
            }
            Err(err) => return Err(cannot(err)),
        }
    }
}

impl Listener {
    /// The next caller.
    ///
    /// A new instance waits for the caller after this one before this one is
    /// handed over, so the pipe is never without one.
    pub async fn accept(&mut self) -> io::Result<NamedPipeServer> {
        let connected = self.next.connect().await;
        let fresh = match create(&self.name, &self.security, false) {
            Ok(fresh) => fresh,
            Err(err) => {
                // The caller stays on the instance it has, and is handed over
                // on the next try. A pause first, so a failure that lasts
                // does not spin the loop that asks.
                tokio::time::sleep(TAKEN_PAUSE).await;
                return Err(err);
            }
        };
        let caller = std::mem::replace(&mut self.next, fresh);
        connected.map(|()| caller)
    }
}

/// One instance of the pipe. The first is made only when no pipe of this
/// name exists, so no other process can have made the pipe first and taken
/// the callers meant for BandBridge.
fn create(name: &str, security: &Security, first: bool) -> io::Result<NamedPipeServer> {
    let mut attributes = security.attributes();
    let mut options = ServerOptions::new();
    options
        .first_pipe_instance(first)
        .reject_remote_clients(true);
    // SAFETY: the attributes point at a security descriptor that `security`
    // owns, and it outlives this call, which copies it.
    unsafe {
        options.create_with_security_attributes_raw(
            name,
            (&mut attributes as *mut SECURITY_ATTRIBUTES).cast(),
        )
    }
}

/// The holder's pipe, or None when no BandBridge listens on it.
pub async fn connect() -> Option<NamedPipeClient> {
    connect_to(&pipe_name().ok()?).await
}

/// [`connect`], to a pipe of this name.
async fn connect_to(name: &str) -> Option<NamedPipeClient> {
    for _ in 0..BUSY_TRIES {
        match ClientOptions::new().open(name) {
            Ok(client) => return Some(client),
            Err(err) if err.raw_os_error() == Some(ERROR_PIPE_BUSY.0 as i32) => {
                tokio::time::sleep(BUSY_PAUSE).await;
            }
            Err(_) => return None,
        }
    }
    None
}

/// A security descriptor that lets in the user this process runs as, and
/// nobody else (Review Focus 4).
struct Security(PSECURITY_DESCRIPTOR);

// SAFETY: the descriptor is memory this value alone owns, and nothing else
// points at it, so it may move to another thread with the listener.
unsafe impl Send for Security {}

impl Security {
    fn this_user_only() -> io::Result<Self> {
        // D: the access list, P: protected from any it could inherit, and one
        // entry: allow (A) everything (GA) to this user.
        let sddl = format!("D:P(A;;GA;;;{})", current_user_sid()?);
        let sddl: Vec<u16> = sddl.encode_utf16().chain([0]).collect();
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(sddl.as_ptr()),
                SDDL_REVISION_1,
                &mut descriptor,
                None,
            )
        }?;
        Ok(Self(descriptor))
    }

    fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.0.0,
            bInheritHandle: false.into(),
        }
    }
}

impl Drop for Security {
    fn drop(&mut self) {
        unsafe { LocalFree(Some(HLOCAL(self.0.0))) };
    }
}

/// The SID of the user this process runs as, as text: `S-1-5-21-…`.
fn current_user_sid() -> io::Result<String> {
    /// The process's token, closed when done with.
    struct Token(HANDLE);
    impl Drop for Token {
        fn drop(&mut self) {
            let _ = unsafe { CloseHandle(self.0) };
        }
    }

    let mut handle = HANDLE::default();
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut handle) }?;
    let token = Token(handle);
    let mut size = 0u32;
    // The first call only says how much room the answer needs, and fails for
    // want of it.
    let _ = unsafe { GetTokenInformation(token.0, TokenUser, None, 0, &mut size) };
    // In u64s, not bytes: the answer holds a pointer, so it must be aligned
    // for one.
    let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
    unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            Some(buffer.as_mut_ptr().cast()),
            size,
            &mut size,
        )
    }?;
    let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
    let mut text = PWSTR::null();
    unsafe { ConvertSidToStringSidW(user.User.Sid, &mut text) }?;
    let sid = unsafe { text.to_string() };
    unsafe { LocalFree(Some(HLOCAL(text.0.cast()))) };
    sid.map_err(io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::tests::{FAKE_KEY, longest_hex_run};
    use crate::ipc::{Incoming, Reply, Request, serve_on, window_only};
    use std::time::Duration;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::sync::mpsc;

    /// Whether a text is a SID: `S-1-` and numbers, such as `S-1-5-21-…` for
    /// a local account or `S-1-12-1-…` for an Entra ID one.
    fn is_sid(text: &str) -> bool {
        text.strip_prefix("S-1-").is_some_and(|rest| {
            rest.split('-')
                .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
        })
    }

    #[test]
    fn a_sid_is_s_1_and_numbers() {
        for sid in [
            "S-1-5-21-62437770-625408165-2877764191-1001",
            "S-1-12-1-1234-5678",
            "S-1-5-18",
        ] {
            assert!(is_sid(sid), "{sid}");
        }
        for not_a_sid in ["", "S-1-", "S-1-5-", "S-2-5-21", "S-1-5-x", "jane doe"] {
            assert!(!is_sid(not_a_sid), "{not_a_sid}");
        }
    }

    /// A pipe of its own for each test, so a test never meets a BandBridge
    /// that runs, or another test.
    fn test_pipe() -> String {
        format!(
            r"\\.\pipe\bandbridge-test-{}-{}",
            std::process::id(),
            rand::random::<u32>()
        )
    }

    /// How long a test waits for an answer before it calls the answer lost.
    const LIMIT: Duration = Duration::from_secs(5);

    /// The pipe is named by the SID of the user who runs BandBridge, which
    /// its security lets in, so each account on the PC has a pipe of its
    /// own, and the editor finds it the same way. The SID is the one Windows
    /// itself gives for the user.
    #[test]
    fn the_pipe_is_named_by_the_users_sid() {
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
        assert_eq!(current_user_sid().unwrap(), sid);
        assert_eq!(pipe_name().unwrap(), format!(r"\\.\pipe\bandbridge-{sid}"));
        assert_eq!(CALLED, "pipe");
    }

    /// A request goes over the pipe as one line of JSON, and each answer
    /// comes back the same way, as over the Mac's socket (W6).
    #[tokio::test]
    async fn a_request_and_its_answers_go_over_the_pipe() {
        let name = test_pipe();
        let (holder, mut asked) = mpsc::channel::<Incoming>(8);
        tokio::spawn(serve_on(listen_at(&name).await.unwrap(), holder));
        let answering = tokio::spawn(async move {
            let incoming = asked.recv().await.expect("the request reached the holder");
            assert_eq!(incoming.request, Request::Status);
            let reply = |text: &str| Reply::Line {
                text: text.to_string(),
            };
            incoming.reply.send(reply("ready")).await.unwrap();
            incoming.reply.send(Reply::Done).await.unwrap();
        });

        let pipe = connect_to(&name).await.expect("the pipe answers");
        let (read, mut write) = tokio::io::split(pipe);
        write
            .write_all(b"{\"request\":\"status\"}\n")
            .await
            .unwrap();
        let mut replies = BufReader::new(read).lines();
        for want in [r#"{"reply":"line","text":"ready"}"#, r#"{"reply":"done"}"#] {
            let line = tokio::time::timeout(LIMIT, replies.next_line())
                .await
                .expect("an answer in time")
                .unwrap()
                .expect("a line");
            assert_eq!(line, want);
        }
        answering.await.unwrap();
    }

    /// Pairing, forgetting, what a forget kept, and the key are the window's
    /// alone. The pipe refuses each by name, with no key in the reason, and
    /// hands none of them to the holder, as the socket does. An install still
    /// gets through, because the editor installs through the pipe.
    #[tokio::test]
    async fn the_pipe_refuses_what_only_the_window_may_ask() {
        let name = test_pipe();
        let (holder, mut asked) = mpsc::channel::<Incoming>(8);
        tokio::spawn(serve_on(listen_at(&name).await.unwrap(), holder));
        let pipe = connect_to(&name).await.expect("the pipe answers");
        let (read, mut write) = tokio::io::split(pipe);
        let mut replies = BufReader::new(read).lines();

        for (request, line) in [
            (
                "pair",
                format!(r#"{{"request":"pair","key":"{FAKE_KEY}","name":"Band"}}"#),
            ),
            (
                "forget",
                r#"{"request":"forget","setup":"delete"}"#.to_string(),
            ),
            ("saved_setup", r#"{"request":"saved_setup"}"#.to_string()),
            ("copy_key", r#"{"request":"copy_key"}"#.to_string()),
        ] {
            write
                .write_all(format!("{line}\n").as_bytes())
                .await
                .unwrap();
            let reply = tokio::time::timeout(LIMIT, replies.next_line())
                .await
                .unwrap_or_else(|_| panic!("the pipe handed {request} to the holder"))
                .unwrap()
                .expect("a reply");
            match serde_json::from_str::<Reply>(&reply).unwrap() {
                Reply::Failed { reason } => {
                    assert_eq!(reason, window_only(request));
                    assert!(reason.contains("and the pipe refuses it"), "{reason}");
                    assert!(
                        longest_hex_run(&reason) < 16,
                        "the reply quoted the key: {reason}"
                    );
                }
                other => panic!("the pipe took {request}: {other:?}"),
            }
        }
        assert!(
            asked.try_recv().is_err(),
            "a request only the window may make reached the holder"
        );

        let install = Request::Install {
            file: r"C:\Users\someone\band-app.bin".to_string(),
        };
        let line = serde_json::to_string(&install).unwrap();
        write
            .write_all(format!("{line}\n").as_bytes())
            .await
            .unwrap();
        let incoming = tokio::time::timeout(LIMIT, asked.recv())
            .await
            .expect("the pipe kept the install from the holder")
            .expect("the install reached the holder");
        assert_eq!(incoming.request, install);
    }

    /// Only the user who runs BandBridge may open the pipe: its access list
    /// allows that user, and nobody else (Review Focus 4).
    #[tokio::test]
    async fn only_this_user_may_open_the_pipe() {
        let listener = listen_at(&test_pipe()).await.unwrap();
        let sid = current_user_sid().unwrap();
        assert!(is_sid(&sid), "{sid}");
        let list = access_list(&listener.next);
        assert!(list.starts_with("D:P(A;;"), "not this user alone: {list}");
        assert_eq!(
            list.matches("(A;").count(),
            1,
            "more than one allowed: {list}"
        );
        assert!(
            list.ends_with(&format!(";;;{sid})")),
            "not this user: {list}"
        );
    }

    /// A second BandBridge cannot open a second pipe beside the first
    /// (Review Focus 3). A pipe a run left behind does not stop the next run,
    /// even with a caller still holding it: Windows takes a pipe away with its
    /// last handle, and the next run waits a moment for that caller to let go
    /// (Review Focus 4).
    #[tokio::test]
    async fn one_pipe_at_a_time_and_none_left_behind() {
        let name = test_pipe();
        let first = listen_at(&name).await.expect("the first run listens");
        assert!(
            listen_at(&name).await.is_err(),
            "a second pipe opened beside the first"
        );

        let caller = connect_to(&name)
            .await
            .expect("a caller reaches the first run");
        drop(first);
        // The caller lets go a moment after the run ends, as one does when
        // its next read finds the pipe broken.
        let letting_go = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            drop(caller);
        });
        listen_at(&name)
            .await
            .expect("the next run listens once the last caller lets go");
        letting_go.await.unwrap();
    }

    /// A caller finds nobody when no BandBridge runs.
    #[tokio::test]
    async fn no_pipe_is_no_holder() {
        assert!(connect_to(&test_pipe()).await.is_none());
    }

    /// The pipe's access list, as SDDL text.
    fn access_list(pipe: &NamedPipeServer) -> String {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::Security::Authorization::{
            ConvertSecurityDescriptorToStringSecurityDescriptorW, GetSecurityInfo, SE_KERNEL_OBJECT,
        };
        use windows::Win32::Security::DACL_SECURITY_INFORMATION;

        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        let read = unsafe {
            GetSecurityInfo(
                HANDLE(pipe.as_raw_handle()),
                SE_KERNEL_OBJECT,
                DACL_SECURITY_INFORMATION,
                None,
                None,
                None,
                None,
                Some(&mut descriptor),
            )
        };
        assert!(read.is_ok(), "cannot read the pipe's security: {read:?}");
        let mut text = PWSTR::null();
        unsafe {
            ConvertSecurityDescriptorToStringSecurityDescriptorW(
                descriptor,
                SDDL_REVISION_1,
                DACL_SECURITY_INFORMATION,
                &mut text,
                None,
            )
        }
        .unwrap();
        let list = unsafe { text.to_string() }.unwrap();
        unsafe {
            LocalFree(Some(HLOCAL(text.0.cast())));
            LocalFree(Some(HLOCAL(descriptor.0)));
        }
        list
    }
}
