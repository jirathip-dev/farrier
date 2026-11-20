use farrier_service::Service;

use crate::id::Build;
use crate::stage::Staged;

/// The consumer's part: may a build take over, put it in service, does it serve.
pub trait Daemon {
    fn admit(&self, _staged: &Staged) -> Result<(), String> {
        Ok(())
    }
    fn restart(&self, to: &Build) -> Result<Restarted, String>;
    fn serves(&self, to: &Build) -> bool;
}

/// Whether a restart took effect, or waits for a later pass and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Restarted {
    Now,
    Later(String),
}

/// A daemon that is a farrier [`Service`]: restart the job, then ask `probe`.
pub struct Supervised<'a, F> {
    service: &'a Service,
    probe: F,
}

impl<'a, F: Fn(&Build) -> bool> Supervised<'a, F> {
    /// Restarts through `service`; `probe` says whether a build serves.
    pub fn new(service: &'a Service, probe: F) -> Self {
        Self { service, probe }
    }
}

impl<F: Fn(&Build) -> bool> Daemon for Supervised<'_, F> {
    fn restart(&self, _: &Build) -> Result<Restarted, String> {
        let restarted = self.service.restart().map_err(|e| e.to_string());
        restarted.map(|()| Restarted::Now)
    }

    fn serves(&self, to: &Build) -> bool {
        (self.probe)(to)
    }
}

/// A plain CLI: nothing to restart, the new file is the update.
pub struct NoDaemon;

impl Daemon for NoDaemon {
    fn restart(&self, _: &Build) -> Result<Restarted, String> {
        Ok(Restarted::Now)
    }

    fn serves(&self, _: &Build) -> bool {
        true
    }
}
