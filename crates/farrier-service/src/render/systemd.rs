use crate::job::{Job, Log, Restart, Schedule};

pub fn units(job: &Job) -> Vec<(&'static str, String)> {
    let label = &job.label;
    let mut service = vec![format!("[Unit]\nDescription={label}\n\n[Service]")];
    let timer = match job.schedule {
        Schedule::KeepAlive(_) => None,
        Schedule::Every(every) => Some(format!("OnUnitActiveSec={}", every.as_secs())),
        Schedule::Daily { hour, minute } => Some(format!(
            "OnCalendar=*-*-* {hour:02}:{minute:02}:00\nPersistent=true"
        )),
    };
    if timer.is_some() {
        service.push("Type=oneshot".to_owned());
    }
    let argv: Vec<String> = (job.argv.iter())
        .map(|a| arg(&a.to_string_lossy()))
        .collect();
    service.push(format!("ExecStart={}", argv.join(" ")));
    if !job.path.is_empty() {
        service.push(format!(
            "Environment={}",
            quoted(&format!("PATH={}", job.path))
        ));
    }
    match job.schedule {
        Schedule::KeepAlive(Restart::Always) => service.push("Restart=always".to_owned()),
        Schedule::KeepAlive(Restart::OnFailure) => service.push("Restart=on-failure".to_owned()),
        _ => {}
    }
    if job.systemd.oom_policy_continue {
        service.push("OOMPolicy=continue".to_owned());
    }
    if let Log::File(file) = &job.log {
        let file = file.to_string_lossy().replace('%', "%%");
        service.push(format!(
            "StandardOutput=append:{file}\nStandardError=append:{file}"
        ));
    }
    let Some(timer) = timer else {
        service.push("\n[Install]\nWantedBy=default.target\n".to_owned());
        return vec![("service", service.join("\n"))];
    };
    service.push(String::new());
    let timer = format!(
        "[Unit]\nDescription={label}\n\n[Timer]\nOnActiveSec=0\n{timer}\n\n\
         [Install]\nWantedBy=timers.target\n"
    );
    vec![("service", service.join("\n")), ("timer", timer)]
}

fn arg(text: &str) -> String {
    let plain = !text.is_empty()
        && !text
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '"' | '\'' | '\\' | ';'));
    if plain {
        text.replace('%', "%%").replace('$', "$$")
    } else {
        quoted(text).replace('$', "$$")
    }
}

fn quoted(text: &str) -> String {
    let text = text
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('%', "%%");
    format!("\"{text}\"")
}
