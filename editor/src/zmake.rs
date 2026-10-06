//! Check for the build tools, and run zmake.
//!
//! zmake is GPL-3, so the editor runs it as a separate program and does not
//! include it. After a failure, zmake prints its trace and FAILED, then waits
//! for a key press. The editor gives it one: its input holds one line end,
//! then closes. So zmake ends, and its own words are the last it says. Its
//! two outputs share one pipe, so they keep the order zmake wrote them in. A
//! zmake that runs on regardless is stopped after two minutes, so a build
//! always ends.

use crate::output::shown;
use serde::Serialize;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

/// What the build tools notice shows.
#[derive(Debug, Clone, Serialize)]
pub struct ToolsStatus {
    pub ready: bool,
    pub dir: String,
    /// The system the editor runs on, as the notice names it: "Mac", "PC",
    /// or "computer".
    pub system: &'static str,
    /// Where to run [`steps`](Self::steps), as the notice says it: "terminal"
    /// on the Mac, and "PowerShell" on Windows, whose commands are
    /// PowerShell's own.
    pub shell: &'static str,
    /// Python 3, Pillow and zmake, in the notice's order.
    pub tools: Vec<Tool>,
    /// The commands that set up what is missing, on this system.
    pub steps: Vec<String>,
}

/// One build tool, and whether this system has it.
#[derive(Debug, Clone, Serialize)]
pub struct Tool {
    pub name: &'static str,
    pub found: bool,
    /// What was found: a version, where it is, or both. For a missing tool,
    /// what it is for.
    pub detail: String,
}

/// How a zmake run ended.
#[derive(Debug, Clone)]
pub struct BuildResult {
    /// Whether the app's .bin is in its `dist` folder.
    pub ok: bool,
    pub bin: Option<String>,
    pub zip: Option<String>,
    /// Why the build did not finish, in the editor's words. zmake's own
    /// words come through the lines [`build`] hands on.
    pub why: Option<String>,
    /// Why the .zip is not beside a .bin that was built.
    pub zip_error: Option<String>,
}

impl BuildResult {
    fn stopped(why: String) -> Self {
        BuildResult {
            ok: false,
            bin: None,
            zip: None,
            why: Some(why),
            zip_error: None,
        }
    }
}

/// How long zmake may run. A band app builds in seconds.
const ZMAKE_LIMIT: Duration = Duration::from_secs(120);

/// How long the editor still listens once it has stopped zmake. A program
/// zmake started may hold its output open for longer.
const STOP_GRACE: Duration = Duration::from_secs(2);

/// The system the editor runs on, as the tools notice names it.
const SYSTEM: &str = if cfg!(target_os = "macos") {
    "Mac"
} else if cfg!(windows) {
    "PC"
} else {
    "computer"
};

/// Where the tools notice's commands run.
const SHELL: &str = if cfg!(windows) { "PowerShell" } else { "terminal" };

/// A path as PowerShell reads it, whatever it holds: in single quotes, where
/// nothing expands, with each single quote in it doubled.
fn powershell_quoted(path: &str) -> String {
    format!("'{}'", path.replace('\'', "''"))
}

/// `BANDBRIDGE_TOOLS`, or the folder `scripts/build-band-apps.sh` uses.
pub fn tools_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("BANDBRIDGE_TOOLS") {
        return PathBuf::from(dir);
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_default();
    home.join(".local").join("share").join("bandbridge-tools")
}

pub fn python_in(dir: &Path) -> PathBuf {
    if cfg!(windows) {
        dir.join("Scripts").join("python.exe")
    } else {
        dir.join("bin").join("python")
    }
}

pub fn zmake_in(dir: &Path) -> PathBuf {
    dir.join("zmake-src")
}

