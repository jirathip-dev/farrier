use std::path::{Path, PathBuf};

use crate::job::{Job, Log, ProcessType, Restart, Schedule};

const HEAD: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
"#;

pub fn log(job: &Job, home: &Path) -> PathBuf {
    match &job.log {
        Log::Default => home.join(format!("Library/Logs/{}.log", job.label)),
        Log::File(file) => file.clone(),
    }
}

pub fn plist(job: &Job, home: &Path) -> String {
    let args: String = (job.argv.iter())
        .map(|a| ["\n    ", &string(&a.to_string_lossy())].concat())
        .collect();
    let mut keys = vec![
        key("Label", &string(job.label.as_str())),
        key("ProgramArguments", &format!("<array>{args}\n  </array>")),
    ];
    if !job.path.is_empty() {
        let path = string(&job.path);
        let env = format!("<dict>\n    <key>PATH</key>\n    {path}\n  </dict>");
        keys.push(key("EnvironmentVariables", &env));
    }
    keys.push(key("RunAtLoad", "<true/>"));
    keys.push(match job.schedule {
        Schedule::KeepAlive(Restart::Always) => key("KeepAlive", "<true/>"),
        Schedule::KeepAlive(Restart::OnFailure) => key(
            "KeepAlive",
            "<dict>\n    <key>SuccessfulExit</key>\n    <false/>\n  </dict>",
        ),
        Schedule::Every(every) => key(
            "StartInterval",
            &format!("<integer>{}</integer>", every.as_secs()),
        ),
        Schedule::Daily { hour, minute } => key(
            "StartCalendarInterval",
            &format!(
                "<dict>\n    <key>Hour</key>\n    <integer>{hour}</integer>\n    \
                 <key>Minute</key>\n    <integer>{minute}</integer>\n  </dict>"
            ),
        ),
    });
    match job.launchd.process_type {
        ProcessType::Standard => {}
        ProcessType::Interactive => keys.push(key("ProcessType", &string("Interactive"))),
        ProcessType::Background => keys.push(key("ProcessType", &string("Background"))),
    }
    if job.launchd.abandon_process_group {
        keys.push(key("AbandonProcessGroup", "<true/>"));
    }
    let log = string(&log(job, home).to_string_lossy());
    keys.push(key("StandardOutPath", &log));
    keys.push(key("StandardErrorPath", &log));
    format!("{HEAD}{}</dict>\n</plist>\n", keys.concat())
}

fn key(name: &str, value: &str) -> String {
    format!("  <key>{name}</key>\n  {value}\n")
}

fn string(text: &str) -> String {
    let text = text
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    format!("<string>{text}</string>")
}
