#![doc = include_str!("../../../skill/farrier/service.md")]
#![allow(
    clippy::missing_errors_doc,
    reason = "a public item gets one doc line; every error is an `Error` variant"
)]

mod ctl;
mod env;
mod job;
mod render;
mod service;

pub use env::{Env, Platform};
pub use job::{Error, Job, Label, Launchd, Log, ProcessType, Restart, Schedule, Systemd};
pub use service::{Commands, Files, Installed, Removed, Service, Status};
