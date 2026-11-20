use std::ffi::OsStr;
use std::process::Command;
use std::time::Duration;

use crate::job::Error;

pub struct Said {
    pub ok: bool,
    pub text: String,
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
