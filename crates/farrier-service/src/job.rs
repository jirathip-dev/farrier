use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// A job's one name: lowercase reverse-DNS, the plist label and the systemd unit name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Label(String);

impl Label {
    /// The label checked: dot-separated lowercase words of `a-z`, `0-9` and inner `-`, at least two.
    pub fn new(label: &str) -> Result<Self, Error> {
        let word = |w: &str| {
            !w.is_empty()
                && !w.starts_with('-')
                && !w.ends_with('-')
                && w.bytes()
                    .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'-'))
        };
        if label.split('.').count() < 2 || !label.split('.').all(word) {
            return Err(Error::Label(label.to_owned()));
        }
        Ok(Self(label.to_owned()))
    }

    /// The label as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Label {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A per-user launchd or systemd job; the same value renders on both.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Job {
    pub label: Label,
    pub argv: Vec<OsString>,
    pub schedule: Schedule,
    pub path: String,
    pub log: Log,
    pub launchd: Launchd,
    pub systemd: Systemd,
    pub supersedes: Vec<String>,
}

/// When the job runs: kept alive, every interval, or daily at a time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Schedule {
    KeepAlive(Restart),
    Every(Duration),
    Daily { hour: u8, minute: u8 },
}

/// When a kept-alive job is started again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Restart {
    Always,
    OnFailure,
}

/// Where both output streams go: the platform's default, or one file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Log {
    Default,
    File(PathBuf),
}

/// launchd's `ProcessType`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ProcessType {
    #[default]
    Standard,
    Interactive,
    Background,
}

/// The keys only launchd has.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Launchd {
    pub process_type: ProcessType,
    pub abandon_process_group: bool,
}

/// The keys only systemd has.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Systemd {
    pub oom_policy_continue: bool,
}

impl Job {
    /// A job running `argv` on `schedule`; refuses a bad label, a relative program path and an impossible time.
    pub fn new<A: AsRef<OsStr>>(
        label: &str,
        argv: impl IntoIterator<Item = A>,
        schedule: Schedule,
    ) -> Result<Self, Error> {
        let label = Label::new(label)?;
        let argv: Vec<OsString> = argv.into_iter().map(|a| a.as_ref().to_owned()).collect();
        match argv.first() {
            Some(program) if Path::new(program).is_absolute() => {}
            program => {
                let shown = program.map(|p| p.to_string_lossy().into_owned());
                return Err(Error::Program(shown.unwrap_or_default()));
            }
        }
        match schedule {
            Schedule::Every(every) if every.as_secs() == 0 => {
                return Err(Error::Time(format!("every {every:?}")));
            }
            Schedule::Daily { hour, minute } if hour > 23 || minute > 59 => {
                return Err(Error::Time(format!("daily at {hour}:{minute:02}")));
            }
            _ => {}
        }
        Ok(Self {
            label,
            argv,
            schedule,
            path: String::new(),
            log: Log::Default,
            launchd: Launchd::default(),
            systemd: Systemd::default(),
            supersedes: Vec::new(),
        })
    }

    /// The job's `PATH`; empty leaves the platform's.
    #[must_use]
    pub fn path(self, path: &str) -> Self {
        Self {
            path: path.to_owned(),
            ..self
        }
    }

    /// Where its output goes.
    #[must_use]
    pub fn log(self, log: Log) -> Self {
        Self { log, ..self }
    }

    /// Its launchd-only keys.
    #[must_use]
    pub fn launchd(self, launchd: Launchd) -> Self {
        Self { launchd, ..self }
    }

    /// Its systemd-only keys.
    #[must_use]
    pub fn systemd(self, systemd: Systemd) -> Self {
        Self { systemd, ..self }
    }

    /// The old labels or unit names `install` unloads and removes.
    #[must_use]
    pub fn supersedes<S: Into<String>>(self, names: impl IntoIterator<Item = S>) -> Self {
        let supersedes = names.into_iter().map(Into::into).collect();
        Self { supersedes, ..self }
    }
}

/// Why a job could not be built, written or controlled.
#[derive(Debug)]
pub enum Error {
    Label(String),
    Program(String),
    Time(String),
    Host(String),
    Io(PathBuf, io::Error),
    Ctl(String, String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Label(l) => write!(f, "{l:?} is not a lowercase reverse-DNS label"),
            Self::Program(p) => write!(f, "the program {p:?} is not an absolute path"),
            Self::Time(t) => write!(f, "{t} is not a time a job can run"),
            Self::Host(why) => write!(f, "this host: {why}"),
            Self::Io(path, e) => write!(f, "{}: {e}", path.display()),
            Self::Ctl(cmd, said) => write!(f, "{cmd} failed: {said}"),
        }
    }
}

impl std::error::Error for Error {}
