use std::fmt;

/// A commit a binary was built from: 7 to 40 lowercase hex characters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Build(String);

impl Build {
    /// The build named by `hex`, if it is 7 to 40 lowercase hex characters.
    #[must_use]
    pub fn parse(hex: &str) -> Option<Self> {
        let ok = (7..=40).contains(&hex.len())
            && hex
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        ok.then(|| Self(hex.to_owned()))
    }

    /// True when one build is a prefix of the other.
    #[must_use]
    pub fn matches(&self, other: &Self) -> bool {
        self.0.starts_with(&other.0) || other.0.starts_with(&self.0)
    }

    /// The first 7 characters, as tags name a build.
    #[must_use]
    pub fn short(&self) -> &str {
        self.0.get(..7).unwrap_or(&self.0)
    }

    /// The hex text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Build {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The branch a release was cut from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Channel {
    Main,
    Branch(String),
}

impl Channel {
    /// The release tag of `build` on this channel.
    #[must_use]
    pub fn tag(&self, build: &Build) -> String {
        match self {
            Self::Main => format!("build-{}", build.short()),
            Self::Branch(branch) => format!("build-{branch}-{}", build.short()),
        }
    }

    /// The channel whose tag of `build` is `tag`, if any.
    #[must_use]
    pub fn of_tag(tag: &str, build: &Build) -> Option<Self> {
        let branch = tag
            .strip_prefix("build-")?
            .strip_suffix(build.short())?
            .strip_suffix('-');
        let channel = branch.map_or(Self::Main, |b| Self::Branch(b.to_owned()));
        (channel.tag(build) == tag && channel != Self::Branch(String::new())).then_some(channel)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    MacOs,
    Linux,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arch {
    Aarch64,
    X86_64,
}

/// A platform a release has an executable for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    pub os: Os,
    pub arch: Arch,
}

impl Target {
    /// Every target a release may hold.
    pub const ALL: [Self; 4] = [
        Self::new(Os::MacOs, Arch::Aarch64),
        Self::new(Os::MacOs, Arch::X86_64),
        Self::new(Os::Linux, Arch::X86_64),
        Self::new(Os::Linux, Arch::Aarch64),
    ];

    #[must_use]
    pub const fn new(os: Os, arch: Arch) -> Self {
        Self { os, arch }
    }

    /// The target this binary was built for, if releases have one.
    #[must_use]
    pub fn host() -> Option<Self> {
        let os = if cfg!(target_os = "macos") {
            Os::MacOs
        } else if cfg!(target_os = "linux") {
            Os::Linux
        } else {
            return None;
        };
        let arch = if cfg!(target_arch = "aarch64") {
            Arch::Aarch64
        } else if cfg!(target_arch = "x86_64") {
            Arch::X86_64
        } else {
            return None;
        };
        Some(Self::new(os, arch))
    }

    /// The Rust target triple.
    #[must_use]
    pub fn triple(self) -> &'static str {
        match (self.os, self.arch) {
            (Os::MacOs, Arch::Aarch64) => "aarch64-apple-darwin",
            (Os::MacOs, Arch::X86_64) => "x86_64-apple-darwin",
            (Os::Linux, Arch::X86_64) => "x86_64-unknown-linux-gnu",
            (Os::Linux, Arch::Aarch64) => "aarch64-unknown-linux-gnu",
        }
    }

    /// The release asset of `bin` for this target: `<bin>-<triple>`.
    #[must_use]
    pub fn asset(self, bin: &str) -> String {
        format!("{bin}-{}", self.triple())
    }
}
