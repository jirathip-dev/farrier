use super::{Builds, Error, Outcome, Updater};
use crate::contract::Reject;
use crate::id::Build;
use crate::state::{Fields, object, opt, quote};

/// A step a rollout asks one host for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Builds,
    Stage,
    Swap,
}

/// What a host answers a phase: how it ended, if it ran one, and its builds after it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reply {
    pub outcome: Option<Outcome>,
    pub builds: Builds,
}

fn encode(outcome: &Outcome) -> Vec<(&'static str, String)> {
    let b = |build: &Build| quote(build.as_str());
    let (kind, mut fields) = match outcome {
        Outcome::Paused { since } => ("paused", vec![("since", since.to_string())]),
        Outcome::Resumed => ("resumed", vec![]),
        Outcome::Current(build) => ("current", vec![("build", b(build))]),
        Outcome::Waiting { tag, why } => {
            ("waiting", vec![("tag", quote(tag)), ("why", quote(why))])
        }
        Outcome::Skipped { build, why } => {
            ("skipped", vec![("build", b(build)), ("why", quote(why))])
        }
        Outcome::Rejected(reject) => {
            let (kind, why) = reject.parts();
            (
                "rejected",
                vec![("reject", quote(kind)), ("why", quote(why))],
            )
        }
        Outcome::Staged(build) => ("staged", vec![("build", b(build))]),
        Outcome::Deferred { build, why } => {
            ("deferred", vec![("build", b(build)), ("why", quote(why))])
        }
        Outcome::Pending { build, why } => {
            ("pending", vec![("build", b(build)), ("why", quote(why))])
        }
        Outcome::Updated { from, to } => (
            "updated",
            vec![("from", opt(from.as_ref())), ("build", b(to))],
        ),
        Outcome::RolledBack { to, why, serving } => (
            "rolled-back",
            vec![
                ("build", b(to)),
                ("why", quote(why)),
                ("serving", serving.to_string()),
            ],
        ),
        Outcome::Would(inner) => {
            let mut fields = encode(inner);
            fields.push(("would", "true".to_owned()));
            return fields;
        }
    };
    fields.insert(0, ("outcome", quote(kind)));
    fields
}

fn decode(f: &Fields, kind: &str) -> Option<Outcome> {
    let build = || f.build("build");
    let why = || f.get("why").map(str::to_owned);
    let outcome = match kind {
        "paused" => Outcome::Paused {
            since: f.num("since")?,
        },
        "resumed" => Outcome::Resumed,
        "current" => Outcome::Current(build()?),
        "waiting" => Outcome::Waiting {
            tag: f.get("tag")?.to_owned(),
            why: why()?,
        },
        "skipped" => Outcome::Skipped {
            build: build()?,
            why: why()?,
        },
        "rejected" => Outcome::Rejected(Reject::from_parts(f.get("reject")?, why()?)?),
        "staged" => Outcome::Staged(build()?),
        "deferred" => Outcome::Deferred {
            build: build()?,
            why: why()?,
        },
        "pending" => Outcome::Pending {
            build: build()?,
            why: why()?,
        },
        "updated" => Outcome::Updated {
            from: f.build("from"),
            to: build()?,
        },
        "rolled-back" => Outcome::RolledBack {
            to: build()?,
            why: why()?,
            serving: f.get("serving")? == "true",
        },
        _ => return None,
    };
    Some(if f.get("would") == Some("true") {
        Outcome::Would(Box::new(outcome))
    } else {
        outcome
    })
}

/// Runs `phase` for `build` and gives the one JSON line a consumer's `--phase` verb prints.
pub fn run(updater: &Updater, phase: Phase, build: &Build) -> String {
    let done = match phase {
        Phase::Builds => Ok(None),
        Phase::Stage => updater.stage(build).map(Some),
        Phase::Swap => updater.swap(build).map(Some),
    };
    let builds = updater.builds();
    let mut fields = vec![
        ("live", opt(builds.live.as_ref())),
        ("staged", opt(builds.staged.as_ref())),
    ];
    match done {
        Ok(Some(outcome)) => fields.extend(encode(&outcome)),
        Ok(None) => {}
        Err(e) => fields.push(("error", quote(&e.to_string()))),
    }
    object(&fields)
}

/// Reads the line [`run`] printed on the other host; its error comes back as an `Error`.
pub fn parse(line: &str) -> Result<Reply, Error> {
    let bad = || Error::Reply(format!("not a phase reply: {line}"));
    let f = Fields::parse(line).ok_or_else(bad)?;
    if let Some(e) = f.get("error") {
        return Err(Error::Reply(e.to_owned()));
    }
    let outcome = match f.get("outcome") {
        Some(kind) => Some(decode(&f, kind).ok_or_else(bad)?),
        None => None,
    };
    let builds = Builds {
        live: f.build("live"),
        staged: f.build("staged"),
    };
    Ok(Reply { outcome, builds })
}
