use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use farrier_service::{
    Env, Job, Launchd, Log, Platform, ProcessType, Restart, Schedule, Service, Systemd,
};

const MINUTES_5: Schedule = Schedule::Every(Duration::from_mins(5));

fn shed(home: &Path, path: &str) -> Result<Vec<Job>, farrier_service::Error> {
    let exe = home.join(".cargo/bin/shed");
    let exe = exe.as_os_str();
    Ok(vec![
        Job::new(
            "dev.shed.serve",
            [exe, "serve".as_ref()],
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
        .supersedes(["shed"]),
        Job::new("dev.shed.update", [exe, "self-update".as_ref()], MINUTES_5)?
            .path(path)
            .launchd(Launchd {
                process_type: ProcessType::Background,
                abandon_process_group: false,
            })
            .supersedes(["shed-update"]),
    ])
}

fn cooee(home: &Path, path: &str) -> Result<Vec<Job>, farrier_service::Error> {
    let exe = home.join(".cargo/bin/cooee");
    let data = home.join(".local/share/cooee/cooee");
    let argv = |verb: &'static str| {
        [
            exe.as_os_str(),
            verb.as_ref(),
            "--instance".as_ref(),
            "cooee".as_ref(),
        ]
    };
    Ok(vec![
        Job::new(
            "dev.cooee.cooee",
            argv("serve"),
            Schedule::KeepAlive(Restart::Always),
        )?
        .path(path)
        .log(Log::File(data.join("cooee.log"))),
        Job::new(
            "dev.cooee.cooee.update",
            argv("update"),
            Schedule::Every(Duration::from_mins(15)),
        )?
        .path(path)
        .log(Log::File(data.join("update.log"))),
    ])
}

fn drover(home: &Path, path: &str) -> Result<Vec<Job>, farrier_service::Error> {
    let exe = home.join(".cargo/bin/drover");
    let state = home.join(".local/state/drover");
    let keep = Schedule::KeepAlive(Restart::OnFailure);
    let jobs = [
        ("dev.drover.tick", &["tick"][..], MINUTES_5),
        (
            "dev.drover.fast",
            &["drive", "--fast"],
            Schedule::Every(Duration::from_mins(1)),
        ),
        ("dev.drover.update", &["self-update"], MINUTES_5),
        (
            "dev.drover.clean",
            &["clean", "--scheduled"],
            Schedule::Daily {
                hour: 3,
                minute: 30,
            },
        ),
        (
            "dev.drover.serve",
            &["serve", "--addr", "127.0.0.1:7777"],
            keep,
        ),
        (
            "dev.drover.service",
            &["service", "--api", "127.0.0.1:7777"],
            keep,
        ),
    ];
    let job = |(label, args, schedule): (&str, &[&str], Schedule)| {
        let argv = std::iter::once(exe.as_os_str()).chain(args.iter().map(AsRef::as_ref));
        let (log, process_type) = match schedule {
            Schedule::KeepAlive(_) => ("serve.log", ProcessType::Interactive),
            _ => ("tick.log", ProcessType::Standard),
        };
        Ok(Job::new(label, argv, schedule)?
            .path(path)
            .log(Log::File(state.join(log)))
            .launchd(Launchd {
                process_type,
                abandon_process_group: true,
            }))
    };
    jobs.into_iter().map(job).collect()
}

fn main() -> Result<(), Box<dyn Error>> {
    let to = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or("usage: units <dir>")?;
    fs::create_dir_all(&to)?;
    for (platform, home) in [
        (Platform::Launchd, "/Users/me"),
        (Platform::Systemd, "/home/me"),
    ] {
        let home = PathBuf::from(home);
        let env = Env {
            platform,
            config_home: home.join(".config"),
            uid: 501,
            tools: "/usr/bin:/bin".into(),
            backoff: Duration::ZERO,
            home,
        };
        let h = env.home.display();
        let path =
            format!("{h}/.cargo/bin:{h}/.local/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin");
        let jobs = [shed, cooee, drover].map(|of| of(&env.home, &path));
        for job in jobs
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
        {
            for (file, text) in Service::new(job, &env).files() {
                let name = file.file_name().ok_or("a job file has no name")?;
                fs::write(to.join(name), text)?;
            }
        }
    }
    Ok(())
}