/// A command that starts the tools' Python, for the tools check and for a
/// build. On Windows it gets no console window. The installed editor is a
/// window program with no console, so Windows would give Python, a console
/// program, a new one, and on a PC that opens consoles in Windows Terminal,
/// that window can take the focus from the editor. BandBridge starts
/// PowerShell the same way.
fn python_command(python: &Path) -> Command {
    #[allow(unused_mut)]
    let mut command = Command::new(python);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(windows::Win32::System::Threading::CREATE_NO_WINDOW.0);
    }
    command
}

/// Python's version, then Pillow's when Python has it, from one run.
fn versions(python: &Path) -> (Option<String>, Option<String>) {
    let asked = python_command(python)
        .args([
            "-c",
            "import sys; print('%d.%d.%d' % sys.version_info[:3]); import PIL; print(PIL.__version__)",
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    let Ok(out) = asked else {
        return (None, None);
    };
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let mut lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
    let python = lines.next().map(str::to_string);
    // Without Pillow, the import fails, and the run with it.
    let pillow = if out.status.success() {
        lines.next().map(str::to_string)
    } else {
        None
    };
    (python, pillow)
}

pub fn status_in(dir: &Path) -> ToolsStatus {
    let python = python_in(dir);
    let has_python = python.is_file();
    let (python_version, pillow_version) = if has_python {
        versions(&python)
    } else {
        (None, None)
    };
    let has_pillow = pillow_version.is_some();
    let zmake = zmake_in(dir);
    let has_zmake = zmake.join("zmake").is_dir();

    let tools = vec![
        Tool {
            name: "Python 3",
            found: has_python,
            detail: match (has_python, python_version) {
                (true, Some(version)) => format!("{version} · {}", shown(&python)),
                (true, None) => shown(&python),
                (false, _) => "Runs zmake".to_string(),
            },
        },
        Tool {
            name: "Pillow",
            found: has_pillow,
            detail: pillow_version.unwrap_or_else(|| "Converts the pictures".to_string()),
        },
        Tool {
            name: "zmake",
            found: has_zmake,
            detail: if has_zmake {
                shown(&zmake)
            } else {
                "Builds the .bin for the band".to_string()
            },
        },
    ];

    let d = dir.display();
    let mut steps = Vec::new();
    if cfg!(windows) {
        // PowerShell's own commands. A command that starts with a quoted path
        // needs PowerShell's call operator, &, to run.
        let at = |inside: &str| powershell_quoted(&format!("{d}{inside}"));
        if !has_python {
            steps.push(format!("py -3 -m venv {}", at("")));
        }
        if !has_pillow {
            steps.push(format!(
                "& {} -m pip install Pillow qrcode",
                at("\\Scripts\\python.exe")
            ));
        }
        if !has_zmake {
            steps.push(format!(
                "git clone https://github.com/melianmiko/zmake {}",
                at("\\zmake-src")
            ));
        }
    } else {
        if !has_python {
            steps.push(format!("python3 -m venv \"{d}\""));
        }
        if !has_pillow {
            steps.push(format!("\"{d}/bin/pip\" install Pillow qrcode"));
        }
        if !has_zmake {
            steps.push(format!("git clone https://github.com/melianmiko/zmake \"{d}/zmake-src\""));
        }
    }
    ToolsStatus {
        ready: steps.is_empty(),
        dir: d.to_string(),
        system: SYSTEM,
        shell: SHELL,
        tools,
        steps,
    }
}

/// Copy a built file to `app_dir/dist/`, creating the folder if needed.
///
/// On failure, return an error message in Simplified Technical English that names
/// the file and the reason.
fn copy_to_dist(app_dir: &Path, built: &Path) -> Result<String, String> {
    let file_name = built
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("<unknown>");
    let dist = app_dir.join("dist");
    std::fs::create_dir_all(&dist).map_err(|e| {
        format!(
            "Could not copy {} to {}: {e}",
            built.display(),
            dist.join(file_name).display()
        )
    })?;
    let file_name = built.file_name().ok_or_else(|| {
        format!(
            "Could not find file name in {}",
            built.display()
        )
    })?;
    let to = dist.join(file_name);
    std::fs::copy(built, &to).map_err(|e| {
        format!(
            "Could not copy {} to {}: {e}",
            built.display(),
            to.display()
        )
    })?;
    Ok(to.display().to_string())
}

/// Build `project` with zmake, then copy its `.bin` and `.zip` to
/// `app_dir/dist`. Each line zmake prints reaches `each` as zmake prints it.
pub fn build(tools: &Path, app_dir: &Path, project: &Path, each: impl FnMut(&str)) -> BuildResult {
    build_within(tools, app_dir, project, ZMAKE_LIMIT, each)
}

/// [`build`], with zmake stopped once it has run for `limit`.
pub fn build_within(
    tools: &Path,
    app_dir: &Path,
    project: &Path,
    limit: Duration,
    mut each: impl FnMut(&str),
) -> BuildResult {
    let name = project
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let python = python_in(tools);
    let cannot_start = |e: std::io::Error| {
        BuildResult::stopped(format!(
            "Could not start zmake with {}: {e}",
            python.display()
        ))
    };
    // zmake writes its log to its error output and the rest to its output.
    // Both go into one pipe, so its lines keep the order zmake wrote them
    // in, and FAILED never lands inside a trace.
    let (output, writer) = match std::io::pipe() {
        Ok(pipe) => pipe,
        Err(e) => return cannot_start(e),
    };
    let both = match writer.try_clone() {
        Ok(both) => both,
        Err(e) => return cannot_start(e),
    };
    let mut command = python_command(&python);
    command
        .args(["-m", "zmake"])
        .arg(project)
        .env("PYTHONPATH", zmake_in(tools))
        // Python holds back what it prints to a pipe until it ends. The
        // window shows each line as zmake says it.
        .env("PYTHONUNBUFFERED", "1")
        .stdin(Stdio::piped())
        .stdout(both)
        .stderr(writer);
    let started = command.spawn();
    // The command holds this side's copies of the pipe's writing end. They
    // close with it, so the pipe ends when zmake does.
    drop(command);
    let mut child = match started {
        Ok(child) => child,
        Err(e) => return cannot_start(e),
    };
    // The key zmake waits for after a failure. Dropping the pipe then closes
    // its input.
    if let Some(mut input) = child.stdin.take() {
        let _ = input.write_all(b"\n");
    }

    // One thread reads the pipe into a queue, so the wait below can stop a
    // zmake that does not end.
    let (tx, rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(output);
        let mut bytes = Vec::new();
        loop {
            bytes.clear();
            match reader.read_until(b'\n', &mut bytes) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let line = String::from_utf8_lossy(&bytes);
                    if tx
                        .send(line.trim_end_matches(['\r', '\n']).to_string())
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
    });

    let deadline = Instant::now() + limit;
    let mut stopped = false;
    loop {
        let wait = if stopped {
            STOP_GRACE
        } else {
            deadline.saturating_duration_since(Instant::now())
        };
        match rx.recv_timeout(wait) {
            Ok(line) => each(&line),
            Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) if stopped => break,
            Err(RecvTimeoutError::Timeout) => {
                let _ = child.kill();
                stopped = true;
            }
        }
    }
    let status = child.wait();
    if stopped {
        return BuildResult::stopped(format!(
            "zmake ran for {} s without finishing, so the editor stopped it.",
            limit.as_secs()
        ));
    }

    let built_bin = project.join("dist").join(format!("{name}.bin"));
    let built_zip = project.join("dist").join(format!("{name}.zip"));
    if !built_bin.is_file() {
        return BuildResult::stopped(format!("zmake did not make {name}.bin."));
    }
    match status {
        Ok(status) if status.success() => {}
        Ok(status) => {
            return BuildResult::stopped(match status.code() {
                Some(code) => format!("zmake stopped with status {code}."),
                None => "zmake was stopped by a signal.".to_string(),
            });
        }
        Err(e) => return BuildResult::stopped(format!("Could not wait for zmake to end: {e}")),
    }

    let bin = match copy_to_dist(app_dir, &built_bin) {
        Ok(path) => path,
        Err(e) => return BuildResult::stopped(e),
    };
    let (zip, zip_error) = if built_zip.is_file() {
        match copy_to_dist(app_dir, &built_zip) {
            Ok(path) => (Some(path), None),
            Err(e) => (None, Some(e)),
        }
    } else {
        (None, None)
    };
    BuildResult {
        ok: true,
        bin: Some(bin),
        zip,
        why: None,
        zip_error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generate::generate;
    use crate::layout::{Button, Layout};

    #[test]
    fn an_empty_tools_folder_lists_every_step() {
        let dir = tempfile::tempdir().unwrap();
        let status = status_in(dir.path());
        assert!(!status.ready);
        let steps = status.steps.join("\n");
        assert!(steps.contains("venv"), "{steps}");
        assert!(steps.contains("Pillow"), "{steps}");
        assert!(steps.contains("github.com/melianmiko/zmake"), "{steps}");
    }

    /// The notice's three rows, in its order. A missing tool says what it is
    /// for.
    #[test]
    fn an_empty_tools_folder_misses_all_three_tools() {
        let dir = tempfile::tempdir().unwrap();
        let rows: Vec<(&str, bool, String)> = status_in(dir.path())
            .tools
            .into_iter()
            .map(|t| (t.name, t.found, t.detail))
            .collect();
        assert_eq!(
            rows,
            [
                ("Python 3", false, "Runs zmake".to_string()),
                ("Pillow", false, "Converts the pictures".to_string()),
                ("zmake", false, "Builds the .bin for the band".to_string()),
            ]
        );
    }

    /// The notice names the system the editor runs on, and where its
    /// commands run.
    #[test]
    fn the_notice_names_this_system() {
        let status = status_in(Path::new("tools"));
        if cfg!(target_os = "macos") {
            assert_eq!((status.system, status.shell), ("Mac", "terminal"));
        } else if cfg!(windows) {
            assert_eq!((status.system, status.shell), ("PC", "PowerShell"));
        } else {
            assert_eq!((status.system, status.shell), ("computer", "terminal"));
        }
    }

    /// On Windows the commands are PowerShell's own: a path in single quotes,
    /// where nothing in it expands, and the call operator in front of a
    /// command that is a quoted path. A home folder with a space, as a
    /// Windows user name often has, stays one path.
    #[cfg(windows)]
    #[test]
    fn the_steps_run_in_powershell() {
        let dir = Path::new(r"C:\Users\Ada Lovelace\.local\share\bandbridge-tools");
        assert_eq!(
            status_in(dir).steps,
            [
                r"py -3 -m venv 'C:\Users\Ada Lovelace\.local\share\bandbridge-tools'",
                r"& 'C:\Users\Ada Lovelace\.local\share\bandbridge-tools\Scripts\python.exe' -m pip install Pillow qrcode",
                r"git clone https://github.com/melianmiko/zmake 'C:\Users\Ada Lovelace\.local\share\bandbridge-tools\zmake-src'",
            ]
        );
    }

    /// On Windows, Python starts with no console window, so the installed
    /// editor, which has none of its own, opens none. PowerShell stands in
    /// for Python here and says what console window it has. A test runs with
    /// a console, which a child would share without the flag.
    #[cfg(windows)]
    #[test]
    fn python_starts_with_no_console_window() {
        let asked = python_command(Path::new("powershell.exe"))
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Add-Type -Namespace T -Name C -MemberDefinition \
                 '[DllImport(\"kernel32.dll\")] public static extern System.IntPtr GetConsoleWindow();'; \
                 [T.C]::GetConsoleWindow()",
            ])
            .stdin(Stdio::null())
            .output()
            .expect("PowerShell runs");
        let said = String::from_utf8_lossy(&asked.stdout).trim().to_string();
        assert_eq!(
            said,
            "0",
            "it had a console window: {said} {}",
            String::from_utf8_lossy(&asked.stderr)
        );
    }

    /// A single quote in a path is doubled, which is how PowerShell reads one
    /// inside single quotes.
    #[test]
    fn a_quote_in_a_path_stays_part_of_it_in_powershell() {
        assert_eq!(powershell_quoted(r"C:\Users\O'Neil"), r"'C:\Users\O''Neil'");
        assert_eq!(powershell_quoted(r"C:\a $b `c"), r"'C:\a $b `c'");
    }

    #[test]
    fn python_is_where_a_venv_puts_it() {
        let dir = Path::new("tools");
        let python = python_in(dir);
        if cfg!(windows) {
            assert_eq!(python, dir.join("Scripts").join("python.exe"));
        } else {
            assert_eq!(python, dir.join("bin").join("python"));
        }
    }

    #[test]
    fn a_missing_python_fails_with_a_message() {
        let tools = tempfile::tempdir().unwrap();
        let app = tempfile::tempdir().unwrap();
        let mut heard = Vec::new();
        let result = build(
            tools.path(),
            app.path(),
            &app.path().join("generated/app"),
            |line| heard.push(line.to_string()),
        );
        assert!(!result.ok);
        let why = result.why.unwrap();
        assert!(why.starts_with("Could not start zmake with "), "{why}");
        assert!(heard.is_empty(), "{heard:?}");
    }

    #[test]
    fn a_copy_failure_is_named() {
        let app = tempfile::tempdir().unwrap();
        let app_path = app.path();

        // Create a file to copy.
        let temp_source = tempfile::tempdir().unwrap();
        let source_file = temp_source.path().join("slides.bin");
        std::fs::write(&source_file, b"test").unwrap();

        // Block the dist folder creation by creating a file named "dist".
        let dist_blocker = app_path.join("dist");
        std::fs::write(&dist_blocker, b"").unwrap();

        let result = copy_to_dist(app_path, &source_file);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("slides.bin"), "{}", err);
    }

    #[test]
    fn a_copy_succeeds_into_dist() {
        let app = tempfile::tempdir().unwrap();
        let app_path = app.path();

        // Create a file to copy.
        let temp_source = tempfile::tempdir().unwrap();
        let source_file = temp_source.path().join("slides.bin");
        std::fs::write(&source_file, b"test").unwrap();

        let result = copy_to_dist(app_path, &source_file);
        assert!(result.is_ok(), "{:?}", result);

        let dest = result.unwrap();
        let expected_end = ["dist", "slides.bin"];
        let dest_path = std::path::Path::new(&dest);
        assert!(dest_path.ends_with(expected_end.iter().collect::<std::path::PathBuf>()));
        assert!(dest_path.is_file());
    }

    /// Runs only where the build tools are set up.
    #[test]
    fn a_generated_app_builds_with_the_real_zmake() {
        let tools = tools_dir();
        if !status_in(&tools).ready {
            eprintln!("skipped: build tools are not set up in {}", tools.display());
            return;
        }
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("slides");
        std::fs::create_dir_all(&dir).unwrap();
        let mut layout = Layout::new("Slides");
        layout.buttons.push(Button::new("bb04", "#1f6feb"));
        let made = generate(&dir, &layout).unwrap();

        let mut heard = Vec::new();
        let result = build(&tools, &dir, &made.project, |line| {
            heard.push(line.to_string())
        });
        assert!(result.ok, "{:?}\n{}", result.why, heard.join("\n"));
        assert!(
            heard
                .iter()
                .any(|line| line.contains("Completed without error.")),
            "{heard:?}"
        );
        assert!(dir.join("dist/slides.bin").is_file());
        assert!(dir.join("dist/slides.zip").is_file());
    }

    /// A tools folder whose Python is a shell script, which stands in for
    /// zmake: it runs `script` with the project folder as `$3`, as zmake
    /// gets it after `-m zmake`.
    #[cfg(unix)]
    fn stand_in_tools(script: &str) -> tempfile::TempDir {
        use std::os::unix::fs::PermissionsExt;
        let tools = tempfile::tempdir().unwrap();
        let python = python_in(tools.path());
        std::fs::create_dir_all(python.parent().unwrap()).unwrap();
        std::fs::write(&python, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(&python, std::fs::Permissions::from_mode(0o755)).unwrap();
        tools
    }

    /// An app folder with an empty zmake project in it.
    #[cfg(unix)]
    fn an_app() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let app = root.path().join("slides");
        let project = app.join("generated").join("slides");
        std::fs::create_dir_all(&project).unwrap();
        (root, app, project)
    }

    /// Each line reaches the window while zmake still runs. This zmake goes
    /// on only once the window has heard its first line, and gives up after
    /// five seconds.
    #[cfg(unix)]
    #[test]
    fn zmakes_lines_arrive_while_it_runs() {
        let (_root, app, project) = an_app();
        let heard_first = app.join("heard");
        let tools = stand_in_tools(&format!(
            r#"echo "Processing app.json:"
for n in $(seq 50); do [ -f '{heard}' ] && break; sleep 0.1; done
[ -f '{heard}' ] || {{ echo "nobody listened"; exit 1; }}
echo "  Done"
mkdir -p "$3/dist" && printf bin > "$3/dist/slides.bin""#,
            heard = heard_first.display()
        ));

        let mut heard = Vec::new();
        let result = build(tools.path(), &app, &project, |line| {
            let _ = std::fs::write(&heard_first, b"");
            heard.push(line.to_string());
        });
        assert!(result.ok, "{:?} {heard:?}", result.why);
        assert_eq!(heard, ["Processing app.json:", "  Done"]);
        assert_eq!(
            result.bin.as_deref(),
            Some(app.join("dist/slides.bin").to_str().unwrap())
        );
        assert!(result.why.is_none());
    }

    /// The trace of a failure, as zmake prints it: a line to its output,
    /// the trace to its error output, FAILED to its output, then a wait for
    /// a key. Without one, Python's `input()` raises EOFError, and its trace
    /// would end the log: this stand-in says so, and stops with 1.
    #[cfg(unix)]
    const A_FAILING_ZMAKE: &str = r#"echo "Processing assets:"
