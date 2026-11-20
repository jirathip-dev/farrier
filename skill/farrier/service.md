# farrier-service

One `Job` value is a per-user launchd job on macOS and a `systemctl --user` unit on Linux. `Service` writes,
loads, starts, stops, restarts and removes it; every verb is idempotent. The crate uses std only.

```rust,no_run
use std::path::Path;
use std::time::Duration;

use farrier_service::{
    Env, Error, Files, Job, Launchd, ProcessType, Restart, Schedule, Service, Systemd,
};

fn serve(exe: &Path, path: &str) -> Result<Job, Error> {
    Ok(Job::new(
        "dev.example.serve",
        [exe.as_os_str(), "serve".as_ref()],
        Schedule::KeepAlive(Restart::Always),
    )?
    .path(path)
    .launchd(Launchd {
        process_type: ProcessType::Interactive,
        abandon_process_group: false,
    })
    .systemd(Systemd {
        oom_policy_continue: true,
    })
    .supersedes(["example"]))
}

fn update(exe: &Path, path: &str) -> Result<Job, Error> {
    Ok(Job::new(
        "dev.example.update",
        [exe.as_os_str(), "self-update".as_ref()],
        Schedule::Every(Duration::from_secs(300)),
    )?
    .path(path))
}

fn install(exe: &Path, force: bool) -> Result<Vec<String>, Error> {
    let env = Env::host()?;
    let mut lines = Vec::new();
    for job in [serve(exe, "/usr/bin:/bin")?, update(exe, "/usr/bin:/bin")?] {
        let service = Service::new(job, &env);
        if service.status()?.files == Files::Differs && !force {
            lines.push(format!("{} has other contents", service.label()));
            continue;
        }
        match service.install() {
            Ok(done) => lines.push(format!("{}: {done}", service.label())),
            Err(e) => lines.push(format!("{e} → {}", service.commands().load)),
        }
    }
    Ok(lines)
}
```

## A `Job`, field by field

`Job::new(label, argv, schedule)` refuses a label that is not lowercase reverse-DNS (`dev.example.serve`), a
first argument that is not an absolute path, `Every` under one second and a `Daily` time past 23:59. The builders
set the rest.

| field | plist (macOS) | unit (Linux) |
|---|---|---|
| `label` | `Label`; the file `~/Library/LaunchAgents/<label>.plist` | `Description=`; the files `$XDG_CONFIG_HOME/systemd/user/<label>.service` (and `.timer`) |
| `argv` | `ProgramArguments` | `ExecStart=`, each word quoted when it needs it, `%` and `$` escaped |
| `schedule`: `KeepAlive(Always)` | `KeepAlive` true | `Restart=always`, `[Install] WantedBy=default.target` |
| `schedule`: `KeepAlive(OnFailure)` | `KeepAlive` `{SuccessfulExit: false}` | `Restart=on-failure`, `WantedBy=default.target` |
| `schedule`: `Every(d)` | `StartInterval` (seconds) | `Type=oneshot`; a `.timer` with `OnActiveSec=0`, `OnUnitActiveSec=<secs>`, `WantedBy=timers.target` |
| `schedule`: `Daily { hour, minute }` | `StartCalendarInterval` `{Hour, Minute}` | `Type=oneshot`; a `.timer` with `OnActiveSec=0`, `OnCalendar=*-*-* HH:MM:00`, `Persistent=true` |
| (always) | `RunAtLoad` true | a timer's `OnActiveSec=0` runs it when loaded |
| `path` (`.path`) | `EnvironmentVariables` `{PATH}`; none when empty | `Environment="PATH=…"`; none when empty |
| `log`: `Log::Default` | `StandardOutPath`, `StandardErrorPath`: `~/Library/Logs/<label>.log` | the journal (no key) |
| `log`: `Log::File(p)` | both paths `p` | `StandardOutput=append:p`, `StandardError=append:p` |
| `launchd.process_type` | `ProcessType` (`Interactive`, `Background`); none for `Standard` | nothing |
| `launchd.abandon_process_group` | `AbandonProcessGroup` true | nothing |
| `systemd.oom_policy_continue` | nothing | `OOMPolicy=continue` |
| `supersedes` | `install` boots out `gui/<uid>/<name>` and removes `<name>.plist` | `install` disables and removes `<name>.service` and `<name>.timer` |

## The verbs

| verb | does | returns |
|---|---|---|
| `files()` | the paths and texts the job renders to; pure | `Vec<(PathBuf, String)>` |
| `write()` | writes only the files that differ, and the log's folder; loads nothing | `Unchanged` or `Written` |
| `install()` | removes `supersedes`, writes on a difference, loads only when not loaded, reloads a loaded job whose files changed, re-bootstraps a refused job; a load is tried 3 times with backoff | `Unchanged`, `Written` or `Loaded` |
| `uninstall()` | unloads, removes the files | `Absent` or `Removed` |
| `start()` | loads it when not loaded, else kicks it (`kickstart`, `start`) | |
| `stop()` | `bootout` on macOS (the plist stays), `systemctl --user stop` on Linux; nothing when not loaded | |
| `restart()` | `kickstart -k` or `restart`; re-bootstraps when refused, not loaded or the restart fails | |
| `status()` | `Files::{Same, Differs, Missing}`, `loaded`, `pid`, `refused` (`spawn failed`, `OS_REASON_CODESIGNING`, a failed unit) | `Status` |
| `commands()` | the `load`, `unload`, `restart` and `log` lines to print for a person | `Commands` |

The host is a value: `Env::host()` reads the platform, `HOME`, `XDG_CONFIG_HOME`, the home folder's owner (the
launchd domain `gui/<uid>`) and `PATH` (where `launchctl` and `systemctl` are found). A test builds its own `Env`
over a temp folder with stub `launchctl` or `systemctl` on `tools` and a zero `backoff`, so both platforms run on
every OS without a real job manager or a clock.

The rendered texts of every job the consumers install are in farrier's `fixtures/units/` (`just gen`).
