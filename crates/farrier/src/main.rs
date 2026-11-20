#[path = "gen/skill.rs"]
mod files;
mod release;
mod skill;

use std::fmt;
use std::io::{self, Write as _};
use std::path::Path;

const USAGE: &str = "usage: farrier skill --to <dir> | farrier --version";

struct Failure(String);

impl fmt::Debug for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn version() -> String {
    let semver = env!("CARGO_PKG_VERSION");
    match option_env!("FARRIER_BUILD") {
        Some(build) => format!("farrier {semver} ({build})"),
        None => format!("farrier {semver}"),
    }
}

fn main() -> Result<(), Failure> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [cmd, flag, dir] if cmd == "skill" && flag == "--to" => {
            skill::write(Path::new(dir)).map_err(|e| Failure(format!("farrier skill: {dir}: {e}")))
        }
        [cmd, verb, rest @ ..] if cmd == "release" && verb == "verify" => {
            release::verify(rest).map_err(|e| Failure(format!("farrier release verify: {e}")))
        }
        [flag] if flag == "--version" => writeln!(io::stdout(), "{}", version())
            .map_err(|e| Failure(format!("farrier --version: {e}"))),
        _ => Err(Failure(format!(
            "{USAGE} | {}",
            release::USAGE.trim_start_matches("usage: ")
        ))),
    }
}
