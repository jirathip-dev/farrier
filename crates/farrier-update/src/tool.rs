use std::path::Path;
use std::process::{Command, Output};

fn run(cmd: &mut Command, what: &str) -> Result<Output, String> {
    let out = cmd.output().map_err(|e| format!("{what}: {e}"))?;
    if out.status.success() {
        Ok(out)
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(format!("{what}: {}: {}", out.status, err.trim()))
    }
}

/// The stdout of `<exe> --version`.
pub fn version(exe: &Path) -> Result<String, String> {
    let what = format!("{} --version", exe.display());
    let out = run(Command::new(exe).arg("--version"), &what)?;
    String::from_utf8(out.stdout).map_err(|_| format!("{what}: not UTF-8"))
}

/// Checks `exe`'s code signature: a Developer ID Application certificate for identifier `id`.
pub fn codesign(exe: &Path, id: &str) -> Result<(), String> {
    let req = format!(
        "=anchor apple generic and identifier \"{id}\" and certificate 1[field.1.2.840.113635.100.6.2.6] exists and certificate leaf[field.1.2.840.113635.100.6.1.13] exists"
    );
    let what = format!("codesign --verify {}", exe.display());
    run(
        Command::new("codesign")
            .args(["--verify", "--strict", "-R", &req])
            .arg(exe),
        &what,
    )
    .map(drop)
}