echo "Traceback (most recent call last):" >&2
echo "IsADirectoryError: [Errno 21] Is a directory" >&2
echo "FAILED"
read key || { echo "EOFError: EOF when reading a line" >&2; exit 1; }"#;

    /// zmake prints its trace and FAILED, then waits for a key. The editor
    /// gives it one, so zmake ends, and its own words are the last ones, in
    /// the order zmake wrote them.
    #[cfg(unix)]
    #[test]
    fn a_build_that_fails_in_zmake_ends_with_zmakes_own_words() {
        let (_root, app, project) = an_app();
        let tools = stand_in_tools(A_FAILING_ZMAKE);
        let started = std::time::Instant::now();
        let mut heard = Vec::new();
        let result = build(tools.path(), &app, &project, |line| {
            heard.push(line.to_string())
        });
        assert!(
            started.elapsed() < std::time::Duration::from_secs(10),
            "it waited"
        );
        assert!(!result.ok);
        assert_eq!(
            result.why.as_deref(),
            Some("zmake did not make slides.bin.")
        );
        assert_eq!(
            heard,
            [
                "Processing assets:",
                "Traceback (most recent call last):",
                "IsADirectoryError: [Errno 21] Is a directory",
                "FAILED",
            ]
        );
        assert!(!app.join("dist").exists(), "nothing was built");
    }

    /// zmake writes its log to its error output and the rest to its output.
    /// Both reach the window in the order zmake wrote them, so FAILED never
    /// lands inside a trace.
    #[cfg(unix)]
    #[test]
    fn zmakes_two_outputs_keep_their_order() {
        let (_root, app, project) = an_app();
        let tools =
            stand_in_tools(r#"for n in $(seq 1 100); do echo "out $n"; echo "err $n" >&2; done"#);
        let mut heard = Vec::new();
        build(tools.path(), &app, &project, |line| {
            heard.push(line.to_string())
        });
        let wrote: Vec<String> = (1..=100)
            .flat_map(|n| [format!("out {n}"), format!("err {n}")])
            .collect();
        assert_eq!(heard, wrote);
    }

    /// A zmake that stops with an error, after it made the .bin, did not
    /// finish either.
    #[cfg(unix)]
    #[test]
    fn a_zmake_that_exits_with_an_error_did_not_build() {
        let (_root, app, project) = an_app();
        let tools =
            stand_in_tools(r#"mkdir -p "$3/dist" && printf bin > "$3/dist/slides.bin"; exit 3"#);
        let result = build(tools.path(), &app, &project, |_| {});
        assert!(!result.ok);
        assert_eq!(result.why.as_deref(), Some("zmake stopped with status 3."));
    }

    /// A zmake that never ends is stopped, so a build always ends and its
    /// button comes back.
    #[cfg(unix)]
    #[test]
    fn a_zmake_that_does_not_end_is_stopped() {
        let (_root, app, project) = an_app();
        let tools = stand_in_tools("echo \"Processing assets:\"\nexec sleep 30");
        let started = std::time::Instant::now();
        let mut heard = Vec::new();
        let result = build_within(
            tools.path(),
            &app,
            &project,
            std::time::Duration::from_secs(1),
            |line| heard.push(line.to_string()),
        );
        assert!(
            started.elapsed() < std::time::Duration::from_secs(10),
            "it waited"
        );
        assert!(!result.ok);
        assert_eq!(
            result.why.as_deref(),
            Some("zmake ran for 1 s without finishing, so the editor stopped it.")
        );
        assert_eq!(heard, ["Processing assets:"]);
    }

    /// A set-up tools folder: each row found, with what it found, and
    /// nothing to run.
    #[cfg(unix)]
    #[test]
    fn a_set_up_tools_folder_finds_all_three_tools() {
        let tools = stand_in_tools("echo 3.12.4\necho 11.0.0");
        std::fs::create_dir_all(zmake_in(tools.path()).join("zmake")).unwrap();
        let status = status_in(tools.path());
        assert!(status.ready, "{:?}", status.steps);
        assert!(status.steps.is_empty());
        let rows: Vec<(&str, bool, String)> = status
            .tools
            .into_iter()
            .map(|t| (t.name, t.found, t.detail))
            .collect();
        let python = crate::output::shown(&python_in(tools.path()));
        let zmake = crate::output::shown(&zmake_in(tools.path()));
        assert_eq!(
            rows,
            [
                ("Python 3", true, format!("3.12.4 · {python}")),
                ("Pillow", true, "11.0.0".to_string()),
                ("zmake", true, zmake),
            ]
        );
    }

    /// Python without Pillow: Python is found, with its version, and Pillow
    /// is missing, with the command that adds it.
    #[cfg(unix)]
    #[test]
    fn python_without_pillow_misses_pillow() {
        let tools = stand_in_tools("echo 3.13.9\nexit 1");
        std::fs::create_dir_all(zmake_in(tools.path()).join("zmake")).unwrap();
        let status = status_in(tools.path());
        assert!(!status.ready);
        let found: Vec<(&str, bool)> = status.tools.iter().map(|t| (t.name, t.found)).collect();
        assert_eq!(
            found,
            [("Python 3", true), ("Pillow", false), ("zmake", true)]
        );
        assert!(
            status.tools[0].detail.starts_with("3.13.9 · "),
            "{}",
            status.tools[0].detail
        );
        assert_eq!(status.steps.len(), 1);
        assert!(
            status.steps[0].contains("install Pillow"),
            "{:?}",
            status.steps
        );
    }
}
