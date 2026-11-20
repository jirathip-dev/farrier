use std::fmt;
use std::io;
use std::process::ExitCode;

use crate::contract::Reject;
use crate::id::Build;

/// Every way a pass ends; its `Display` is the one line a tool prints.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Paused {
        since: u64,
    },
    Resumed,
    Current(Build),
    Waiting {
        tag: String,
        why: String,
    },
    Skipped {
        build: Build,
        why: String,
    },
    Rejected(Reject),
    Staged(Build),
    Deferred {
        build: Build,
        why: String,
    },
    Pending {
        build: Build,
        why: String,
    },
    Updated {
        from: Option<Build>,
        to: Build,
    },
    RolledBack {
        to: Build,
        why: String,
        serving: bool,
    },
    Would(Box<Outcome>),
}

impl Outcome {
    pub(crate) fn ok(&self) -> bool {
        match self {
            Self::Rejected(_) | Self::RolledBack { .. } | Self::Skipped { .. } => false,
            Self::Would(inner) => inner.ok(),
            _ => true,
        }
    }

    /// Nonzero for a rejected, rolled back or skipped build.
    pub fn exit(&self) -> ExitCode {
        if self.ok() {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        }
    }
}

impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Paused { since } => write!(f, "paused since {since}"),
            Self::Resumed => f.write_str("resumed"),
            Self::Current(build) => write!(f, "current: {build}"),
            Self::Waiting { tag, why } => write!(f, "waiting for {tag}: {why}"),
            Self::Skipped { build, why } => write!(f, "skipped {build}: {why}"),
            Self::Rejected(reject) => write!(f, "rejected: {reject}"),
            Self::Staged(build) => write!(f, "staged {build}"),
            Self::Deferred { build, why } => write!(f, "deferred {build}: {why}"),
            Self::Pending { build, why } => write!(f, "pending {build}: {why}"),
            Self::Updated {
                from: Some(from),
                to,
            } => write!(f, "updated {from} -> {to}"),
            Self::Updated { from: None, to } => write!(f, "updated a dev build -> {to}"),
            Self::RolledBack { to, why, serving } => {
                let now = if *serving { "serves" } else { "does not serve" };
                write!(f, "rolled back {to}: {why}; the previous build {now}")
            }
            Self::Would(inner) => write!(f, "would: {inner}"),
        }
    }
}

/// The live build and the staged one, each when known.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Builds {
    pub live: Option<Build>,
    pub staged: Option<Build>,
}

/// A pass that could not run: the state folder, the lock, a missing tool, a refused swap, a bad reply.
#[derive(Debug)]
pub enum Error {
    State(io::Error),
    Busy,
    Tool(String),
    Refused(String),
    Reply(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::State(e) => write!(f, "the update state: {e}"),
            Self::Busy => f.write_str("another update pass holds the lock"),
            Self::Tool(why) | Self::Refused(why) => f.write_str(why),
            Self::Reply(why) => write!(f, "phase reply: {why}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::State(e)
    }
}
