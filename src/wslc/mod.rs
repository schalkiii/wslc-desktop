//! Framework-agnostic client for Microsoft's `wslc.exe` (WSL Containers).
//!
//! - [`client`]: process spawning with timeout + `CREATE_NO_WINDOW`.
//! - [`commands`]: typed command wrappers (list/stats/logs/lifecycle/run).
//! - [`types`]: serde structs matching the real `wslc --format json` schemas.

pub mod client;
pub mod commands;
pub mod types;

pub use client::{LogStream, WslcClient};
pub use commands::{LogOptions, RunSpec};
pub use types::{Container, ContainerState, Image, Network, Stat, Volume};
