//! The band core.
//!
//! Everything that talks to the band lives here, so the command line and the
//! window are two ways into the same functions rather than two implementations.
//! Neither front end holds behavior of its own.

pub mod action;
pub mod app_names;
pub mod apps;
pub mod binding;
pub mod band_time;
pub mod ble;
pub mod config;
pub mod control;
pub mod crypto;
pub mod device;
pub mod error;
pub mod event;
pub mod flow;
pub mod install;
pub mod ipc;
pub mod keys;
pub mod menu_names;
pub mod pair;
mod platform;
pub mod protocol;
pub mod screens;
pub mod session;
pub mod shortcuts;
pub mod signal;
pub mod supervisor;
