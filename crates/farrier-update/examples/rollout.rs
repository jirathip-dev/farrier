use std::error::Error;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::{Command, ExitCode};

use farrier_update::phase::{self, Phase, Reply};
use farrier_update::{Build, Channel, Install, Product, Updater};

const FX: Product = Product {
    bin: "fx",
    repo: "example/fx",
    keys: &[
        "untrusted comment: fx release key\nRWQf6LRCGA9i59SLOFxz6NxvASXDJeRtuZykwQepbDEGt87ig1BNpWaE\n",
    ],
    codesign_id: Some("fx"),
};

fn build(sha: &str) -> Result<Build, String> {
    Build::parse(sha).ok_or_else(|| format!("{sha} is no build"))
}

fn on(host: &str, phase: &str, build: &Build) -> Result<Reply, Box<dyn Error>> {
    let out = Command::new("ssh")
        .args([host, "fx", "self-update", "--phase", phase, build.as_str()])
        .output()?;
    Ok(phase::parse(String::from_utf8_lossy(&out.stdout).trim())?)
}

fn rollout(updater: &Updater, head: &Build, hosts: &[String]) -> Result<ExitCode, Box<dyn Error>> {
    writeln!(io::stdout(), "main: {}", updater.stage(head)?)?;
    for host in hosts {
        let staged = on(host, "stage", head)?.builds.staged;
        if staged.as_ref() != Some(head) {
            writeln!(io::stdout(), "{host}: not staged; it catches up next head")?;
        }
    }
    for host in hosts {
        if let Some(outcome) = on(host, "swap", head)?.outcome {
            writeln!(io::stdout(), "{host}: {outcome}")?;
        }
    }
    let outcome = updater.swap(head)?;
    writeln!(io::stdout(), "main: {outcome}")?;
    Ok(outcome.exit())
}

fn main() -> Result<ExitCode, Box<dyn Error>> {
    let home = PathBuf::from(std::env::var_os("HOME").ok_or("HOME is not set")?);
    let install = Install {
        exe: std::env::current_exe()?,
        state: home.join(".local/state/fx/update"),
    };
    let updater = Updater::new(&FX, install, Channel::Main);
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [verb, step, sha] if verb == "--phase" => {
            let step = match step.as_str() {
                "stage" => Phase::Stage,
                "swap" => Phase::Swap,
                _ => Phase::Builds,
            };
            writeln!(io::stdout(), "{}", phase::run(&updater, step, &build(sha)?))?;
            Ok(ExitCode::SUCCESS)
        }
        [sha, hosts @ ..] => rollout(&updater, &build(sha)?, hosts),
        [] => {
            Err("usage: rollout <sha> <host>... | rollout --phase stage|swap|builds <sha>".into())
        }
    }
}
