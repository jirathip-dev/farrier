use std::ffi::OsString;
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::time::Duration;

use crate::job::Error;

/// Which job manager a host runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Launchd,
    Systemd,
}

/// The host as values: a test builds its own over a temp folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Env {
    pub platform: Platform,
    pub home: PathBuf,
    pub config_home: PathBuf,
    pub uid: u32,
    pub tools: OsString,
    pub backoff: Duration,
}

impl Env {
    /// This host: its platform, `HOME`, `XDG_CONFIG_HOME`, the home folder's owner and `PATH`.
    pub fn host() -> Result<Self, Error> {
        let platform = if cfg!(target_os = "macos") {
            Platform::Launchd
        } else if cfg!(target_os = "linux") {
            Platform::Systemd
        } else {
            return Err(Error::Host("neither launchd nor systemd".to_owned()));
        };
        let absolute = |key| {
            std::env::var_os(key)
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
        };
        let home = absolute("HOME").ok_or_else(|| Error::Host("HOME is not set".to_owned()))?;
        let uid = std::fs::metadata(&home)
            .map_err(|e| Error::Io(home.clone(), e))?
            .uid();
        Ok(Self {
            platform,
            config_home: absolute("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config")),
            home,
            uid,
            tools: std::env::var_os("PATH").unwrap_or_else(|| "/usr/bin:/bin".into()),
            backoff: Duration::from_secs(1),
        })
    }
}
