#[path = "gen/skill.rs"]
mod files;
mod skill;

use std::fmt;
use std::path::Path;

const USAGE: &str = "usage: farrier skill --to <dir>";

struct Failure(String);

impl fmt::Debug for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn main() -> Result<(), Failure> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [cmd, flag, dir] if cmd == "skill" && flag == "--to" => {
            skill::write(Path::new(dir)).map_err(|e| Failure(format!("farrier skill: {dir}: {e}")))
        }
        _ => Err(Failure(USAGE.to_owned())),
    }
}
