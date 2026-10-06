//! Running an action's command on Windows.
//!
//! A shell line runs in Windows PowerShell (W3). A script file runs as a
//! double-click runs it, except a .ps1, which PowerShell runs, since a
//! double-click would open it in Notepad (W4). Shortcuts is a Mac app, so
//! Windows runs no shortcut (W5).

use crate::action::{Ended, Outcome, exited, expand_home, first_line};
use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;
use windows::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_TEMPORARY, FILE_FLAG_DELETE_ON_CLOSE, FILE_SHARE_DELETE, FILE_SHARE_READ,
    FILE_SHARE_WRITE,
};
use windows::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
};
use windows::Win32::System::Threading::CREATE_NO_WINDOW;
use windows::Win32::UI::Shell::{
    SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW, ShellExecuteExW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{PCWSTR, w};

/// Why Windows runs no shortcut. The loader refuses a shortcut action with
/// it, and the window does not offer the kind (W5).
pub const SHORTCUTS_REFUSED: Option<&str> = Some(NO_SHORTCUTS);

const NO_SHORTCUTS: &str = "Shortcuts are a Mac feature. Use a Script or Shell action on Windows.";

/// Windows PowerShell, which every Windows 10 and 11 has under this name.
const POWERSHELL: &str = "powershell.exe";

/// Run a command line in Windows PowerShell, and say how it went (W3).
pub async fn shell(line: String) -> Outcome {
    exited(run(powershell(shell_arguments(&line))).await)
}

/// Run a script file, and say how it went (W4).
///
/// A leading `~` is the home folder, as on the Mac: neither PowerShell nor a
/// double-click expands it.
pub async fn script(path: String) -> Outcome {
    let file = expand_home(&path, std::env::var_os("USERPROFILE").as_deref());
    match run_for(&file) {
        Run::PowerShell(arguments) => exited(run(powershell(arguments)).await),
        Run::Open => open(file).await,
    }
}

/// Windows has no Shortcuts. The loader refuses a shortcut action, so this
/// says why for one that did not go through it.
pub async fn shortcut(_name: String) -> Outcome {
    Outcome::Error(NO_SHORTCUTS.to_string())
}

/// Windows has no Shortcuts, so there are none to pick from, and this says
/// why rather than give an empty list.
pub async fn shortcut_names() -> Result<Vec<String>, String> {
    Err(NO_SHORTCUTS.to_string())
}

/// How Windows runs a script file.
#[derive(Debug, PartialEq, Eq)]
enum Run {
    /// PowerShell runs it with these arguments, and the action waits for its
    /// exit code.
    PowerShell(Vec<OsString>),
    /// Windows opens it as a double-click does, and the action does not wait.
    Open,
}

/// How a script file runs: a .ps1 with PowerShell, anything else as a
/// double-click opens it.
fn run_for(file: &Path) -> Run {
    let ps1 = file
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("ps1"));
    if !ps1 {
        return Run::Open;
    }
    let mut arguments: Vec<OsString> = [
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
    ]
    .into_iter()
    .map(OsString::from)
    .collect();
    arguments.push(file.as_os_str().to_os_string());
    Run::PowerShell(arguments)
}

/// What PowerShell gets for a shell line: no profile, so the line runs the
/// same for everyone, and no prompt, so a line that asks for input fails
/// rather than wait for an answer that never comes.
fn shell_arguments(line: &str) -> Vec<OsString> {
    let mut arguments: Vec<OsString> = ["-NoProfile", "-NonInteractive", "-Command"]
        .into_iter()
        .map(OsString::from)
        .collect();
    arguments.push(line.into());
    arguments
}

/// PowerShell with these arguments, and no console window: BandBridge runs
/// in the tray, and a window flashing up on a press would take the focus
/// from the app in front.
fn powershell(arguments: Vec<OsString>) -> Command {
    let mut command = Command::new(POWERSHELL);
    command.args(arguments).creation_flags(CREATE_NO_WINDOW.0);
    command
}

/// Run PowerShell to its end, and say how it ended: its exit code, and the
/// complaint it wrote to stderr. Err when it could not be run at all.
///
/// A line may start an app that goes on running, such as `winver`, and the
/// app keeps the handles it got from PowerShell. A pipe would stay open for
/// as long as the app runs, and reading one to its end would wait that long,
/// on threads nothing can stop. So PowerShell's stdin and stdout go nowhere,
/// its stderr goes to a file, and the file is read once PowerShell has ended.
async fn run(command: Command) -> Result<Ended, String> {
    run_in(command, &std::env::temp_dir()).await
}

