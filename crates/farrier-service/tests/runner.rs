use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use farrier_service::{
    Env, Error, Files, Installed, Job, Platform, Restart, Runner, Schedule, Service,
};

#[derive(Default)]
struct Fake {
    calls: Vec<String>,
    limits: Vec<Duration>,
    loaded: bool,
    fail_load: usize,
    time_out: bool,
}

impl Fake {
    fn said(&mut self, tool: &str, args: &[String], limit: Duration) -> io::Result<(bool, String)> {
        let line = format!("{tool} {}", args.join(" "));
        self.calls.push(line);
        self.limits.push(limit);
        if self.time_out {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "timed out after 7s",
            ));
        }
        let verb = args
            .iter()
            .find(|a| !a.starts_with('-'))
            .map_or("", String::as_str);
        if matches!(verb, "bootstrap" | "enable") && self.fail_load > 0 {
            self.fail_load -= 1;
            return Ok((false, "Bootstrap failed: 5: Input/output error\n".into()));
        }
        match verb {
            "bootstrap" | "enable" | "start" | "restart" => self.loaded = true,
            "bootout" | "disable" | "stop" => self.loaded = false,
            "print" if !self.loaded => return Ok((false, "Could not find service\n".into())),
            "print" => return Ok((true, "\tstate = running\n\tpid = 4242\n".into())),
            "show" => {
                let state = if self.loaded { "active" } else { "inactive" };
                let unit = format!("ActiveState={state}\nMainPID=0\nResult=success\n");
                return Ok((true, format!("{unit}\n{unit}")));
            }
            _ => {}
        }
        Ok((true, String::new()))
    }
}

struct Host {
    fake: Arc<Mutex<Fake>>,
    env: Env,
}

impl Host {
    fn new(name: &str, platform: Platform, fake: Fake) -> Self {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        let fake = Arc::new(Mutex::new(fake));
        let shared = Arc::clone(&fake);
        let env = Env {
            platform,
            home: dir.join("home"),
            config_home: dir.join("home/.config"),
            uid: 501,
            tools: PathBuf::from("/nonexistent").into(),
            backoff: Duration::ZERO,
            runner: Some(Runner(Arc::new(move |tool, args, limit| {
                shared.lock().unwrap().said(tool, args, limit)
            }))),
            limit: Duration::from_secs(7),
        };
        Self { fake, env }
    }

    fn calls(&self) -> Vec<String> {
        std::mem::take(&mut self.fake.lock().unwrap().calls)
    }
}

fn serve() -> Job {
    Job::new(
        "dev.test.serve",
        ["/opt/test/bin/test", "serve"],
        Schedule::KeepAlive(Restart::OnFailure),
    )
    .unwrap()
}

fn every() -> Job {
    let schedule = Schedule::Every(Duration::from_secs(300));
    Job::new(
        "dev.test.update",
        ["/opt/test/bin/test", "self-update"],
        schedule,
    )
    .unwrap()
}

#[test]
fn launchd_install_retries_the_load_and_status_reads_the_pid_through_the_runner() {
    let host = Host::new(
        "runner-mac",
        Platform::Launchd,
        Fake {
            fail_load: 2,
            ..Fake::default()
        },
    );
    let service = Service::new(serve(), &host.env);
    assert_eq!(service.install().unwrap(), Installed::Written);
    let bootstrap = "launchctl bootstrap gui/501 ".to_owned()
        + &host
            .env
            .home
            .join("Library/LaunchAgents/dev.test.serve.plist")
            .display()
            .to_string();
    assert_eq!(
        host.calls(),
        [
            "launchctl print gui/501/dev.test.serve".to_owned(),
            bootstrap.clone(),
            bootstrap.clone(),
            bootstrap,
        ]
    );
    let status = service.status().unwrap();
    assert_eq!(
        (status.files, status.loaded, status.pid),
        (Files::Same, true, Some(4242))
    );
    assert_eq!(service.install().unwrap(), Installed::Unchanged);
    let fake = host.fake.lock().unwrap();
    assert!(
        fake.limits.iter().all(|l| *l == Duration::from_secs(7)),
        "{:?}",
        fake.limits
    );
}

#[test]
fn launchd_load_gives_up_after_three_tries_with_the_runner_text() {
    let host = Host::new(
        "runner-mac-fail",
        Platform::Launchd,
        Fake {
            fail_load: 3,
            ..Fake::default()
        },
    );
    let err = Service::new(serve(), &host.env).install().unwrap_err();
    assert!(
        matches!(&err, Error::Ctl(cmd, said) if cmd.starts_with("launchctl bootstrap") && said.contains("Input/output")),
        "{err}"
    );
    assert_eq!(
        host.calls()
            .iter()
            .filter(|c| c.contains("bootstrap"))
            .count(),
        3
    );
}

#[test]
fn systemd_install_status_and_uninstall_run_through_the_runner() {
    let host = Host::new(
        "runner-linux",
        Platform::Systemd,
        Fake {
            fail_load: 1,
            ..Fake::default()
        },
    );
    let service = Service::new(every(), &host.env);
    assert_eq!(service.install().unwrap(), Installed::Written);
    let show = "systemctl --user show -p ActiveState -p MainPID -p Result dev.test.update.service dev.test.update.timer";
    assert_eq!(
        host.calls(),
        [
            show,
            "systemctl --user daemon-reload",
            "systemctl --user enable --now dev.test.update.timer",
            "systemctl --user daemon-reload",
            "systemctl --user enable --now dev.test.update.timer",
        ]
    );
    let status = service.status().unwrap();
    assert_eq!(
        (status.files, status.loaded, status.pid),
        (Files::Same, true, None)
    );
    service.uninstall().unwrap();
    assert_eq!(
        host.calls(),
        [
            show,
            show,
            "systemctl --user disable --now dev.test.update.timer",
            "systemctl --user daemon-reload",
        ]
    );
}

#[test]
fn a_runner_time_out_is_a_ctl_error_naming_the_command() {
    for (platform, cmd) in [
        (Platform::Launchd, "launchctl print gui/501/dev.test.serve"),
        (
            Platform::Systemd,
            "systemctl --user show -p ActiveState -p MainPID -p Result dev.test.serve.service",
        ),
    ] {
        let host = Host::new(
            "runner-time-out",
            platform,
            Fake {
                time_out: true,
                ..Fake::default()
            },
        );
        let err = Service::new(serve(), &host.env).status().unwrap_err();
        assert!(
            matches!(&err, Error::Ctl(c, said) if c == cmd && said.contains("timed out")),
            "{err}"
        );
    }
}

#[test]
fn env_equality_ignores_which_runner_is_set() {
    let host = Host::new("runner-eq", Platform::Launchd, Fake::default());
    let mut other = host.env.clone();
    other.runner = Some(Runner(Arc::new(|_, _, _| Ok((true, String::new())))));
    assert_eq!(host.env, other);
    assert!(format!("{other:?}").contains("Runner"));
}
