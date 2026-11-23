use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use farrier_service::{
    Env, Error, Files, Installed, Job, Launchd, Log, Platform, ProcessType, Removed, Restart,
    Schedule, Service, Systemd,
};

const LAUNCHCTL: &str = r#"#!/bin/sh
d=@DIR@
echo "launchctl $*" >>"$d/calls"
if [ -f "$d/fail.$1" ] && [ "$(cat "$d/fail.$1")" -gt 0 ]; then
  echo $(($(cat "$d/fail.$1") - 1)) >"$d/fail.$1"
  echo "Bootstrap failed: 5: Input/output error"
  exit 5
fi
case $1 in
  bootstrap) touch "$d/loaded" ;;
  bootout) [ -f "$d/loaded" ] || exit 3; rm -f "$d/loaded" ;;
  print) [ -f "$d/loaded" ] || { echo "Could not find service"; exit 113; }; cat "$d/print" 2>/dev/null ;;
esac
exit 0
"#;

const SYSTEMCTL: &str = r#"#!/bin/sh
d=@DIR@
shift
echo "systemctl --user $*" >>"$d/calls"
case $1 in
  enable|start|restart) touch "$d/loaded" ;;
  disable|stop) rm -f "$d/loaded" ;;
  show)
    if [ -f "$d/show" ]; then cat "$d/show"; exit 0; fi
    sep=""
    for u in "$@"; do
      case $u in
        *.service|*.timer)
          printf '%s' "$sep"; sep="
"
          if [ -f "$d/loaded" ]; then echo ActiveState=active; else echo ActiveState=inactive; fi
          echo MainPID=0
          echo Result=success ;;
      esac
    done ;;
esac
exit 0
"#;

struct Host {
    dir: PathBuf,
    env: Env,
}