/// [`run`], with the file for the complaint in `folder`. A folder that takes
/// no new file costs the complaint and not the run: PowerShell's stderr then
/// goes nowhere.
async fn run_in(mut command: Command, folder: &Path) -> Result<Ended, String> {
    let cannot = |err: std::io::Error| format!("cannot run it: {err}");
    let file = complaint_file(folder)
        .inspect_err(|err| {
            tracing::debug!(
                "no file for PowerShell's complaint in {}: {err}",
                folder.display()
            );
        })
        .ok();
    let stderr = match file.as_ref().map(File::try_clone) {
        Some(Ok(handle)) => Stdio::from(handle),
        _ => Stdio::null(),
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr)
        // Without this, giving up on the task leaves PowerShell running.
        .kill_on_drop(true);
    let mut powershell = command.spawn().map_err(cannot)?;
    let status = powershell.wait().await.map_err(cannot)?;
    Ok(Ended {
        code: status.code(),
        complaint: file.and_then(|file| complaint(&written(file))),
    })
}

/// A file in `folder` for what PowerShell writes to stderr. Windows deletes
/// it when the last handle to it closes: this process's, PowerShell's, and
/// those of any app PowerShell started. Every handle may share it, so one an
/// app keeps stops nothing.
fn complaint_file(folder: &Path) -> std::io::Result<File> {
    let name = format!(
        "bandbridge-{}-{:016x}.txt",
        std::process::id(),
        rand::random::<u64>()
    );
    OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .share_mode((FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE).0)
        .custom_flags(FILE_FLAG_DELETE_ON_CLOSE.0)
        .attributes(FILE_ATTRIBUTE_TEMPORARY.0)
        .open(folder.join(name))
}

