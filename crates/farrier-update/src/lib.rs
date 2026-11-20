#![doc = include_str!("../../../skill/farrier/update.md")]
#![expect(
    clippy::missing_errors_doc,
    reason = "one /// line per pub item (.drover/rules.md); each Err type names the failure"
)]

pub mod contract;
pub mod id;
mod stage;
mod state;
mod swap;
pub mod tool;
mod updater;

pub use contract::Reject;
pub use id::{Arch, Build, Channel, Os, Target};
pub use stage::{Limits, Product, Staged};
pub use state::{Resting, State};
pub use updater::{
    Builds, Daemon, Error, Install, NoDaemon, Outcome, Restarted, Supervised, Updater, fetch, phase,
};