impl Host {
    fn new(name: &str, platform: Platform) -> Self {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
        if dir.exists() {
            fs::remove_dir_all(&dir).unwrap();
        }
        let bin = dir.join("bin");
        fs::create_dir_all(&bin).unwrap();
        for (tool, text) in [("launchctl", LAUNCHCTL), ("systemctl", SYSTEMCTL)] {
            let path = bin.join(tool);
            fs::write(&path, text.replace("@DIR@", &dir.display().to_string())).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let env = Env {
            platform,
            home: dir.join("home"),
            config_home: dir.join("home/.config"),
            uid: 501,
            tools: format!("{}:/usr/bin:/bin", bin.display()).into(),
            backoff: Duration::ZERO,
            runner: None,
            limit: Duration::ZERO,
        };
        Self { dir, env }
    }

    fn service(&self, job: Job) -> Service {
        Service::new(job, &self.env)
    }

    fn calls(&self) -> Vec<String> {
        let calls = fs::read_to_string(self.dir.join("calls")).unwrap_or_default();
        fs::remove_file(self.dir.join("calls")).ok();
        calls.lines().map(str::to_owned).collect()
    }

    fn set(&self, name: &str, text: &str) {
        fs::write(self.dir.join(name), text).unwrap();
    }
}

fn serve() -> Job {
    Job::new(
        "dev.test.serve",
        ["/opt/test/bin/test", "serve"],
        Schedule::KeepAlive(Restart::OnFailure),
    )
    .unwrap()
    .path("/usr/bin:/bin")
}

fn every() -> Job {
    Job::new(
        "dev.test.update",
        ["/opt/test/bin/test", "self-update"],
        Schedule::Every(Duration::from_secs(300)),
    )
    .unwrap()
}

fn mutating(calls: &[String]) -> Vec<String> {
    let reads = [" print ", " show "];
    let calls = calls
        .iter()
        .filter(|c| !reads.iter().any(|r| c.contains(r)));
    calls.cloned().collect()
}

#[test]
fn job_new_refuses_a_bad_label_a_relative_program_and_an_impossible_time() {
    let keep = Schedule::KeepAlive(Restart::Always);
    for label in [
        "Dev.test.serve",
        "serve",
        "dev..serve",
        "dev.-x",
        "dev.test/serve",
        "",
    ] {
        let job = Job::new(label, ["/bin/true"], keep);
        assert!(matches!(job, Err(Error::Label(_))), "{label}: {job:?}");
    }
    for argv in [&["bin/test"][..], &["test", "serve"], &[]] {
        let job = Job::new("dev.test.serve", argv, keep);
        assert!(matches!(job, Err(Error::Program(_))), "{argv:?}: {job:?}");
    }
    for schedule in [
        Schedule::Daily {
            hour: 24,
            minute: 0,
        },
        Schedule::Daily {
            hour: 3,
            minute: 60,
        },
        Schedule::Every(Duration::from_millis(500)),
    ] {
        let job = Job::new("dev.test.serve", ["/bin/true"], schedule);
        assert!(matches!(job, Err(Error::Time(_))), "{schedule:?}: {job:?}");
    }
    let ok = Job::new(
        "dev.test-2.serve",
        ["/bin/true"],
        Schedule::Daily {
            hour: 23,
            minute: 59,
        },
    );
    assert_eq!(ok.unwrap().label.as_str(), "dev.test-2.serve");
}

#[test]
fn one_job_renders_a_plist_on_launchd_and_a_unit_and_timer_on_systemd_without_touching_disk() {
    let mac = Host::new("render-mac", Platform::Launchd);
    let linux = Host::new("render-linux", Platform::Systemd);
    let job = every().log(Log::File("/var/log/a b%.log".into()));
    let plist = mac.service(job.clone()).files();
    let units = linux.service(job.clone()).files();
    assert_eq!(mac.service(job.clone()).files(), plist);
    assert!(!mac.env.home.exists() && !linux.env.home.exists());

    let [(path, text)] = &plist[..] else {
        panic!("{plist:?}")
    };
    assert_eq!(
        path,
        &mac.env
            .home
            .join("Library/LaunchAgents/dev.test.update.plist")
    );
    for key in [
        "<key>Label</key>\n  <string>dev.test.update</string>",
        "<key>StartInterval</key>\n  <integer>300</integer>",
        "<key>RunAtLoad</key>\n  <true/>",
        "<key>StandardErrorPath</key>\n  <string>/var/log/a b%.log</string>",
    ] {
        assert!(text.contains(key), "{key} in {text}");
    }
    assert!(!text.contains("EnvironmentVariables") && !text.contains("ProcessType"));

    let [(service, unit), (timer, timer_text)] = &units[..] else {
        panic!("{units:?}")
    };
    let dir = linux.env.config_home.join("systemd/user");
    assert_eq!(
        (service, timer),
        (
            &dir.join("dev.test.update.service"),
            &dir.join("dev.test.update.timer")
        )
    );
    assert!(
        unit.contains("Type=oneshot\nExecStart=/opt/test/bin/test self-update\n"),
        "{unit}"
    );
    assert!(
        unit.contains("StandardOutput=append:/var/log/a b%%.log"),
        "{unit}"
    );
    assert!(!unit.contains("[Install]"), "{unit}");
    assert!(
        timer_text.contains("OnActiveSec=0\nOnUnitActiveSec=300\n"),
        "{timer_text}"
    );
    assert!(
        timer_text.contains("WantedBy=timers.target"),
        "{timer_text}"
    );
}

#[test]
fn rendering_writes_each_key_a_job_sets_and_escapes_what_needs_it() {
    let job = Job::new(
        "dev.test.serve",
        ["/opt/my app/test", "--name", "a&b <c> $HOME 100%"],
        Schedule::KeepAlive(Restart::Always),
    )
    .unwrap()
    .path("/opt/bin:/usr/bin")
    .launchd(Launchd {
        process_type: ProcessType::Interactive,
        abandon_process_group: true,
    })
    .systemd(Systemd {
        oom_policy_continue: true,
    });
    let mac = Host::new("keys-mac", Platform::Launchd);
    let linux = Host::new("keys-linux", Platform::Systemd);
    let (_, plist) = mac.service(job.clone()).files().remove(0);
    for key in [
        "<string>/opt/my app/test</string>",
        "<string>a&amp;b &lt;c&gt; $HOME 100%</string>",
        "<key>PATH</key>\n    <string>/opt/bin:/usr/bin</string>",
        "<key>KeepAlive</key>\n  <true/>",
        "<key>ProcessType</key>\n  <string>Interactive</string>",
        "<key>AbandonProcessGroup</key>\n  <true/>",
    ] {
        assert!(plist.contains(key), "{key} in {plist}");
    }
    let log = mac.env.home.join("Library/Logs/dev.test.serve.log");
    assert!(plist.contains(&format!(
        "<key>StandardOutPath</key>\n  <string>{}</string>",
        log.display()
    )));
    let units = linux.service(job).files();
    let [(_, unit)] = &units[..] else {
        panic!("{units:?}")
    };
    for key in [
        r#"ExecStart="/opt/my app/test" --name "a&b <c> $$HOME 100%%""#,
        r#"Environment="PATH=/opt/bin:/usr/bin""#,
        "Restart=always\nOOMPolicy=continue\n",
        "[Install]\nWantedBy=default.target\n",
    ] {
        assert!(unit.contains(key), "{key} in {unit}");
    }
    assert!(
        !unit.contains("StandardOutput"),
        "the journal is the default: {unit}"
    );

    let daily = Job::new(
        "dev.test.clean",
        ["/bin/true"],
        Schedule::Daily { hour: 3, minute: 5 },
    )
    .unwrap();
    let (_, plist) = mac.service(daily.clone()).files().remove(0);
    assert!(plist.contains(
        "<key>Hour</key>\n    <integer>3</integer>\n    <key>Minute</key>\n    <integer>5</integer>"
    ));
    let (_, timer) = linux.service(daily).files().remove(1);
    assert!(
        timer.contains("OnCalendar=*-*-* 03:05:00\nPersistent=true\n"),
        "{timer}"
    );
}

#[test]
fn launchd_install_writes_and_loads_once_then_changes_nothing() {
    let host = Host::new("mac-install", Platform::Launchd);
    let service = host.service(serve());
    assert_eq!(service.status().unwrap().files, Files::Missing);
    assert_eq!(service.install().unwrap(), Installed::Written);
    let plist = host
        .env
        .home
        .join("Library/LaunchAgents/dev.test.serve.plist");
    let domain = format!("launchctl bootstrap gui/501 {}", plist.display());
    assert_eq!(mutating(&host.calls()), std::slice::from_ref(&domain));
    assert!(host.env.home.join("Library/Logs").is_dir());

    assert_eq!(service.install().unwrap(), Installed::Unchanged);
    assert_eq!(mutating(&host.calls()), Vec::<String>::new());

    host.set(
        "print",
        "gui/501/dev.test.serve = {\n\tstate = running\n\tpid = 4242\n}\n",
    );
    let status = service.status().unwrap();
    assert_eq!(
        (status.files, status.loaded, status.pid, status.refused),
        (Files::Same, true, Some(4242), None)
    );

    fs::remove_file(host.dir.join("loaded")).unwrap();
    assert_eq!(service.install().unwrap(), Installed::Loaded);
    assert_eq!(mutating(&host.calls()), std::slice::from_ref(&domain));

    let changed = host.service(serve().path("/opt/bin"));
    assert_eq!(changed.status().unwrap().files, Files::Differs);
    assert_eq!(changed.install().unwrap(), Installed::Written);
    assert_eq!(
        mutating(&host.calls()),
        [
            "launchctl bootout gui/501/dev.test.serve".to_owned(),
            domain
        ]
    );
    assert!(fs::read_to_string(&plist).unwrap().contains("/opt/bin"));
}

#[test]
fn launchd_install_rebootstraps_a_refused_job_with_three_tries() {
    let host = Host::new("mac-refused", Platform::Launchd);
    let service = host.service(serve());
    service.install().unwrap();
    host.calls();
    host.set("print", "\tlast exit reason = OS_REASON_CODESIGNING\n");
    assert_eq!(
        service.status().unwrap().refused.as_deref(),
        Some("last exit reason = OS_REASON_CODESIGNING")
    );

    host.set("fail.bootstrap", "2");
    assert_eq!(service.install().unwrap(), Installed::Loaded);
    let calls = mutating(&host.calls());
    assert_eq!(calls.len(), 4, "{calls:?}");
    assert_eq!(calls[0], "launchctl bootout gui/501/dev.test.serve");
    assert!(
        calls[1..]
            .iter()
            .all(|c| c.starts_with("launchctl bootstrap gui/501 ")),
        "{calls:?}"
    );

    fs::remove_file(host.dir.join("print")).unwrap();
    fs::remove_file(host.dir.join("loaded")).unwrap();
    host.set("fail.bootstrap", "3");
    let err = service.install().unwrap_err();
    assert!(
        matches!(&err, Error::Ctl(cmd, said) if cmd.starts_with("launchctl bootstrap") && said.contains("Input/output")),
        "{err}"
    );
    assert_eq!(mutating(&host.calls()).len(), 3);
}

#[test]
fn launchd_install_removes_superseded_jobs_once() {
    let host = Host::new("mac-supersedes", Platform::Launchd);
    let agents = host.env.home.join("Library/LaunchAgents");
    fs::create_dir_all(&agents).unwrap();
    fs::write(agents.join("old.plist"), "old").unwrap();
    let service = host.service(serve().supersedes(["old", "never-installed"]));
    service.install().unwrap();
    let calls = mutating(&host.calls());
    assert_eq!(
        calls.first().map(String::as_str),
        Some("launchctl bootout gui/501/old")
    );
    assert!(!agents.join("old.plist").exists());
    assert_eq!(service.install().unwrap(), Installed::Unchanged);
    assert_eq!(mutating(&host.calls()), Vec::<String>::new());
}

#[test]
fn launchd_start_stop_restart_and_uninstall_are_idempotent() {
    let host = Host::new("mac-verbs", Platform::Launchd);
    let service = host.service(serve());
    service.write().unwrap();
    service.stop().unwrap();
    assert_eq!(mutating(&host.calls()), Vec::<String>::new());
    service.start().unwrap();
    service.start().unwrap();
    let calls = mutating(&host.calls());
    assert!(calls[0].starts_with("launchctl bootstrap "), "{calls:?}");
    assert_eq!(calls[1], "launchctl kickstart gui/501/dev.test.serve");
    service.restart().unwrap();
    assert_eq!(
        mutating(&host.calls()),
        ["launchctl kickstart -k gui/501/dev.test.serve"]
    );
    host.set("fail.kickstart", "1");
    service.restart().unwrap();
    let calls = mutating(&host.calls());
    assert_eq!(calls.len(), 3, "{calls:?}");
    assert_eq!(calls[0], "launchctl kickstart -k gui/501/dev.test.serve");
    assert_eq!(calls[1], "launchctl bootout gui/501/dev.test.serve");
    assert!(calls[2].starts_with("launchctl bootstrap "), "{calls:?}");
    service.stop().unwrap();
    service.stop().unwrap();
    assert_eq!(
        mutating(&host.calls()),
        ["launchctl bootout gui/501/dev.test.serve"]
    );

    service.start().unwrap();
    assert_eq!(service.uninstall().unwrap(), Removed::Removed);
    assert_eq!(service.status().unwrap().files, Files::Missing);
    assert!(!service.status().unwrap().loaded);
    host.calls();
    assert_eq!(service.uninstall().unwrap(), Removed::Absent);
    assert_eq!(mutating(&host.calls()), Vec::<String>::new());
}

#[test]
fn systemd_install_enables_the_timer_once_and_uninstall_removes_both_units() {
    let host = Host::new("linux-timer", Platform::Systemd);
    let dir = host.env.config_home.join("systemd/user");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("old-update.service"), "old").unwrap();
    let service = host.service(every().supersedes(["old-update"]));
    assert_eq!(service.install().unwrap(), Installed::Written);
    assert_eq!(
        mutating(&host.calls()),
        [
            "systemctl --user disable --now old-update.service",
            "systemctl --user daemon-reload",
            "systemctl --user daemon-reload",
            "systemctl --user enable --now dev.test.update.timer",
        ]
    );
    assert!(
        dir.join("dev.test.update.timer").is_file() && !dir.join("old-update.service").exists()
    );
    assert_eq!(service.install().unwrap(), Installed::Unchanged);
    assert_eq!(mutating(&host.calls()), Vec::<String>::new());
    assert!(service.status().unwrap().loaded);

