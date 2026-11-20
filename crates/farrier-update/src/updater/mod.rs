use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::contract::{self, Reject};
use crate::id::{Build, Channel, Target};
use crate::stage::{Fetch, Got, Limits, Product, Staged, Stop};
use crate::state::{self, Resting, State};
use crate::swap;
use crate::tool::{self, Fail};

mod daemon;
mod outcome;
pub mod phase;

pub use daemon::{Daemon, NoDaemon, Restarted, Supervised};
pub use outcome::{Builds, Error, Outcome};

/// The live executable, and the folder of `state.json`, `lock` and `paused`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Install {
    pub exe: PathBuf,
    pub state: PathBuf,
}

fn now() -> u64 {
    let since = SystemTime::now().duration_since(UNIX_EPOCH);
    since.map_or(0, |d| d.as_secs())
}

/// Verifies `build`'s release for `target` and leaves its executable at `to`, as a host with no binary needs.
pub fn fetch(
    product: &Product,
    channel: &Channel,
    build: &Build,
    target: Target,
    to: &Path,
) -> Result<Staged, Reject> {
    let limits = Limits::default();
    let fetch = Fetch {
        product,
        channel,
        build,
        target,
        limit: limits.download,
    };
    match fetch.run(to, limits.probe) {
        Ok(Got::Staged(staged)) => Ok(staged),
        Ok(Got::Waiting(why)) => Err(Reject::NoAsset(why)),
        Err(Stop::Rejected(reject)) => Err(reject),
        Err(Stop::Missing(why)) => Err(Reject::Download(why)),
    }
}

/// One host's update of one product: the statechart in the farrier ADR.
pub struct Updater<'a> {
    product: &'a Product,
    install: Install,
    channel: Channel,
    daemon: &'a dyn Daemon,
    limits: Limits,
}

impl<'a> Updater<'a> {
    /// An updater of `product` at `install`, following `channel`, with no daemon and the default limits.
    #[must_use]
    pub fn new(product: &'a Product, install: Install, channel: Channel) -> Self {
        Self {
            product,
            install,
            channel,
            daemon: &NoDaemon,
            limits: Limits::default(),
        }
    }

    /// Restarts and confirms through `daemon`.
    #[must_use]
    pub fn daemon(self, daemon: &'a dyn Daemon) -> Self {
        Self { daemon, ..self }
    }

    /// Replaces the default limits.
    #[must_use]
    pub fn limits(self, limits: Limits) -> Self {
        Self { limits, ..self }
    }

    fn dir(&self) -> &Path {
        &self.install.state
    }

    fn live(&self) -> Option<Build> {
        let out = tool::version(&self.install.exe, self.limits.probe).ok()?;
        contract::version(&out)
    }

    fn pass(
        &self,
        step: impl FnOnce(&mut State) -> Result<Outcome, Error>,
    ) -> Result<Outcome, Error> {
        let _lock = state::lock(self.dir())?.ok_or(Error::Busy)?;
        let mut st = state::load(self.dir())?;
        let outcome = step(&mut st)?;
        st.at = now();
        st.last = outcome.to_string();
        st.ok = outcome.ok();
        state::save(self.dir(), &st)?;
        Ok(outcome)
    }

    fn check(&self, st: &State) -> Result<Result<Build, Outcome>, Error> {
        let branch = self.channel.branch();
        let waiting = |why| Outcome::Waiting {
            tag: format!("the head of {branch}"),
            why,
        };
        let head = match tool::head(self.product.repo, branch) {
            Ok(Some(head)) => head,
            Ok(None) => return Ok(Err(waiting("no such branch".to_owned()))),
            Err(Fail::Missing(why)) => return Err(Error::Tool(why)),
            Err(Fail::Failed(why)) => return Ok(Err(waiting(why))),
        };
        Ok(if self.live().is_some_and(|live| live.matches(&head)) {
            Err(Outcome::Current(head))
        } else if st
            .failed
            .as_ref()
            .is_some_and(|failed| failed.matches(&head))
        {
            Err(Outcome::Skipped {
                build: head,
                why: "it failed here before; waiting for the next head".to_owned(),
            })
        } else {
            Ok(head)
        })
    }

    fn staged(&self, st: &mut State, to: &Build) -> Result<Result<Staged, Outcome>, Error> {
        let path = swap::new(&self.install.exe);
        if matches!(&st.resting, Resting::Staged(b) if b.matches(to)) && path.is_file() {
            let build = to.clone();
            return Ok(Ok(Staged { path, build }));
        }
        st.resting = Resting::Idle;
        let fetch = Fetch {
            product: self.product,
            channel: &self.channel,
            build: to,
            target: Target::host().ok_or(Error::Tool("no release target for this host".into()))?,
            limit: self.limits.download,
        };
        match fetch.run(&path, self.limits.probe) {
            Ok(Got::Staged(staged)) => {
                st.resting = Resting::Staged(staged.build.clone());
                Ok(Ok(staged))
            }
            Ok(Got::Waiting(why)) => Ok(Err(Outcome::Waiting {
                tag: self.channel.tag(to),
                why,
            })),
            Err(Stop::Missing(why)) => Err(Error::Tool(why)),
            Err(Stop::Rejected(reject)) => {
                if !reject.retry() && !matches!(reject, Reject::NoAsset(_)) {
                    st.failed = Some(to.clone());
                }
                Ok(Err(Outcome::Rejected(reject)))
            }
        }
    }

