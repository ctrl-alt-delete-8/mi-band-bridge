/// One instruction typed on stdin while `listen` runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlInput {
    Connect,
    Disconnect,
    Status,
    Quit,
}

/// Parse one line of stdin. An unknown or blank line is ignored, not an error.
pub fn parse_line(line: &str) -> Option<ControlInput> {
    match line.trim().to_ascii_lowercase().as_str() {
        "c" | "connect" => Some(ControlInput::Connect),
        "d" | "disconnect" => Some(ControlInput::Disconnect),
        "s" | "status" => Some(ControlInput::Status),
        "q" | "quit" => Some(ControlInput::Quit),
        _ => None,
    }
}

/// The help banner printed once when `listen` starts.
pub const CONTROL_HELP: &str = "\
Controls (type and press Return):
  c, connect     connect to the band
  d, disconnect  disconnect and stay disconnected
  s, status      print the current state
  q, quit        release the band and exit";

use crate::supervisor::Command;
use tokio::sync::mpsc::Sender;

/// Read stdin lines and turn them into commands.
///
/// This runs on a plain OS thread, NOT a tokio blocking task. A blocking read on
/// stdin cannot be canceled, and tokio waits for its blocking pool at shutdown,
/// so using tokio::io::stdin here makes the process hang after `q` until the
/// user presses Return again. A detached thread is killed on process exit.
///
/// Closed stdin is not an error. A piped or backgrounded run simply has no
/// control input, and the supervisor keeps running on its configured default.
pub fn spawn_stdin_reader(tx: Sender<Command>) {
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut line = String::new();
        loop {
            line.clear();
            match stdin.read_line(&mut line) {
                Ok(0) | Err(_) => return,
                Ok(_) => {}
            }
            let Some(input) = parse_line(&line) else {
                if !line.trim().is_empty() {
                    println!("{CONTROL_HELP}");
                }
                continue;
            };
            let command = match input {
                ControlInput::Connect => Command::Connect,
                ControlInput::Disconnect => Command::Disconnect,
                ControlInput::Quit => Command::Shutdown,
                ControlInput::Status => Command::ReportState,
            };
            if tx.blocking_send(command).is_err() {
                return;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_short_and_long_forms() {
        let cases = [
            ("c", ControlInput::Connect),
            ("connect", ControlInput::Connect),
            ("d", ControlInput::Disconnect),
            ("disconnect", ControlInput::Disconnect),
            ("s", ControlInput::Status),
            ("status", ControlInput::Status),
            ("q", ControlInput::Quit),
            ("quit", ControlInput::Quit),
        ];
        for (input, expected) in cases {
            assert_eq!(parse_line(input), Some(expected), "failed on {input:?}");
        }
    }

    #[test]
    fn ignores_case_and_surrounding_space() {
        assert_eq!(parse_line("  DiSconnect \t"), Some(ControlInput::Disconnect));
        assert_eq!(parse_line("\tC\n"), Some(ControlInput::Connect));
    }

    #[test]
    fn returns_none_for_an_unknown_or_blank_line() {
        assert_eq!(parse_line(""), None);
        assert_eq!(parse_line("   "), None);
        assert_eq!(parse_line("reconnect"), None);
        assert_eq!(parse_line("dd"), None);
    }
}
