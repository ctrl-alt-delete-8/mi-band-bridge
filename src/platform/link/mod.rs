//! The link between BandBridge's own processes: a Unix socket on macOS and a
//! named pipe on Windows, carrying the same newline-framed JSON (W6).
//!
//! Both sides give the same names. [`crate::ipc`] speaks the protocol over
//! whichever this system has.
//!
//! - `listen` opens the link for the process that holds the band, and
//!   `Listener::accept` waits for the next caller on it.
//! - `connect` reaches the holder, or gives None when nothing holds the band.
//! - `CALLED` is the word a message uses for the link.

#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "macos")]
pub use mac::{CALLED, Listener, connect, listen};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::{CALLED, Listener, connect, listen};
