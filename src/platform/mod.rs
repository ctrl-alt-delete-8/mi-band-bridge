//! What differs between macOS and Windows, one concern to a module (W1).
//!
//! Each concern has a macOS side and a Windows side, and both give the same
//! names: sending keys, running an action's command, and the link between
//! BandBridge's own processes. The rest of the crate calls only those names,
//! so a difference between the two systems lives here and nowhere else.
//!
//! Finding the band differs too. That difference is one function, so it stays
//! in [`crate::ble`].

#[cfg(not(any(target_os = "macos", windows)))]
compile_error!("BandBridge runs on macOS and Windows.");

pub mod actions;
pub mod keys;
pub mod link;

/// This computer, as a message to a person names it: "this Mac" or "this
/// PC". A macro, so a message that names it can still be a constant.
#[cfg(target_os = "macos")]
macro_rules! this_computer {
    () => {
        "this Mac"
    };
}
#[cfg(windows)]
macro_rules! this_computer {
    () => {
        "this PC"
    };
}
pub(crate) use this_computer;
