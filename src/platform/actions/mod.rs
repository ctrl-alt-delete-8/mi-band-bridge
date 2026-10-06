//! Running an action's command: with sh and Shortcuts on macOS, and with
//! Windows PowerShell and the shell's open verb on Windows (W3 to W5).
//!
//! Both sides give the same names, and [`crate::action`] calls only these.
//!
//! - `shell`, `script` and `shortcut` run one action that takes time, and say
//!   how it went. The caller gives each a deadline.
//! - `shortcut_names` lists the shortcuts the window offers to pick from.
//! - `SHORTCUTS_REFUSED` is why the loader refuses a shortcut action, on a
//!   system that has no Shortcuts.
//! - `lines`, for the tests only, writes a few shell lines in this system's
//!   shell, so one test checks both systems.

#[cfg(target_os = "macos")]
mod mac;
#[cfg(all(target_os = "macos", test))]
pub use mac::lines;
#[cfg(target_os = "macos")]
pub use mac::{SHORTCUTS_REFUSED, script, shell, shortcut, shortcut_names};

#[cfg(windows)]
mod windows;
#[cfg(all(windows, test))]
pub use windows::lines;
#[cfg(windows)]
pub use windows::{SHORTCUTS_REFUSED, script, shell, shortcut, shortcut_names};
