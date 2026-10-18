//! Octopus Chat core.
//!
//! Everything that can be reasoned about without a window lives here: the
//! SQLite store, the provider layer, tools, permissions and the agent loop.
//! The Tauri crate on top is a thin command/event shell, which keeps this
//! library testable and lets the UI be rewritten without touching the agent.

pub mod agent;
pub mod db;
pub mod error;
pub mod fsview;
pub mod git;
pub mod models;
pub mod paths;
pub mod permissions;
pub mod providers;
pub mod settings;
pub mod tools;
pub mod util;

pub use error::{Error, Result};
