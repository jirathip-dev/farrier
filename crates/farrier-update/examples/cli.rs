use std::error::Error;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use farrier_update::{Channel, Install, Outcome, Product, Updater};

const FX: Product = Product {
    bin: "fx",
    repo: "example/fx",
    keys: &[
        "untrusted comment: fx release key\nRWQf6LRCGA9i59SLOFxz6NxvASXDJeRtuZykwQepbDEGt87ig1BNpWaE\n",
    ],
    codesign_id: Some("fx"),
};

fn self_update(dry_run: bool) -> Result<Outcome, Box<dyn Error>> {
    let home = PathBuf::from(std::env::var_os("HOME").ok_or("HOME is not set")?);
    let install = Install {
        exe: std::env::current_exe()?,
        state: home.join(".local/state/fx/update"),
    };
    let updater = Updater::new(&FX, install, Channel::Main);
    Ok(if dry_run {
        updater.plan()?
    } else {
        updater.run()?
    })
}

fn main() -> ExitCode {
    let dry_run = std::env::args().any(|a| a == "--dry-run");
    match self_update(dry_run) {
        Ok(outcome) => {
            writeln!(io::stdout(), "{outcome}").ok();
            outcome.exit()
        }
        Err(e) => {
            writeln!(io::stderr(), "fx self-update: {e}").ok();
            ExitCode::FAILURE
        }
    }
}
