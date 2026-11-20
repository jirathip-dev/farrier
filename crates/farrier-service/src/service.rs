use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use crate::ctl::{self, Said};
use crate::env::{Env, Platform};
use crate::job::{Error, Job, Label, Log, Schedule};
use crate::render;

/// One job on one host: every verb is idempotent.
pub struct Service {
    job: Job,
    env: Env,
}

/// What `install` or `write` changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Installed {
    Unchanged,
    Written,
    Loaded,
}

/// What `uninstall` changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Removed {
    Absent,
    Removed,
}

/// The job's files on disk against what it renders.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Files {
    Same,
    Differs,
    Missing,
}

/// The job as the host sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub files: Files,
    pub loaded: bool,
    pub pid: Option<u32>,
    pub refused: Option<String>,
}

/// The shell lines a person runs for the job.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commands {
    pub load: String,
    pub unload: String,
    pub restart: String,
    pub log: String,
}

#[derive(Clone, Copy)]
enum Verb {
    Probe,
    Load,
    Unload,
    Start,
    Stop,
    Restart,
}

impl Service {
    /// The job on the host `env` describes.
    #[must_use]
    pub fn new(job: Job, env: &Env) -> Self {
        Self {
            job,
            env: env.clone(),
        }
    }

    /// The job's label.
    #[must_use]
    pub fn label(&self) -> &Label {
        &self.job.label
    }

    /// Each file the job needs, with its text: a plist, or a unit and its timer.
    #[must_use]
    pub fn files(&self) -> Vec<(PathBuf, String)> {
        let label = self.job.label.as_str();
        match self.env.platform {
            Platform::Launchd => {
                let text = render::launchd::plist(&self.job, &self.env.home);
                vec![(self.plist(label), text)]
            }
            Platform::Systemd => (render::systemd::units(&self.job).into_iter())
                .map(|(ext, text)| (self.units().join(format!("{label}.{ext}")), text))
                .collect(),
        }
    }

    /// Write the files that differ, and the log's folder; loads nothing.
    pub fn write(&self) -> Result<Installed, Error> {
        let mut wrote = Installed::Unchanged;
        for (path, text) in self.files() {
            if fs::read_to_string(&path).is_ok_and(|old| old == text) {
                continue;
            }
            if let Some(dir) = path.parent() {
                fs::create_dir_all(dir).map_err(|e| Error::Io(dir.to_owned(), e))?;
            }
            fs::write(&path, text).map_err(|e| Error::Io(path.clone(), e))?;
            wrote = Installed::Written;
        }
        if let Some(dir) = self.log_file().as_deref().and_then(Path::parent) {
            fs::create_dir_all(dir).map_err(|e| Error::Io(dir.to_owned(), e))?;
        }
        Ok(wrote)
    }

    /// Remove superseded jobs, write on a difference, load when not loaded, re-bootstrap a refused job.
    pub fn install(&self) -> Result<Installed, Error> {
        self.drop_superseded()?;
        let wrote = self.write()?;
        let live = self.probe()?;
        if live.loaded && live.refused.is_none() && wrote == Installed::Unchanged {
            return Ok(Installed::Unchanged);
        }
        if live.loaded || live.refused.is_some() {
            self.rebootstrap()?;
        } else {
            self.load()?;
        }
        Ok(match wrote {
            Installed::Unchanged => Installed::Loaded,
            done => done,
        })
    }

    /// Unload the job and remove its files.
    pub fn uninstall(&self) -> Result<Removed, Error> {
        let live = self.probe()?;
        let paths: Vec<PathBuf> = (self.files().into_iter())
            .map(|(path, _)| path)
            .filter(|path| path.exists())
            .collect();
        if paths.is_empty() && !live.loaded && live.refused.is_none() {
            return Ok(Removed::Absent);
        }
        self.each(Verb::Unload, false)?;
        self.remove(&paths)?;
        Ok(Removed::Removed)
    }

