//! The band app editor: everything except the window.
//!
//! The window in `main.rs` only calls into these modules, so the logic is
//! tested here without a window.

pub mod about;
pub mod checks;
pub mod generate;
pub mod geometry;
pub mod images;
pub mod label;
pub mod install;
pub mod layout;
pub mod output;
pub mod signals;
pub mod zmake;
