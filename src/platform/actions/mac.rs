//! Running an action's command on macOS: a shell line and a script with sh,
//! and a shortcut with the `shortcuts` tool, which lists them too.

use crate::action::{Ended, Outcome, exited, expand_home, first_line, wait_for, with_reason};
use std::time::Duration;
use tokio::process::Command;

/// macOS runs shortcuts, so the loader refuses none.
pub const SHORTCUTS_REFUSED: Option<&str> = None;

/// Run a command line, and say how it went.
pub async fn shell(line: String) -> Outcome {
    // Through a shell, so a pipe, a quote, and `open -a Music` all
    // work the way they do in a terminal.
    let mut command = Command::new("sh");
    command.arg("-c").arg(line);
    exited(wait_for(command).await)
}

/// Run a script file, and say how it went.
pub async fn script(path: String) -> Outcome {
    // Handed to sh as a file to read, so it needs no execute bit. sh
    // does not expand a `~` in its own arguments, so that is done here.
    let mut command = Command::new("sh");
    command.arg(expand_home(&path, std::env::var_os("HOME").as_deref()));
    exited(wait_for(command).await)
}

/// Run a shortcut in Shortcuts, by the name it shows there, and say how it
/// went.
pub async fn shortcut(name: String) -> Outcome {
    let mut command = Command::new("shortcuts");
    command.arg("run").arg(name);
    ran(wait_for(command).await)
}

/// A shortcut: done, or the error, which names the exit code.
fn ran(ended: Result<Ended, String>) -> Outcome {
    match ended {
        Ok(Ended { code: Some(0), .. }) => Outcome::Done,
        Ok(Ended {
            code: Some(code),
            complaint,
        }) => Outcome::Error(with_reason(format!("exit {code}"), complaint)),
        other => exited(other),
    }
}

/// How long `shortcuts list` may take.
const LIST_LIMIT: Duration = Duration::from_secs(10);

/// The shortcuts in Shortcuts, by the names a `shortcut` action runs them
/// by, in the order `shortcuts list` gives them: the window's picker.
///
/// A failure says why. An empty list would read as a Mac with no shortcuts.
pub async fn shortcut_names() -> Result<Vec<String>, String> {
    let mut command = Command::new("shortcuts");
    command.arg("list");
    names_listed(command, LIST_LIMIT).await
}

/// The names a listing command prints, one a line, or why there are none.
async fn names_listed(mut command: Command, limit: Duration) -> Result<Vec<String>, String> {
    // Without this, giving up on the listing leaves it running.
    command.kill_on_drop(true);
    let not_listed = |why: String| format!("the shortcuts could not be listed: {why}");
    let output = match tokio::time::timeout(limit, command.output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(err)) => return Err(not_listed(format!("cannot run shortcuts: {err}"))),
        Err(_) => {
            return Err(not_listed(format!(
                "shortcuts did not answer in {} seconds",
                limit.as_secs()
            )));
        }
    };
    if !output.status.success() {
        let ended = match output.status.code() {
            Some(code) => format!("shortcuts list ended with exit {code}"),
            None => "shortcuts list was stopped by a signal".to_string(),
        };
        return Err(not_listed(with_reason(ended, first_line(&output.stderr))));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|name| !name.trim().is_empty())
        .map(str::to_string)
        .collect())
}

/// Shell lines for the tests, in sh.
#[cfg(test)]
pub mod lines {
    use std::path::Path;

    /// The exit code sh gives a program it cannot find.
    pub const NOT_FOUND: i32 = 127;

    /// A line that adds `word` and a line break to the end of `file`.
    pub fn append(word: &str, file: &Path) -> String {
        format!("echo {word} >> '{}'", file.display())
    }

    /// A line that writes `text` to stderr.
    pub fn to_stderr(text: &str) -> String {
        format!("echo {text} >&2")
    }

    /// A line that waits this many milliseconds.
    pub fn pause(milliseconds: u64) -> String {
        format!("sleep {}", milliseconds as f64 / 1000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::run_once;
    use crate::config::Does;

    /// A script runs with sh, so it needs no execute bit, and reports the way
    /// a command does.
    #[tokio::test]
    async fn a_script_file_runs_with_sh() {
        let folder = tempfile::tempdir().unwrap();
        let works = folder.path().join("works.sh");
        std::fs::write(&works, "true\n").unwrap();
        let fails = folder.path().join("fails.sh");
        std::fs::write(&fails, "echo the lamp is away >&2\nexit 4\n").unwrap();

        let run = |path: &std::path::Path| {
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
        let missing = run(&folder.path().join("missing.sh")).await;
        assert!(
            matches!(missing, Outcome::Exit { code: 127, .. }),
            "{missing:?}"
        );
    }

    /// A shortcut is done, or the error, which names the exit code. These
    /// run no shortcut: `shortcuts run` would run a real one.
    #[test]
    fn a_shortcut_is_done_or_the_error() {
        let ended = |code, complaint: Option<&str>| {
            Ok(Ended {
                code,
                complaint: complaint.map(str::to_string),
            })
        };
        assert_eq!(ran(ended(Some(0), Some("a warning"))), Outcome::Done);
        assert_eq!(
            ran(ended(Some(1), Some("Couldn't find shortcut"))),
            Outcome::Error("exit 1: Couldn't find shortcut".to_string())
        );
        assert_eq!(
            ran(ended(Some(1), None)),
            Outcome::Error("exit 1".to_string())
        );
        assert_eq!(
            ran(ended(None, None)),
            Outcome::Error("stopped by a signal".to_string())
        );
        assert_eq!(
            ran(Err(
                "cannot run it: No such file or directory (os error 2)".to_string()
            )),
            Outcome::Error("cannot run it: No such file or directory (os error 2)".to_string())
        );
    }

    // --- the shortcuts to pick from --------------------------------------------

    fn sh(script: &str) -> Command {
        let mut command = Command::new("sh");
        command.arg("-c").arg(script);
        command
    }

    /// One name a line, as `shortcuts list` prints them, blank lines left
    /// out. A name is kept exactly, because `shortcuts run` needs it so.
    #[tokio::test]
    async fn the_shortcuts_are_the_lines_the_list_prints() {
        let listed = names_listed(
            sh("printf 'Start Focus\\n\\nToggle the lamp \\nTurn on the desk lights\\n'"),
            LIST_LIMIT,
        )
        .await;
        assert_eq!(
            listed,
            Ok(vec![
                "Start Focus".to_string(),
                "Toggle the lamp ".to_string(),
                "Turn on the desk lights".to_string(),
            ])
        );
        assert_eq!(names_listed(sh("true"), LIST_LIMIT).await, Ok(Vec::new()));
    }

    /// A list that could not be had says why, rather than being empty, which
    /// would read as a Mac with no shortcuts.
    #[tokio::test]
    async fn a_list_that_could_not_be_had_says_why() {
        let failed = names_listed(
            sh("echo 'The operation could not be completed' >&2; exit 1"),
            LIST_LIMIT,
        )
        .await;
        assert_eq!(
            failed,
            Err(
                "the shortcuts could not be listed: shortcuts list ended with exit 1: The \
                 operation could not be completed"
                    .to_string()
            )
        );

        let missing = names_listed(Command::new("no-such-shortcuts-tool"), LIST_LIMIT)
            .await
            .unwrap_err();
        assert!(
            missing.starts_with("the shortcuts could not be listed: cannot run shortcuts: "),
            "{missing}"
        );

        let hung = names_listed(sh("sleep 30"), Duration::from_millis(200))
            .await
            .unwrap_err();
        assert!(hung.contains("shortcuts did not answer"), "{hung}");
    }
}
