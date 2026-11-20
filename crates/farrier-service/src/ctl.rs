use std::ffi::OsStr;
use std::process::Command;
use std::time::Duration;

use crate::job::Error;

pub struct Said {
    pub ok: bool,
    pub text: String,
}

#[derive(Default)]
pub struct Live {
    pub loaded: bool,
    pub pid: Option<u32>,
    pub refused: Option<String>,
}

impl Live {
    pub fn launchd(said: &Said) -> Self {
        let lines = || said.text.lines().map(str::trim);
        let refusal = |l: &&str| l.contains("spawn failed") || l.contains("OS_REASON_CODESIGNING");
        Self {
            loaded: said.ok,
            pid: lines().find_map(|l| l.strip_prefix("pid = ")?.parse().ok()),
            refused: lines().find(refusal).filter(|_| said.ok).map(str::to_owned),
        }
    }

    pub fn systemd(text: &str, timer: bool) -> Self {
        let field = |block: &str, key: &str| {
            let value = block
                .lines()
                .find_map(|l| l.strip_prefix(key)?.strip_prefix('='));
            value.unwrap_or_default().to_owned()
        };
        let mut blocks = text.split("\n\n");
        let service = blocks.next().unwrap_or_default();
        let state = field(service, "ActiveState");
        Self {
            loaded: if timer {
                field(blocks.next().unwrap_or_default(), "ActiveState") == "active"
            } else {
                matches!(state.as_str(), "active" | "activating" | "reloading")
            },
            pid: field(service, "MainPID")
                .parse()
                .ok()
                .filter(|pid| *pid != 0),
            refused: (state == "failed").then(|| format!("failed ({})", field(service, "Result"))),
        }
    }
}

pub fn run(tools: &OsStr, tool: &str, args: &[String]) -> Result<Said, Error> {
    let out = Command::new(tool)
        .args(args)
        .env("PATH", tools)
        .output()
        .map_err(|e| Error::Ctl(format!("{tool} {}", args.join(" ")), e.to_string()))?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    Ok(Said {
        ok: out.status.success(),
        text,
    })
}

pub fn wait(time: Duration) {
    std::thread::sleep(time);
}
