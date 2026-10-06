//! Sending keys: CGEvent and NSEvent on macOS, SendInput on Windows.
//!
//! Both sides give the same names. [`crate::keys`] re-exports them, beside
//! the tables of names both systems share.
//!
//! - `fire` posts a checked keystroke, and `tap_key` and `tap_media` post
//!   one key by its code on this system.
//! - `trusted` says whether this process may post keys at all.
//! - `key_name`, `media_key_name`, `heard_key` and `heard_media_key` name a
//!   key the window's Record keys hears, by its code on this system.
//! - `unsupported_key`, `unsupported_media_key` and `unsupported_modifier`
//!   say why this system cannot send a name the file may hold, so the loader
//!   refuses it with a reason and the window leaves it out.
//! - `LABELS` holds the words the window shows for the modifiers, where the
//!   Mac's marks do not fit.
//! - `keys_only_here` names the keys this system sends that the shared table
//!   does not list, such as insert on Windows. The Mac has none.

#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "macos")]
pub use mac::{
    LABELS, fire, heard_key, heard_media_key, key_name, keys_only_here, media_key_name, tap_key,
    tap_media, trusted, unsupported_key, unsupported_media_key, unsupported_modifier,
};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::{
    LABELS, SENT_BY_BANDBRIDGE, fire, heard_key, heard_media_key, key_name, keys_only_here,
    media_key_name, tap_blank, tap_key, tap_media, trusted, unsupported_key, unsupported_media_key,
    unsupported_modifier,
};