    service.stop().unwrap();
    assert_eq!(
        mutating(&host.calls()),
        ["systemctl --user stop dev.test.update.timer"]
    );
    service.stop().unwrap();
    assert_eq!(mutating(&host.calls()), Vec::<String>::new());

    assert_eq!(service.uninstall().unwrap(), Removed::Removed);
    assert_eq!(
        mutating(&host.calls()),
        [
            "systemctl --user disable --now dev.test.update.timer",
            "systemctl --user daemon-reload"
        ]
    );
    assert!(
        !dir.join("dev.test.update.service").exists()
            && !dir.join("dev.test.update.timer").exists()
    );
    assert_eq!(service.uninstall().unwrap(), Removed::Absent);
}

#[test]
fn systemd_reads_a_failed_unit_as_refused_and_restart_rebootstraps_it() {
    let host = Host::new("linux-failed", Platform::Systemd);
    let service = host.service(serve());
    service.install().unwrap();
    host.calls();
    host.set("show", "ActiveState=failed\nMainPID=0\nResult=exit-code\n");
    let status = service.status().unwrap();
    assert_eq!(
        (status.loaded, status.refused.as_deref()),
        (false, Some("failed (exit-code)"))
    );
    service.restart().unwrap();
    assert_eq!(
        mutating(&host.calls()),
        [
            "systemctl --user disable --now dev.test.serve.service",
            "systemctl --user daemon-reload",
            "systemctl --user enable --now dev.test.serve.service",
        ]
    );
    host.set("show", "ActiveState=active\nMainPID=77\nResult=success\n");
    assert_eq!(service.status().unwrap().pid, Some(77));
    service.restart().unwrap();
    assert_eq!(
        mutating(&host.calls()),
        ["systemctl --user restart dev.test.serve.service"]
    );
}

