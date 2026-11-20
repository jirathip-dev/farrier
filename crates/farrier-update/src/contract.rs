use std::fmt;

use minisign_verify::{PublicKey, Signature};
use sha2::{Digest, Sha256};

use crate::id::Build;

/// The checksum list of a release.
pub const SUMS: &str = "SHA256SUMS";
/// The minisign signature of [`SUMS`].
pub const SUMS_SIG: &str = "SHA256SUMS.minisig";

/// Why a release is not installed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reject {
    NoAsset(String),
    Download(String),
    Signature(String),
    Checksum(String),
    CodeSign(String),
    Version(String),
}

impl Reject {
    /// True only for a failure the next pass may not see again: a download.
    #[must_use]
    pub fn retry(&self) -> bool {
        matches!(self, Self::Download(_))
    }

    /// The variant's name and its reason.
    #[must_use]
    pub fn parts(&self) -> (&'static str, &str) {
        match self {
            Self::NoAsset(why) => ("NoAsset", why),
            Self::Download(why) => ("Download", why),
            Self::Signature(why) => ("Signature", why),
            Self::Checksum(why) => ("Checksum", why),
            Self::CodeSign(why) => ("CodeSign", why),
            Self::Version(why) => ("Version", why),
        }
    }

    /// The reject [`Reject::parts`] names.
    #[must_use]
    pub fn from_parts(kind: &str, why: String) -> Option<Self> {
        let make = match kind {
            "NoAsset" => Self::NoAsset,
            "Download" => Self::Download,
            "Signature" => Self::Signature,
            "Checksum" => Self::Checksum,
            "CodeSign" => Self::CodeSign,
            "Version" => Self::Version,
            _ => return None,
        };
        Some(make(why))
    }
}

impl fmt::Display for Reject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (kind, why) = self.parts();
        write!(f, "{kind}: {why}")
    }
}

/// The release a signature must name: `<owner>/<repo> <tag> <sha40>`.
pub struct Release<'a> {
    pub repo: &'a str,
    pub tag: &'a str,
    pub sha: &'a Build,
}

impl Release<'_> {
    /// The trusted comment the release's signature carries.
    #[must_use]
    pub fn comment(&self) -> String {
        format!("{} {} {}", self.repo, self.tag, self.sha)
    }
}

/// Checks `sig` over `sums` against `keys` (minisign public key files), then its trusted comment against `release`.
pub fn verify_sums(sums: &[u8], sig: &str, keys: &[&str], release: &Release) -> Result<(), Reject> {
    let sig = Signature::decode(sig)
        .map_err(|e| Reject::Signature(format!("{SUMS_SIG} is no minisign signature: {e}")))?;
    let signed = keys.iter().any(|key| {
        PublicKey::decode(key.trim()).is_ok_and(|key| key.verify(sums, &sig, false).is_ok())
    });
    if !signed {
        return Err(Reject::Signature(format!(
            "{SUMS} is not signed by a trusted key"
        )));
    }
    let want = release.comment();
    if sig.trusted_comment() == want {
        Ok(())
    } else {
        Err(Reject::Signature(format!(
            "signed for '{}', not '{want}'",
            sig.trusted_comment()
        )))
    }
}

/// A parsed `SHA256SUMS`: each asset name with its SHA-256 in lowercase hex.
#[derive(Debug, PartialEq, Eq)]
pub struct Sums(Vec<(String, String)>);

impl Sums {
    /// Parses `SHA256SUMS`: lines of 64 lowercase hex, two spaces and a name, each ending in a newline.
    pub fn parse(text: &str) -> Result<Self, Reject> {
        let body = text.strip_suffix('\n').ok_or_else(|| {
            Reject::Checksum(format!("{SUMS} is empty or lacks its last newline"))
        })?;
        let mut lines: Vec<(String, String)> = Vec::new();
        for (n, line) in body.split('\n').enumerate() {
            let bad = || Reject::Checksum(format!("{SUMS} line {}: '{line}'", n + 1));
            let (hex, name) = line.split_once("  ").ok_or_else(bad)?;
            let hex_ok = hex.len() == 64
                && hex
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
            let name_ok = !name.is_empty()
                && name != "."
                && name != ".."
                && !name.starts_with('*')
                && !name.contains(|c: char| c.is_whitespace() || c == '/' || c == '\\');
            if !hex_ok || !name_ok || lines.iter().any(|(listed, _)| listed == name) {
                return Err(bad());
            }
            lines.push((name.to_owned(), hex.to_owned()));
        }
        Ok(Self(lines))
    }

    /// The listed asset names, in file order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(|(name, _)| name.as_str())
    }

    /// Checks `bytes` against the line for `name`.
    pub fn check(&self, name: &str, bytes: &[u8]) -> Result<(), Reject> {
        let (_, want) = self
            .0
            .iter()
            .find(|(listed, _)| listed == name)
            .ok_or_else(|| Reject::NoAsset(format!("{name} is not in {SUMS}")))?;
        let got: String = Sha256::digest(bytes)
            .iter()
            .flat_map(|b| [b >> 4, b & 0xf])
            .filter_map(|n| char::from_digit(u32::from(n), 16))
            .collect();
        if &got == want {
            Ok(())
        } else {
            Err(Reject::Checksum(format!(
                "{name} has SHA-256 {got}, {SUMS} lists {want}"
            )))
        }
    }
}

/// The build a `--version` output names: its last line ends with `(<hex>)`.
#[must_use]
pub fn version(out: &str) -> Option<Build> {
    let (_, hex) = out
        .lines()
        .last()?
        .trim_end()
        .strip_suffix(')')?
        .rsplit_once('(')?;
    Build::parse(hex)
}

/// Checks that a `--version` output names `build`.
pub fn names(out: &str, build: &Build) -> Result<(), Reject> {
    match version(out) {
        Some(named) if named.matches(build) => Ok(()),
        _ => Err(Reject::Version(format!(
            "--version names no build {build}: '{}'",
            out.trim()
        ))),
    }
}