    /// Run the job: load it when not loaded, re-bootstrap it when refused.
    pub fn start(&self) -> Result<(), Error> {
        let live = self.probe()?;
        if live.refused.is_some() {
            self.rebootstrap()
        } else if live.loaded {
            self.each(Verb::Start, true)
        } else {
            self.load()
        }
    }

    /// Stop the job when it is loaded; its files stay.
    pub fn stop(&self) -> Result<(), Error> {
        if self.probe()?.loaded {
            self.each(Verb::Stop, true)?;
        }
        Ok(())
    }

    /// Restart the job, re-bootstrapping it when refused, not loaded, or the restart fails.
    pub fn restart(&self) -> Result<(), Error> {
        let live = self.probe()?;
        if !live.loaded || live.refused.is_some() || self.each(Verb::Restart, true).is_err() {
            return self.rebootstrap();
        }
        Ok(())
    }

    /// The files against what the job renders, and whether it is loaded, its pid and a refusal.
    pub fn status(&self) -> Result<Status, Error> {
        let files: Vec<bool> = (self.files().into_iter())
            .filter_map(|(path, text)| fs::read_to_string(path).ok().map(|old| old == text))
            .collect();
        let files = match files.len() {
            0 => Files::Missing,
            n if n == self.files().len() && files.iter().all(|same| *same) => Files::Same,
            _ => Files::Differs,
        };
        Ok(Status {
            files,
            ..self.probe()?
        })
    }

    /// The load, unload, restart and log lines for a person to run.
    #[must_use]
    pub fn commands(&self) -> Commands {
        let line = |verb| {
            let cmds: Vec<String> = self.cmds(verb).iter().map(|c| c.join(" ")).collect();
            cmds.join(" && ")
        };
        let log = match self.log_file() {
            Some(file) => format!("tail -f {}", file.display()),
            None => format!("journalctl --user -u {}.service -f", self.job.label),
        };
        Commands {
            load: line(Verb::Load),
            unload: line(Verb::Unload),
            restart: line(Verb::Restart),
            log,
        }
    }

    fn plist(&self, label: &str) -> PathBuf {
        let agents = self.env.home.join("Library/LaunchAgents");
        agents.join(format!("{label}.plist"))
    }

    fn units(&self) -> PathBuf {
        self.env.config_home.join("systemd/user")
    }

    fn timer(&self) -> bool {
        !matches!(self.job.schedule, Schedule::KeepAlive(_))
    }

    fn log_file(&self) -> Option<PathBuf> {
        match (&self.job.log, self.env.platform) {
            (Log::Default, Platform::Systemd) => None,
            _ => Some(render::launchd::log(&self.job, &self.env.home)),
        }
    }

    fn cmds(&self, verb: Verb) -> Vec<Vec<String>> {
        let label = self.job.label.as_str();
        let words = |tool: &str, args: &[&str]| {
            let all = std::iter::once(tool).chain(args.iter().copied());
            all.map(str::to_owned).collect::<Vec<_>>()
        };
        if self.env.platform == Platform::Launchd {
            let domain = format!("gui/{}", self.env.uid);
            let target = format!("{domain}/{label}");
            let plist = self.plist(label).to_string_lossy().into_owned();
            let args: &[&str] = match verb {
                Verb::Probe => &["print", &target],
                Verb::Load => &["bootstrap", &domain, &plist],
                Verb::Unload | Verb::Stop => &["bootout", &target],
                Verb::Start => &["kickstart", &target],
                Verb::Restart => &["kickstart", "-k", &target],
            };
            return vec![words("launchctl", args)];
        }
        let service = format!("{label}.service");
        let timer = format!("{label}.timer");
        let unit = if self.timer() { &timer } else { &service };
        let show = ["show", "-p", "ActiveState", "-p", "MainPID", "-p", "Result"];
        let mut probe = [&show[..], &[&service]].concat();
        if self.timer() {
            probe.push(&timer);
        }
        let each: Vec<Vec<&str>> = match verb {
            Verb::Probe => vec![probe],
            Verb::Load => vec![vec!["daemon-reload"], vec!["enable", "--now", unit]],
            Verb::Unload => vec![vec!["disable", "--now", unit]],
            Verb::Start => vec![vec!["start", unit]],
            Verb::Stop => vec![vec!["stop", unit]],
            Verb::Restart => vec![vec!["restart", &service]],
        };
        let user = |args: Vec<&str>| words("systemctl", &[&["--user"], &args[..]].concat());
        each.into_iter().map(user).collect()
    }

