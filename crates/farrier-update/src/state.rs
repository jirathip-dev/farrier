use std::fs::{self, File, TryLockError};
use std::io::{self, ErrorKind};
use std::iter::Peekable;
use std::path::Path;
use std::str::Chars;

use crate::id::Build;

/// `state.json`: when the last pass ended, where it rests, the build that failed, its line and how it went.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct State {
    pub at: u64,
    pub resting: Resting,
    pub failed: Option<Build>,
    pub last: String,
    pub ok: bool,
}

/// Where a host rests between passes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Resting {
    #[default]
    Idle,
    Staged(Build),
    Swapped {
        from: Option<Build>,
        to: Build,
        at: u64,
    },
}

pub fn quote(text: &str) -> String {
    let mut out = String::from('"');
    for ch in text.chars() {
        match ch {
            '"' | '\\' => {
                out.push('\\');
                out.push(ch);
            }
            c if c.is_control() => {
                out.push_str("\\u00");
                let n = u32::from(c);
                out.extend(
                    [n >> 4, n & 15]
                        .into_iter()
                        .filter_map(|d| char::from_digit(d, 16)),
                );
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

pub fn opt(build: Option<&Build>) -> String {
    build.map_or_else(|| "null".to_owned(), |b| quote(b.as_str()))
}

pub fn object(fields: &[(&str, String)]) -> String {
    let fields: Vec<String> = fields
        .iter()
        .map(|(k, v)| format!("{}:{v}", quote(k)))
        .collect();
    format!("{{{}}}", fields.join(","))
}

/// A flat JSON object read back: each value a string, a number or boolean as its text, or `None` for null.
pub struct Fields(Vec<(String, Option<String>)>);

fn string(c: &mut Peekable<Chars>) -> Option<String> {
    (c.next()? == '"').then_some(())?;
    let mut out = String::new();
    loop {
        match c.next()? {
            '"' => return Some(out),
            '\\' => out.push(match c.next()? {
                'u' => {
                    char::from_u32(u32::from_str_radix(&c.take(4).collect::<String>(), 16).ok()?)?
                }
                ch @ ('"' | '\\' | '/') => ch,
                _ => return None,
            }),
            ch => out.push(ch),
        }
    }
}

fn literal(c: &mut Peekable<Chars>) -> Option<String> {
    let mut lit = String::new();
    while let Some(ch) = c.next_if(|ch| !matches!(ch, ',' | '}') && !ch.is_whitespace()) {
        lit.push(ch);
    }
    let ok = matches!(lit.as_str(), "null" | "true" | "false") || lit.parse::<u64>().is_ok();
    ok.then_some(lit)
}

impl Fields {
    pub fn parse(text: &str) -> Option<Self> {
        let mut c = text.trim().chars().peekable();
        let ws = |c: &mut Peekable<Chars>| while c.next_if(|ch| ch.is_whitespace()).is_some() {};
        (c.next()? == '{').then_some(())?;
        ws(&mut c);
        let mut out = Vec::new();
        if c.next_if_eq(&'}').is_none() {
            loop {
                ws(&mut c);
                let key = string(&mut c)?;
                ws(&mut c);
                (c.next()? == ':').then_some(())?;
                ws(&mut c);
                let value = if c.peek() == Some(&'"') {
                    Some(string(&mut c)?)
                } else {
                    Some(literal(&mut c)?).filter(|lit| lit != "null")
                };
                out.push((key, value));
                ws(&mut c);
                match c.next()? {
                    ',' => {}
                    '}' => break,
                    _ => return None,
                }
            }
        }
        c.next().is_none().then_some(Self(out))
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.iter().find(|(k, _)| k == key)?.1.as_deref()
    }

    pub fn num(&self, key: &str) -> Option<u64> {
        self.get(key)?.parse().ok()
    }

    pub fn build(&self, key: &str) -> Option<Build> {
        Build::parse(self.get(key)?)
    }
}

impl State {
    fn json(&self) -> String {
        let (resting, build, from, since) = match &self.resting {
            Resting::Idle => ("idle", None, None, 0),
            Resting::Staged(b) => ("staged", Some(b), None, 0),
            Resting::Swapped { from, to, at } => ("swapped", Some(to), from.as_ref(), *at),
        };
        object(&[
            ("at", self.at.to_string()),
            ("resting", quote(resting)),
            ("build", opt(build)),
            ("from", opt(from)),
            ("swapped_at", since.to_string()),
            ("failed", opt(self.failed.as_ref())),
            ("last", quote(&self.last)),
            ("ok", self.ok.to_string()),
        ])
    }

    fn parse(text: &str) -> Option<Self> {
        let f = Fields::parse(text)?;
        let resting = match f.get("resting")? {
            "idle" => Resting::Idle,
            "staged" => Resting::Staged(f.build("build")?),
            "swapped" => Resting::Swapped {
                from: f.build("from"),
                to: f.build("build")?,
                at: f.num("swapped_at")?,
            },
            _ => return None,
        };
        Some(Self {
            at: f.num("at")?,
            resting,
            failed: f.build("failed"),
            last: f.get("last")?.to_owned(),
            ok: f.get("ok")? == "true",
        })
    }
}

/// The state in `dir`, or the first state when there is none yet.
pub fn load(dir: &Path) -> io::Result<State> {
    match fs::read_to_string(dir.join("state.json")) {
        Ok(text) => State::parse(&text).ok_or_else(|| {
            io::Error::new(ErrorKind::InvalidData, "state.json: not a farrier state")
        }),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(State::default()),
        Err(e) => Err(e),
    }
}

/// Writes `state` to `dir`, whole or not at all.
pub fn save(dir: &Path, state: &State) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let tmp = dir.join("state.json.tmp");
    fs::write(&tmp, state.json() + "\n")?;
    fs::rename(tmp, dir.join("state.json"))
}

/// The pass lock in `dir`, or `None` while another pass holds it.
pub fn lock(dir: &Path) -> io::Result<Option<File>> {
    fs::create_dir_all(dir)?;
    let file = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("lock"))?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(e)) => Err(e),
    }
}

/// Since when `run` is paused, if it is.
pub fn paused(dir: &Path) -> Option<u64> {
    fs::read_to_string(dir.join("paused"))
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// Pauses `run` from `now`, or keeps the pause already there.
pub fn pause(dir: &Path, now: u64) -> io::Result<u64> {
    if let Some(since) = paused(dir) {
        return Ok(since);
    }
    fs::create_dir_all(dir)?;
    fs::write(dir.join("paused"), format!("{now}\n"))?;
    Ok(now)
}

/// Lifts a pause; nothing when there is none.
pub fn resume(dir: &Path) -> io::Result<()> {
    match fs::remove_file(dir.join("paused")) {
        Err(e) if e.kind() != ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}
