use std::fmt;
use std::io::{ErrorKind, Read};
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crate::id::Build;

/// Why a tool gave no answer: it is not installed, or it failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fail {
    Missing(String),
    Failed(String),
}

impl fmt::Display for Fail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing(why) | Self::Failed(why) => f.write_str(why),
        }
    }
}

fn output(cmd: &mut Command, what: &str) -> Result<Output, Fail> {
    cmd.stdin(Stdio::null())
        .output()
        .map_err(|e| match e.kind() {
            ErrorKind::NotFound => Fail::Missing(format!("{what}: not installed")),
            _ => Fail::Failed(format!("{what}: {e}")),
        })
}

fn run(cmd: &mut Command, what: &str) -> Result<Output, Fail> {
    let out = output(cmd, what)?;
    if out.status.success() {
        Ok(out)
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(Fail::Failed(format!(
            "{what}: {}: {}",
            out.status,
            err.trim()
        )))
    }
}

/// The commit `refs/heads/<branch>` of `https://github.com/<repo>` names, by `git ls-remote`.
pub fn head(repo: &str, branch: &str) -> Result<Option<Build>, Fail> {
    let what = format!("git ls-remote {repo} {branch}");
    let url = format!("https://github.com/{repo}");
    let refname = format!("refs/heads/{branch}");
    let out = run(
        Command::new("git").args(["ls-remote", &url, &refname]),
        &what,
    )?;
    let text = String::from_utf8_lossy(&out.stdout);
    Ok(text.split_whitespace().next().and_then(Build::parse))
}

/// The body at `url` by `curl`, or `None` when the server says it is not there.
pub fn get(url: &str, limit: Duration) -> Result<Option<Vec<u8>>, Fail> {
    let secs = limit.as_secs().max(1).to_string();
    let what = format!("curl {url}");
    let out = output(
        Command::new("curl").args(["-fsSL", "--max-time", &secs, url]),
        &what,
    )?;
    match out.status.code() {
        Some(0) => Ok(Some(out.stdout)),
        Some(22) => Ok(None),
        _ => Err(Fail::Failed(format!(
            "{what}: {}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ))),
    }
}

/// The stdout of `<exe> --version`, killed when it has not answered within `limit`.
pub fn version(exe: &Path, limit: Duration) -> Result<String, String> {
    let what = format!("{} --version", exe.display());
    let mut child = Command::new(exe)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("{what}: {e}"))?;
    let stdout = child.stdout.take();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut text = Vec::new();
        let read = stdout.map(|mut o| o.read_to_end(&mut text));
        tx.send(read.map(|r| r.map(|_| text))).ok();
    });
    let read = rx.recv_timeout(limit);
    if read.is_err() {
        child.kill().ok();
    }
    let status = child.wait().map_err(|e| format!("{what}: {e}"))?;
    match read {
        Ok(Some(Ok(text))) if status.success() => {
            String::from_utf8(text).map_err(|_| format!("{what}: not UTF-8"))
        }
        Err(_) => Err(format!("{what}: no answer within {}s", limit.as_secs())),
        _ => Err(format!("{what}: {status}")),
    }
}

/// Checks `exe`'s code signature: a Developer ID Application certificate for identifier `id`.
pub fn codesign(exe: &Path, id: &str) -> Result<(), String> {
    let req = format!(
        "=anchor apple generic and identifier \"{id}\" and certificate 1[field.1.2.840.113635.100.6.2.6] exists and certificate leaf[field.1.2.840.113635.100.6.1.13] exists"
    );
    let what = format!("codesign --verify {}", exe.display());
    run(
        Command::new("codesign")
            .args(["--verify", "--strict", "-R", &req])
            .arg(exe),
        &what,
    )
    .map(drop)
    .map_err(|e| e.to_string())
}

/// Waits `time`: the one sleep of the crate.
pub fn wait(time: Duration) {
    thread::sleep(time);
}