    fn run(&self, cmd: &[String]) -> Result<Said, Error> {
        let Some((tool, args)) = cmd.split_first() else {
            return Ok(Said {
                ok: true,
                text: String::new(),
            });
        };
        ctl::run(&self.env.tools, tool, args)
    }

    fn each(&self, verb: Verb, strict: bool) -> Result<(), Error> {
        for cmd in self.cmds(verb) {
            let said = self.run(&cmd)?;
            if strict && !said.ok {
                return Err(Error::Ctl(cmd.join(" "), said.text.trim().to_owned()));
            }
        }
        Ok(())
    }

    fn load(&self) -> Result<(), Error> {
        let mut wait = self.env.backoff;
        for _ in 1..3 {
            if self.each(Verb::Load, true).is_ok() {
                return Ok(());
            }
            ctl::wait(wait);
            wait = wait.saturating_mul(2);
        }
        self.each(Verb::Load, true)
    }

    fn rebootstrap(&self) -> Result<(), Error> {
        self.each(Verb::Unload, false)?;
        self.load()
    }

    fn remove(&self, paths: &[PathBuf]) -> Result<(), Error> {
        for path in paths {
            fs::remove_file(path).map_err(|e| Error::Io(path.clone(), e))?;
        }
        if self.env.platform == Platform::Systemd && !paths.is_empty() {
            self.run(&["systemctl", "--user", "daemon-reload"].map(str::to_owned))?;
        }
        Ok(())
    }

    fn drop_superseded(&self) -> Result<(), Error> {
        let names = self.job.supersedes.iter();
        for name in names.filter(|n| !n.is_empty() && !n.contains('/')) {
            let paths = match self.env.platform {
                Platform::Launchd => vec![self.plist(name)],
                Platform::Systemd => (["timer", "service"].iter())
                    .map(|ext| self.units().join(format!("{name}.{ext}")))
                    .collect(),
            };
            let paths: Vec<PathBuf> = paths.into_iter().filter(|p| p.exists()).collect();
            let units = paths.iter().filter_map(|p| p.file_name()?.to_str());
            let cmd: Vec<String> = match self.env.platform {
                _ if paths.is_empty() => continue,
                Platform::Launchd => vec![
                    "launchctl".into(),
                    "bootout".into(),
                    format!("gui/{}/{name}", self.env.uid),
                ],
                Platform::Systemd => (["systemctl", "--user", "disable", "--now"].into_iter())
                    .chain(units)
                    .map(str::to_owned)
                    .collect(),
            };
            self.run(&cmd)?;
            self.remove(&paths)?;
        }
        Ok(())
    }

    fn probe(&self) -> Result<Status, Error> {
        let mut live = ctl::Live::default();
        for cmd in self.cmds(Verb::Probe) {
            let said = self.run(&cmd)?;
            live = match self.env.platform {
                Platform::Launchd => ctl::Live::launchd(&said),
                Platform::Systemd if said.ok => ctl::Live::systemd(&said.text, self.timer()),
                Platform::Systemd => {
                    return Err(Error::Ctl(cmd.join(" "), said.text.trim().to_owned()));
                }
            };
        }
        Ok(Status {
            files: Files::Missing,
            loaded: live.loaded,
            pid: live.pid,
            refused: live.refused,
        })
    }
}

impl fmt::Display for Installed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Unchanged => "unchanged",
            Self::Written => "written",
            Self::Loaded => "loaded",
        })
    }
}

impl fmt::Display for Removed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Absent => "absent",
            Self::Removed => "removed",
        })
    }
}