#[test]
fn commands_name_the_lines_a_person_runs() {
    let mac = Host::new("cmds-mac", Platform::Launchd);
    let c = mac.service(serve()).commands();
    let plist = mac
        .env
        .home
        .join("Library/LaunchAgents/dev.test.serve.plist");
    assert_eq!(
        c.load,
        format!("launchctl bootstrap gui/501 {}", plist.display())
    );
    assert_eq!(c.unload, "launchctl bootout gui/501/dev.test.serve");
    assert_eq!(c.restart, "launchctl kickstart -k gui/501/dev.test.serve");
    let log = mac.env.home.join("Library/Logs/dev.test.serve.log");
    assert_eq!(c.log, format!("tail -f {}", log.display()));

    let linux = Host::new("cmds-linux", Platform::Systemd);
    let c = linux.service(every()).commands();
    assert_eq!(
        c.load,
        "systemctl --user daemon-reload && systemctl --user enable --now dev.test.update.timer"
    );
    assert_eq!(
        c.unload,
        "systemctl --user disable --now dev.test.update.timer"
    );
    assert_eq!(
        c.restart,
        "systemctl --user restart dev.test.update.service"
    );
    assert_eq!(c.log, "journalctl --user -u dev.test.update.service -f");
    let c = linux
        .service(every().log(Log::File("/tmp/u.log".into())))
        .commands();
    assert_eq!(c.log, "tail -f /tmp/u.log");
}

#[test]
fn a_missing_tool_is_an_error_not_a_stopped_job() {
    let mut host = Host::new("no-tool", Platform::Launchd);
    host.env.tools = host.dir.join("nothing").into_os_string();
    let err = host.service(serve()).status().unwrap_err();
    assert!(
        matches!(&err, Error::Ctl(cmd, _) if cmd.starts_with("launchctl print")),
        "{err}"
    );
}

#[test]
fn env_host_reads_this_host() {
    let env = Env::host().unwrap();
    let want = if cfg!(target_os = "macos") {
        Platform::Launchd
    } else {
        Platform::Systemd
    };
    assert_eq!(env.platform, want);
    assert!(env.home.is_absolute() && env.config_home.is_absolute());
}