    fn swap_in(&self, st: &mut State, staged: &Staged) -> Result<Outcome, Error> {
        let to = staged.build.clone();
        if let Err(why) = self.daemon.admit(staged) {
            return Ok(Outcome::Deferred { build: to, why });
        }
        let from = self.live();
        swap::swap(&self.install.exe)?;
        st.resting = Resting::Swapped {
            from: from.clone(),
            to: to.clone(),
            at: now(),
        };
        state::save(self.dir(), st)?;
        self.restart(st, from, to)
    }

    fn confirm(&self, to: &Build) -> bool {
        let end = Instant::now() + self.limits.confirm;
        loop {
            if self.daemon.serves(to) {
                return true;
            }
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            tool::wait(left.min(Duration::from_secs(1)));
        }
    }

    fn restart(&self, st: &mut State, from: Option<Build>, to: Build) -> Result<Outcome, Error> {
        let why = match self.daemon.restart(&to) {
            Ok(Restarted::Later(why)) => return Ok(Outcome::Pending { build: to, why }),
            Ok(Restarted::Now) if self.confirm(&to) => {
                st.resting = Resting::Idle;
                return Ok(Outcome::Updated { from, to });
            }
            Ok(Restarted::Now) => {
                format!("it did not serve within {}s", self.limits.confirm.as_secs())
            }
            Err(why) => why,
        };
        swap::back(&self.install.exe)?;
        let serving = match self.daemon.restart(from.as_ref().unwrap_or(&to)) {
            Ok(Restarted::Now) => from.as_ref().is_none_or(|from| self.daemon.serves(from)),
            _ => false,
        };
        st.resting = Resting::Idle;
        st.failed = Some(to.clone());
        Ok(Outcome::RolledBack { to, why, serving })
    }

    /// The whole chart: check the head, stage it, swap it in, restart, confirm or roll back.
    pub fn run(&self) -> Result<Outcome, Error> {
        self.pass(|st| {
            if let Some(since) = state::paused(self.dir()) {
                return Ok(Outcome::Paused { since });
            }
            if let Resting::Swapped { from, to, .. } = st.resting.clone() {
                return self.restart(st, from, to);
            }
            let head = match self.check(st)? {
                Ok(head) => head,
                Err(done) => return Ok(done),
            };
            match self.staged(st, &head)? {
                Ok(staged) => self.swap_in(st, &staged),
                Err(done) => Ok(done),
            }
        })
    }

    /// What `run` would do, read-only: the head is checked, nothing is downloaded or written.
    pub fn plan(&self) -> Result<Outcome, Error> {
        let st = state::load(self.dir())?;
        let would = if let Some(since) = state::paused(self.dir()) {
            Outcome::Paused { since }
        } else if let Resting::Swapped { from, to, .. } = st.resting.clone() {
            Outcome::Updated { from, to }
        } else {
            match self.check(&st)? {
                Ok(to) => Outcome::Updated {
                    from: self.live(),
                    to,
                },
                Err(done) => done,
            }
        };
        Ok(Outcome::Would(Box::new(would)))
    }

    /// Downloads and verifies `to` into `<exe>.new`, and rests there.
    pub fn stage(&self, to: &Build) -> Result<Outcome, Error> {
        self.pass(|st| {
            if let Resting::Swapped { to: swapped, .. } = &st.resting {
                return Err(Error::Refused(format!(
                    "{swapped} is swapped in, not confirmed yet"
                )));
            }
            if self.live().is_some_and(|live| live.matches(to)) {
                return Ok(Outcome::Current(to.clone()));
            }
            Ok(match self.staged(st, to)? {
                Ok(staged) => Outcome::Staged(staged.build),
                Err(done) => done,
            })
        })
    }

    /// Swaps in the staged `to`, restarts, confirms or rolls back; refuses any other staged build.
    pub fn swap(&self, to: &Build) -> Result<Outcome, Error> {
        let path = swap::new(&self.install.exe);
        self.pass(|st| match st.resting.clone() {
            Resting::Swapped {
                from, to: swapped, ..
            } if swapped.matches(to) => self.restart(st, from, swapped),
            Resting::Staged(build) if build.matches(to) && path.is_file() => {
                self.swap_in(st, &Staged { path, build })
            }
            _ if self.live().is_some_and(|live| live.matches(to)) => {
                Ok(Outcome::Current(to.clone()))
            }
            _ => Err(Error::Refused(format!("{to} is not staged"))),
        })
    }

    /// The live build and the staged one.
    #[must_use]
    pub fn builds(&self) -> Builds {
        let staged = match self.status().resting {
            Resting::Staged(b) if swap::new(&self.install.exe).is_file() => Some(b),
            _ => None,
        };
        Builds {
            live: self.live(),
            staged,
        }
    }

    /// `state.json` as it is, or the first state.
    #[must_use]
    pub fn status(&self) -> State {
        state::load(self.dir()).unwrap_or_default()
    }

    /// Stops `run` until `resume`; `stage` and `swap` still work.
    pub fn pause(&self) -> Result<Outcome, Error> {
        let since = state::pause(self.dir(), now())?;
        Ok(Outcome::Paused { since })
    }

    /// Lets `run` work again.
    pub fn resume(&self) -> Result<Outcome, Error> {
        state::resume(self.dir())?;
        Ok(Outcome::Resumed)
    }
}
