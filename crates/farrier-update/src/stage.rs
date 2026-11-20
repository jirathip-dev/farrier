use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::contract::{self, Reject, Release, SUMS, SUMS_SIG, Sums};
use crate::id::{Build, Channel, Os, Target};
use crate::tool::{self, Fail};

/// What a consumer updates: its binary, its repo, the public keys and the code-signing identifier it trusts.
#[derive(Clone, Copy, Debug)]
pub struct Product {
    pub bin: &'static str,
    pub repo: &'static str,
    pub keys: &'static [&'static str],
    pub codesign_id: Option<&'static str>,
}

/// A verified executable at `path`, built from `build`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Staged {
    pub path: PathBuf,
    pub build: Build,
}

/// How long a download, the staged binary's `--version` and the new build's first answer may take.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub download: Duration,
    pub probe: Duration,
    pub confirm: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            download: Duration::from_secs(300),
            probe: Duration::from_secs(10),
            confirm: Duration::from_secs(60),
        }
    }
}

pub struct Fetch<'a> {
    pub product: &'a Product,
    pub channel: &'a Channel,
    pub build: &'a Build,
    pub target: Target,
    pub limit: Duration,
}

pub enum Got {
    Waiting(String),
    Staged(Staged),
}

pub enum Stop {
    Missing(String),
    Rejected(Reject),
}

impl From<Reject> for Stop {
    fn from(r: Reject) -> Self {
        Self::Rejected(r)
    }
}

impl Fetch<'_> {
    fn get(&self, tag: &str, name: &str) -> Result<Option<Vec<u8>>, Stop> {
        let url = format!(
            "https://github.com/{}/releases/download/{tag}/{name}",
            self.product.repo
        );
        tool::get(&url, self.limit).map_err(|e| match e {
            Fail::Missing(why) => Stop::Missing(why),
            Fail::Failed(why) => Stop::Rejected(Reject::Download(why)),
        })
    }

    /// Checks 2 to 6 of the release contract; leaves the verified executable at `to`, or nothing.
    pub fn run(&self, to: &Path, probe: Duration) -> Result<Got, Stop> {
        let tag = self.channel.tag(self.build);
        let (Some(sums), Some(sig)) = (self.get(&tag, SUMS)?, self.get(&tag, SUMS_SIG)?) else {
            return Ok(Got::Waiting(format!("{tag} has no {SUMS} yet")));
        };
        let name = self.target.asset(self.product.bin);
        let sums = self.sums(&tag, sums, &sig, &name)?;
        let bytes = self
            .get(&tag, &name)?
            .ok_or_else(|| Reject::Download(format!("{tag}/{name} is not there")))?;
        let staged = sums
            .check(&name, &bytes)
            .and_then(|()| self.place(to, &bytes, probe));
        if staged.is_err() {
            fs::remove_file(to).ok();
        }
        Ok(Got::Staged(staged?))
    }

    fn sums(&self, tag: &str, sums: Vec<u8>, sig: &[u8], name: &str) -> Result<Sums, Reject> {
        let release = Release {
            repo: self.product.repo,
            tag,
            sha: self.build,
        };
        let sig = String::from_utf8_lossy(sig);
        contract::verify_sums(&sums, &sig, self.product.keys, &release)?;
        let text = String::from_utf8(sums)
            .map_err(|_| Reject::Checksum(format!("{SUMS} is not UTF-8")))?;
        let sums = Sums::parse(&text)?;
        if sums.names().any(|n| n == name) {
            Ok(sums)
        } else {
            Err(Reject::NoAsset(format!("{tag} has no {name}")))
        }
    }

    fn place(&self, to: &Path, bytes: &[u8], probe: Duration) -> Result<Staged, Reject> {
        let io = |e: io::Error| Reject::Download(format!("{}: {e}", to.display()));
        fs::write(to, bytes).map_err(io)?;
        fs::set_permissions(to, fs::Permissions::from_mode(0o755)).map_err(io)?;
        if Target::host() == Some(self.target) {
            if let (Os::MacOs, Some(id)) = (self.target.os, self.product.codesign_id) {
                tool::codesign(to, id).map_err(Reject::CodeSign)?;
            }
            let out = tool::version(to, probe).map_err(Reject::Version)?;
            contract::names(&out, self.build)?;
        }
        Ok(Staged {
            path: to.to_path_buf(),
            build: self.build.clone(),
        })
    }
}
