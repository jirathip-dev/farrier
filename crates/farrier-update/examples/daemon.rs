use std::error::Error;
use std::io::{self, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::ExitCode;

use farrier_service::{Env, Job, Restart, Schedule, Service};
use farrier_update::{Build, Channel, Install, Outcome, Product, Supervised, Updater};

const FX: Product = Product {
    bin: "fx",
    repo: "example/fx",
    keys: &[
        "untrusted comment: fx release key\nRWQf6LRCGA9i59SLOFxz6NxvASXDJeRtuZykwQepbDEGt87ig1BNpWaE\n",
    ],
    codesign_id: Some("fx"),
};

fn self_update() -> Result<Outcome, Box<dyn Error>> {
    let exe = std::env::current_exe()?;
    let home = PathBuf::from(std::env::var_os("HOME").ok_or("HOME is not set")?);
    let job = Job::new(
        "dev.example.fx",
        [exe.as_os_str(), "serve".as_ref()],
        Schedule::KeepAlive(Restart::Always),
    )?;
    let service = Service::new(job, &Env::host()?);
    let serves = Supervised::new(&service, |_: &Build| {
        TcpStream::connect(("127.0.0.1", 7070)).is_ok()
    });
    let install = Install {
        exe,
        state: home.join(".local/state/fx/update"),
    };
    let updater = Updater::new(&FX, install, Channel::Main).daemon(&serves);
    Ok(updater.run()?)
}

fn main() -> ExitCode {
    match self_update() {
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