/// What PowerShell wrote to the file, as text. PowerShell's handle to it
/// shares this one's place in it, which is past what it wrote, so the read
/// starts over from the start.
fn written(mut file: File) -> String {
    let mut bytes = Vec::new();
    let read = file
        .seek(SeekFrom::Start(0))
        .and_then(|_| file.read_to_end(&mut bytes));
    if let Err(err) = read {
        tracing::debug!("what PowerShell wrote to stderr could not be read: {err}");
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// The part of what PowerShell wrote to stderr worth showing.
///
/// PowerShell writes an error as its message, its place, and then lines that
/// start with "+" after their spaces, which quote the code and name the
/// error. It wraps each line at 120 columns, the place's among them: see
/// [`place_above`]. Most errors put the message first, above the place. A
/// parse error puts its place first, and its message after the code it
/// quotes. The lines of the message are joined back into one.
///
/// - No "+" line: what a program the line ran wrote, whose first line is
///   the complaint, as on the Mac.
/// - No place above the first "+" line, as for `Write-Error`: every line
///   above it.
/// - A place below the first line: every line above the place.
/// - A place on the first line: after the "+" lines under it, the lines up
///   to the next "+" line, or to the next place when it comes first, which
///   is where a second error in the same record starts.
fn complaint(text: &str) -> Option<String> {
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    let Some(plus) = next_plus(&lines, 0) else {
        return first_line(text.as_bytes());
    };
    let message = match place_above(&lines, 0, plus) {
        Some(0) => {
            let from = lines[plus..]
                .iter()
                .position(|line| !is_plus(line))
                .map_or(lines.len(), |past| plus + past);
            let to = match next_plus(&lines, from) {
                Some(next) => place_above(&lines, from, next).unwrap_or(next),
                None => lines.len(),
            };
            &lines[from..to]
        }
        Some(place) => &lines[..place],
        None => &lines[..plus],
    };
    if message.is_empty() {
        return first_line(text.as_bytes());
    }
    let message: Vec<&str> = message.iter().map(|line| line.trim()).collect();
    Some(message.join(" "))
}

/// Whether a line quotes the code or names the error: it starts with "+"
/// after its spaces.
fn is_plus(line: &str) -> bool {
    line.trim_start().starts_with('+')
}

/// The first "+" line at `from` or below it.
fn next_plus(lines: &[&str], from: usize) -> Option<usize> {
    lines
        .iter()
        .skip(from)
        .position(|line| is_plus(line))
        .map(|past| from + past)
}

/// Where the place above the "+" line at `plus` starts, no higher than
/// `from`: the nearest line above it that starts a place.
///
/// A place runs from a line that starts with "At " down to the "+" line.
/// PowerShell wraps it at 120 columns like any line: in a word when a long
/// path has no space, at a space when it has one, which it keeps at the end
/// of the line. So its lines, joined with nothing between them, are the
/// place: `At line:1 char:8` for a command line, or
/// `At C:\scripts\lamp.ps1:12 char:5` for a script. The nearest such line is
/// the one, so a message line of its own that starts with "At ", above the
/// place, stays in the message.
fn place_above(lines: &[&str], from: usize, plus: usize) -> Option<usize> {
    (from..plus)
        .rev()
        .find(|&start| lines[start].starts_with("At ") && is_place(&lines[start..plus].concat()))
}

/// Whether a text is a place, to its end: `At <file>:<n> char:<n>`, where
/// the file of a command line is `line`.
fn is_place(text: &str) -> bool {
    let number = |text: &str| !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit());
    let Some(rest) = text.trim_end().strip_prefix("At ") else {
        return false;
    };
    let Some((at, column)) = rest.rsplit_once(" char:") else {
        return false;
    };
    let Some((file, row)) = at.rsplit_once(':') else {
        return false;
    };
    !file.is_empty() && number(row) && number(column)
}

/// Open a file as a double-click opens it: an .exe or a .bat runs, a .lnk
/// starts what it points at, and a document opens in its app. A double-click
/// does not wait for what it started, so neither does this: the action is
/// done once Windows has started it.
///
/// The call can block, so it runs on a thread of its own.
async fn open(file: PathBuf) -> Outcome {
    match tokio::task::spawn_blocking(move || shell_open(&file)).await {
        Ok(Ok(())) => Outcome::Done,
        Ok(Err(why)) => Outcome::Error(why),
        Err(err) => Outcome::Error(format!("the action stopped part way: {err}")),
    }
}

/// ShellExecuteEx with the open verb, in the file's own folder, as Explorer
/// opens a file.
///
/// No dialog: a failure goes to the event log, not onto the screen of a
/// person who pressed a button on their wrist. The call returns only once
/// the file has started.
fn shell_open(file: &Path) -> Result<(), String> {
    let wide = |text: &std::ffi::OsStr| -> Vec<u16> { text.encode_wide().chain([0]).collect() };
    let path = wide(file.as_os_str());
    let folder = file.parent().map(|folder| wide(folder.as_os_str()));
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_FLAG_NO_UI | SEE_MASK_NOASYNC,
        lpVerb: w!("open"),
        lpFile: PCWSTR(path.as_ptr()),
        lpDirectory: folder
            .as_ref()
            .map_or(PCWSTR::null(), |folder| PCWSTR(folder.as_ptr())),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    // The shell may hand the file to an extension of its own through COM, so
    // COM is set up on this thread for the call, and taken down after it.
    let com = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
    let opened = unsafe { ShellExecuteExW(&mut info) };
    if com.is_ok() {
        unsafe { CoUninitialize() };
    }
    opened.map_err(|err| format!("cannot open it: {}", err.message()))
}

/// Shell lines for the tests, in Windows PowerShell.
#[cfg(test)]
pub mod lines {
    use std::path::Path;

    /// The exit code PowerShell gives a line that names no command it can
    /// find.
    pub const NOT_FOUND: i32 = 1;

    /// A line that adds `word` and a line break to the end of `file`.
    /// `echo` would write UTF-16 in Windows PowerShell, so the file is
    /// written through .NET, as plain text.
    pub fn append(word: &str, file: &Path) -> String {
        format!(
            "[IO.File]::AppendAllText('{}', \"{word}`n\")",
            file.display()
        )
    }

    /// A line that writes `text` to stderr.
    pub fn to_stderr(text: &str) -> String {
        format!("[Console]::Error.WriteLine('{text}')")
    }

    /// A line that waits this many milliseconds.
    pub fn pause(milliseconds: u64) -> String {
        format!("Start-Sleep -Milliseconds {milliseconds}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::{Started, expand_home, finished, run_once, start};
    use crate::config::Does;
    use std::ffi::OsStr;
    use std::time::{Duration, Instant};

    // --- what a shell line and a script build (W3, W4) -----------------------

    /// A shell line goes to Windows PowerShell as written, with no profile
    /// and no prompt.
    #[test]
    fn a_shell_line_goes_to_powershell_as_written() {
        let line = r#"Get-Date | Out-File "C:\a folder\now.txt""#;
        let command = powershell(shell_arguments(line));
        let command = command.as_std();
        assert_eq!(command.get_program(), "powershell.exe");
        let arguments: Vec<&OsStr> = command.get_args().collect();
        assert_eq!(
            arguments,
            ["-NoProfile", "-NonInteractive", "-Command", line]
        );
    }

    /// A .ps1 runs with PowerShell, where a double-click would open it in
    /// Notepad. Any other file opens as a double-click opens it: a .bat, a
    /// .cmd and an .exe run, a .lnk starts what it points at, and a document
    /// opens in its app.
    #[test]
    fn a_ps1_runs_with_powershell_and_any_other_file_opens() {
        let ps1 = Path::new(r"C:\Users\someone\lamp.ps1");
        let arguments: Vec<OsString> = [
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ]
        .into_iter()
        .map(OsString::from)
        .chain([ps1.as_os_str().to_os_string()])
        .collect();
        assert_eq!(run_for(ps1), Run::PowerShell(arguments));
        assert!(
            matches!(run_for(Path::new(r"C:\x\LAMP.PS1")), Run::PowerShell(_)),
            "the extension in either case"
        );
        for other in [
            r"C:\x\lamp.bat",
            r"C:\x\lamp.cmd",
            r"C:\x\lamp.exe",
            r"C:\x\Lamp.lnk",
            r"C:\x\notes.txt",
            r"C:\x\lamp",
            r"C:\x\lamp.ps1.txt",
        ] {
            assert_eq!(run_for(Path::new(other)), Run::Open, "{other}");
        }
    }

    /// A leading ~ is the home folder, with either slash after it.
    #[test]
    fn a_leading_tilde_is_the_home_folder_with_either_slash() {
        let home = Some(OsStr::new(r"C:\Users\someone"));
        assert_eq!(
            expand_home(r"~\scripts\lamp.ps1", home),
            PathBuf::from(r"C:\Users\someone\scripts\lamp.ps1")
        );
        assert_eq!(
            expand_home("~/scripts/lamp.ps1", home),
            PathBuf::from(r"C:\Users\someone\scripts\lamp.ps1")
        );
        assert_eq!(
            expand_home(r"~other\lamp.ps1", home),
            PathBuf::from(r"~other\lamp.ps1")
        );
    }

    // --- running them --------------------------------------------------------

    /// Quotes, a pipe and a `$` reach PowerShell as written.
    #[tokio::test]
    async fn a_shell_line_with_quotes_and_a_pipe_runs_as_written() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("out");
        let line = format!(
            r#""a b", 'c' | ForEach-Object {{ [IO.File]::AppendAllText('{}', "$_`n") }}"#,
            file.display()
        );
        assert_eq!(
            run_once(&Does::Shell(line)).await,
            Outcome::Exit {
                code: 0,
                error: None
            }
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "a b\nc\n");
    }

    /// A .ps1 ends in its exit code, and one that fails brings the first
    /// line it wrote to stderr, as a script does on the Mac.
    #[tokio::test]
    async fn a_ps1_gives_its_exit_code_and_why() {
        let folder = tempfile::tempdir().unwrap();
        let works = folder.path().join("works.ps1");
        std::fs::write(&works, "exit 0\r\n").unwrap();
        let fails = folder.path().join("fails.ps1");
        std::fs::write(
            &fails,
            "[Console]::Error.WriteLine('the lamp is away')\r\nexit 4\r\n",
        )
        .unwrap();

        let run = |path: &Path| {
            let does = Does::Script(path.display().to_string());
            async move { run_once(&does).await }
        };
        assert_eq!(
            run(&works).await,
            Outcome::Exit {
                code: 0,
                error: None
            }
        );
        assert_eq!(
            run(&fails).await,
            Outcome::Exit {
                code: 4,
                error: Some("the lamp is away".to_string())
            }
        );
        let missing = run(&folder.path().join("missing.ps1")).await;
        assert!(
            matches!(&missing, Outcome::Exit { code, error: Some(why) } if *code != 0 && why.contains("missing.ps1")),
            "{missing:?}"
        );
    }

    /// Any other file opens as a double-click opens it, in its own folder,
    /// and the action is done once Windows has started it. A .bat runs here,
    /// and writes where it ran.
    #[tokio::test]
    async fn a_bat_file_opens_as_a_double_click_opens_it() {
        let folder = tempfile::tempdir().unwrap();
        let bat = folder.path().join("where.bat");
        std::fs::write(&bat, "@echo off\r\necho %CD%> \"%~dp0ran.txt\"\r\n").unwrap();
        assert_eq!(
            run_once(&Does::Script(bat.display().to_string())).await,
            Outcome::Done
        );

        // A double-click does not wait, and neither does the action.
        let ran = folder.path().join("ran.txt");
        let mut waited = Duration::ZERO;
        while !ran.exists() && waited < Duration::from_secs(10) {
            tokio::time::sleep(Duration::from_millis(100)).await;
            waited += Duration::from_millis(100);
        }
        let written = std::fs::read_to_string(&ran).expect("the .bat ran");
        assert!(
            written
                .trim()
                .eq_ignore_ascii_case(&folder.path().display().to_string()),
            "it ran in {written:?}"
        );
    }

    /// A file that is not there says so, and puts no dialog on the screen.
    #[tokio::test]
    async fn a_file_that_is_not_there_is_the_error() {
        let folder = tempfile::tempdir().unwrap();
        let missing = folder.path().join("missing.bat");
        match run_once(&Does::Script(missing.display().to_string())).await {
            Outcome::Error(why) => assert!(why.starts_with("cannot open it: "), "{why}"),
            other => panic!("expected the error, got {other:?}"),
        }
    }

    // --- a line that starts an app ----------------------------------------------

    /// Starts ping, which runs for 14 seconds, beside PowerShell, with
    /// PowerShell's handles, as an app a line opens does.
    const STARTS_PING: &str =
        "Start-Process -NoNewWindow -FilePath ping.exe -ArgumentList '-n','15','127.0.0.1'";

    /// A line that starts a program and leaves it running is over when
    /// PowerShell ends, not when the program does.
    #[tokio::test]
    async fn a_line_that_starts_a_program_ends_when_powershell_does() {
        let started = Instant::now();
        let outcome = run_once(&Does::Shell(STARTS_PING.to_string())).await;
        let took = started.elapsed();
        assert_eq!(
            outcome,
            Outcome::Exit {
                code: 0,
                error: None
            }
        );
        assert!(took < Duration::from_secs(5), "it took {took:?}");
    }

    /// The same, from a .ps1.
    #[tokio::test]
    async fn a_ps1_that_starts_a_program_ends_when_powershell_does() {
        let folder = tempfile::tempdir().unwrap();
        let ps1 = folder.path().join("starts-ping.ps1");
        std::fs::write(&ps1, format!("{STARTS_PING}\r\n")).unwrap();
        let started = Instant::now();
        let outcome = run_once(&Does::Script(ps1.display().to_string())).await;
        let took = started.elapsed();
        assert_eq!(
            outcome,
            Outcome::Exit {
                code: 0,
                error: None
            }
        );
        assert!(took < Duration::from_secs(5), "it took {took:?}");
    }

    /// A line that does not end is given up on at the deadline, as a press
    /// gives up on it, and its PowerShell is stopped with it.
    #[tokio::test]
    async fn a_line_that_hangs_is_given_up_on_and_its_powershell_stopped() {
        use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
        use windows::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
            WaitForSingleObject,
        };

        let folder = tempfile::tempdir().unwrap();
        let said = folder.path().join("pid");
        let line = format!(
            "[IO.File]::WriteAllText('{}', \"$PID\"); Start-Sleep -Seconds 60",
            said.display()
        );
        let started = Instant::now();
        let Started::Running(task) = start(&Does::Shell(line)) else {
            panic!("a shell line runs in a task of its own");
        };

        // Hold a handle to that PowerShell while it runs, so its id cannot
        // name another process by the time it is checked.
        let mut pid = None;
        while pid.is_none() && started.elapsed() < Duration::from_secs(8) {
            tokio::time::sleep(Duration::from_millis(100)).await;
            pid = std::fs::read_to_string(&said)
                .ok()
                .and_then(|text| text.trim().parse::<u32>().ok());
        }
        let pid = pid.expect("PowerShell said its process id");
        let powershell = unsafe {
            OpenProcess(
                PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
                false,
                pid,
            )
        }
        .expect("PowerShell runs");

        let outcome = finished(task).await;
        let took = started.elapsed();
        assert_eq!(
            outcome,
            Outcome::Error("gave up after 10 seconds".to_string())
        );
        assert!(
            took >= Duration::from_secs(10) && took < Duration::from_secs(12),
            "it took {took:?}"
        );
        let ended = unsafe { WaitForSingleObject(powershell, 2000) };
        unsafe { CloseHandle(powershell) }.unwrap();
        assert_eq!(ended, WAIT_OBJECT_0, "PowerShell still runs");
    }

    // --- the complaint -------------------------------------------------------

    // What Windows PowerShell 5.1 wrote to stderr on a Windows 11 PC, run as a press
    // runs it: no window, stdin and stdout to nothing, stderr to a file.

    const NOT_FOUND: &str = "this-program-does-not-exist : The term 'this-program-does-not-exist' is not recognized as the name of a cmdlet, \r\nfunction, script file, or operable program. Check the spelling of the name, or if a path was included, verify that the \r\npath is correct and try again.\r\nAt line:1 char:1\r\n+ this-program-does-not-exist\r\n+ ~~~~~~~~~~~~~~~~~~~~~~~~~~~\r\n    + CategoryInfo          : ObjectNotFound: (this-program-does-not-exist:String) [], CommandNotFoundException\r\n    + FullyQualifiedErrorId : CommandNotFoundException\r\n \r\n";

    const THROWN: &str = "the lamp is away\r\nAt C:\\Users\\Jane Doe\\AppData\\Local\\Temp\\t005-samples\\throws.ps1:1 char:1\r\n+ throw \"the lamp is away\"\r\n+ ~~~~~~~~~~~~~~~~~~~~~~~~\r\n    + CategoryInfo          : OperationStopped: (the lamp is away:String) [], RuntimeException\r\n    + FullyQualifiedErrorId : the lamp is away\r\n \r\n";

    const WRITE_ERROR: &str = "Write-Error 'the lamp is away' : the lamp is away\r\n    + CategoryInfo          : NotSpecified: (:) [Write-Error], WriteErrorException\r\n    + FullyQualifiedErrorId : Microsoft.PowerShell.Commands.WriteErrorException\r\n \r\n";

    const NATIVE: &str = "FINDSTR: Cannot open C:\\no-such-folder\\lamp.txt\r\n";

    /// `echo a && echo b`: a parse error, which puts its place first.
    const AND_AND: &str = "At line:1 char:8\r\n+ echo a && echo b\r\n+        ~~\r\nThe token '&&' is not a valid statement separator in this version.\r\n    + CategoryInfo          : ParserError: (:) [], ParentContainsErrorRecordException\r\n    + FullyQualifiedErrorId : InvalidEndOfLine\r\n \r\n";

    /// `throw ('b' * 118 + ' At least one lamp is away, and this ends the
    /// message.')`: a wrapped message whose second line starts with "At ".
    const AT_LEAST: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb \r\nAt least one lamp is away, and this ends the message.\r\nAt line:1 char:1\r\n+ throw ('b' * 118 + ' At least one lamp is away, and this ends the mes ...\r\n+ ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~\r\n    + CategoryInfo          : OperationStopped: (bbbbbbbbbbbbbbb...ds the message.:String) [], RuntimeException\r\n    + FullyQualifiedErrorId : bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb \r\n   bbbbbbbbbbbbbbbbbbbbbbbbbbbbbb At least one lamp is away, and this ends the message.\r\n \r\n";

    /// `throw 'the lamp is away'` in a .ps1 whose place is 142 columns: the
    /// place wraps inside a word of the path, which has no space.
    const LONG_THROWN: &str = "the lamp is away\r\nAt C:\\src\\bb-rereview2\\scripts-for-the-band-buttons-in-the-office\\toggle-the-desk-lamp-and-the-shelf-lights\\when-the-ba\r\nnd-says-so.ps1:1 char:1\r\n+ throw 'the lamp is away'\r\n+ ~~~~~~~~~~~~~~~~~~~~~~~~\r\n    + CategoryInfo          : OperationStopped: (the lamp is away:String) [], RuntimeException\r\n    + FullyQualifiedErrorId : the lamp is away\r\n \r\n";

    /// `echo a && echo b` in a .ps1 whose place is 146 columns: a parse
    /// error whose place wraps.
    const LONG_PARSE_ERROR: &str = "At C:\\src\\bb-rereview2\\scripts-for-the-band-buttons-in-the-office\\toggle-the-desk-lamp-and-the-shelf-lights\\when-the-ba\r\nnd-says-so-too.ps1:1 char:8\r\n+ echo a && echo b\r\n+        ~~\r\nThe token '&&' is not a valid statement separator in this version.\r\n    + CategoryInfo          : ParserError: (:) [], ParentContainsErrorRecordException\r\n    + FullyQualifiedErrorId : InvalidEndOfLine\r\n \r\n";

    /// The same two in a folder whose path has spaces: the place wraps at a
    /// space, which stays at the end of its line.
    const LONG_THROWN_SPACES: &str = "the lamp is away\r\nAt C:\\src\\bb-rereview2\\Band scripts for the office\\Toggle the desk lamp and the shelf lights in the \r\nroom\\when-the-band-says-so.ps1:1 char:1\r\n+ throw 'the lamp is away'\r\n+ ~~~~~~~~~~~~~~~~~~~~~~~~\r\n    + CategoryInfo          : OperationStopped: (the lamp is away:String) [], RuntimeException\r\n    + FullyQualifiedErrorId : the lamp is away\r\n \r\n";

    const LONG_PARSE_ERROR_SPACES: &str = "At C:\\src\\bb-rereview2\\Band scripts for the office\\Toggle the desk lamp and the shelf lights in the \r\nroom\\when-the-band-says-so-too.ps1:1 char:8\r\n+ echo a && echo b\r\n+        ~~\r\nThe token '&&' is not a valid statement separator in this version.\r\n    + CategoryInfo          : ParserError: (:) [], ParentContainsErrorRecordException\r\n    + FullyQualifiedErrorId : InvalidEndOfLine\r\n \r\n";

    /// The "At least" throw in a .ps1 whose place wraps: a message line that
    /// starts with "At ", above a place that wraps.
    const LONG_AT_LEAST: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb \r\nAt least one lamp is away, and this ends the message.\r\nAt C:\\src\\bb-rereview2\\scripts-for-the-band-buttons-in-the-office\\toggle-the-desk-lamp-and-the-shelf-lights\\when-the-ba\r\nnd-says-at-least.ps1:1 char:1\r\n+ throw ('b' * 118 + ' At least one lamp is away, and this ends the mes ...\r\n+ ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~\r\n    + CategoryInfo          : OperationStopped: (bbbbbbbbbbbbbbb...ds the message.:String) [], RuntimeException\r\n    + FullyQualifiedErrorId : bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb \r\n   bbbbbbbbbbbbbbbbbbbbbbbbbbbbbb At least one lamp is away, and this ends the message.\r\n \r\n";

    /// `cmd /c echo the lamp is away 1>&2 & exit 2`: two parse errors in one
    /// record, each with its place.
    const TWO_PARSE_ERRORS: &str = "At line:1 char:30\r\n+ cmd /c echo the lamp is away 1>&2 & exit 2\r\n+                              ~~~~\r\nThe '1>&2' operator is reserved for future use.\r\nAt line:1 char:35\r\n+ cmd /c echo the lamp is away 1>&2 & exit 2\r\n+                                   ~\r\nThe ampersand (&) character is not allowed. The & operator is reserved for future use; wrap an ampersand in double \r\nquotation marks (\"&\") to pass it as part of a string.\r\n    + CategoryInfo          : ParserError: (:) [], ParentContainsErrorRecordException\r\n    + FullyQualifiedErrorId : RedirectionNotSupported\r\n \r\n";

    /// An error's message is the lines above where it happened, joined into
    /// one: PowerShell wraps it at 120 columns.
    #[test]
    fn a_powershell_error_is_its_whole_message() {
        assert_eq!(
            complaint(NOT_FOUND).as_deref(),
            Some(
                "this-program-does-not-exist : The term 'this-program-does-not-exist' is not \
                 recognized as the name of a cmdlet, function, script file, or operable program. \
                 Check the spelling of the name, or if a path was included, verify that the path \
                 is correct and try again."
            )
        );
        assert_eq!(complaint(THROWN).as_deref(), Some("the lamp is away"));
        assert_eq!(
            complaint(WRITE_ERROR).as_deref(),
            Some("Write-Error 'the lamp is away' : the lamp is away")
        );
        // A message line that starts with "At " is no place.
        assert_eq!(
            complaint(AT_LEAST),
            Some(format!(
                "{} At least one lamp is away, and this ends the message.",
                "b".repeat(118)
            ))
        );
    }

    /// A parse error puts its place first. Its message is the lines after the
    /// quoted code, up to the next place or "+ " line: the first error's,
    /// when a record holds two.
    #[test]
    fn a_parse_error_is_its_message_not_its_place() {
        assert_eq!(
            complaint(AND_AND).as_deref(),
            Some("The token '&&' is not a valid statement separator in this version.")
        );
        assert_eq!(
            complaint(TWO_PARSE_ERRORS).as_deref(),
            Some("The '1>&2' operator is reserved for future use.")
        );
        // The same message, wrapped, after a place.
        let wrapped = TWO_PARSE_ERRORS
            .split_once("The '1>&2' operator is reserved for future use.\r\n")
            .unwrap()
            .1;
        assert_eq!(
            complaint(wrapped).as_deref(),
            Some(
                "The ampersand (&) character is not allowed. The & operator is reserved for \
                 future use; wrap an ampersand in double quotation marks (\"&\") to pass it as \
                 part of a string."
            )
        );
    }

    /// A place PowerShell wrapped is still a place, whether it wrapped in a
    /// word or at a space, and a message line that starts with "At " above
    /// it stays in the message.
    #[test]
    fn a_wrapped_place_is_still_a_place() {
        let reason = "The token '&&' is not a valid statement separator in this version.";
        for (sample, message) in [
            (LONG_THROWN, "the lamp is away"),
            (LONG_THROWN_SPACES, "the lamp is away"),
            (LONG_PARSE_ERROR, reason),
            (LONG_PARSE_ERROR_SPACES, reason),
        ] {
            assert_eq!(complaint(sample).as_deref(), Some(message));
        }
        assert_eq!(
            complaint(LONG_AT_LEAST),
            Some(format!(
                "{} At least one lamp is away, and this ends the message.",
                "b".repeat(118)
            ))
        );

        // The place is the nearest run of lines above the "+" line that
        // starts with "At " and joins into one.
        let lines: Vec<&str> = LONG_AT_LEAST.lines().collect();
        let plus = next_plus(&lines, 0).unwrap();
        assert_eq!(place_above(&lines, 0, plus), Some(2));
        let lines: Vec<&str> = LONG_PARSE_ERROR_SPACES.lines().collect();
        assert_eq!(place_above(&lines, 0, 2), Some(0));
        assert!(is_place(&lines[..2].concat()));
        assert!(!is_place(lines[0]), "the first line alone is no place");
    }

    /// A place is exactly `At line:<n> char:<n>` or `At <file>:<n>
    /// char:<n>`, to its end.
    #[test]
    fn a_place_is_a_line_and_a_column() {
        for place in [
            "At line:1 char:8",
            "At line:12 char:140",
            r"At C:\Users\Jane Doe\scripts\lamp.ps1:3 char:1",
        ] {
            assert!(is_place(place), "{place}");
        }
        for not_a_place in [
            "At least one lamp is away, and this ends the message.",
            "At line:one char:8",
            "At line:1 char:",
            "At :1 char:2",
            "At line:1 char:8 and more",
            "at line:1 char:8",
        ] {
            assert!(!is_place(not_a_place), "{not_a_place}");
        }
    }

    /// What a program wrote, and anything else with no place in it, gives
    /// its first line, as on the Mac.
    #[test]
    fn anything_else_is_its_first_line() {
        assert_eq!(
            complaint(NATIVE).as_deref(),
            Some("FINDSTR: Cannot open C:\\no-such-folder\\lamp.txt")
        );
        assert_eq!(
            complaint("\r\n  the lamp is away \r\nand more\r\n").as_deref(),
            Some("the lamp is away")
        );
        // A line that only starts like a place, with nothing above it.
        assert_eq!(
            complaint("At least one lamp is away\r\n").as_deref(),
            Some("At least one lamp is away")
        );
        assert_eq!(complaint(""), None);
        assert_eq!(complaint(" \r\n"), None);
    }

    /// A folder that takes no new file costs the complaint, not the run: the
    /// line runs, and ends in its exit code, with no why.
    #[tokio::test]
    async fn a_folder_that_takes_no_file_costs_the_complaint_not_the_run() {
        let folder = tempfile::tempdir().unwrap();
        // A file where the folder should be.
        let not_a_folder = folder.path().join("not-a-folder");
        std::fs::write(&not_a_folder, "").unwrap();
        assert!(complaint_file(&not_a_folder).is_err(), "it took a file");

        let fails = format!("{}; exit 3", lines::to_stderr("the lamp is away"));
        let mark = folder.path().join("ran");
        let works = format!("{}; exit 0", lines::append("x", &mark));
        for (line, code) in [(fails, 3), (works, 0)] {
            let ended = run_in(powershell(shell_arguments(&line)), &not_a_folder).await;
            assert_eq!(
                ended,
                Ok(Ended {
                    code: Some(code),
                    complaint: None
                }),
                "{line}"
            );
        }
        assert_eq!(
            std::fs::read_to_string(&mark).unwrap(),
            "x\n",
            "the line ran"
        );
    }

    /// A press gets the whole message too.
    #[tokio::test]
    async fn a_press_gets_the_whole_message() {
        let outcome = run_once(&Does::Shell("this-program-does-not-exist".to_string())).await;
        let Outcome::Exit {
            code: 1,
            error: Some(why),
        } = outcome
        else {
            panic!("expected exit 1 and why, got {outcome:?}");
        };
        assert!(
            why.ends_with("verify that the path is correct and try again."),
            "{why}"
        );
    }

    // --- no Shortcuts on Windows (W5) ----------------------------------------

    /// Windows has no Shortcuts app. The action and the list both say what
    /// to use instead.
    #[tokio::test]
    async fn a_shortcut_says_to_use_a_script_or_shell_action() {
        let why = "Shortcuts are a Mac feature. Use a Script or Shell action on Windows.";
        assert_eq!(SHORTCUTS_REFUSED, Some(why));
        assert_eq!(NO_SHORTCUTS, why);
        assert_eq!(
            shortcut("Start Focus".to_string()).await,
            Outcome::Error(why.to_string())
        );
        assert_eq!(shortcut_names().await, Err(why.to_string()));
    }
}
