#![expect(
    clippy::missing_errors_doc,
    reason = "one /// line per pub item (.drover/rules.md); each Err type names the failure"
)]

pub mod contract;
pub mod id;
pub mod tool;
