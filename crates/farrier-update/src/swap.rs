use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

fn beside(exe: &Path, ext: &str) -> PathBuf {
    let mut path = OsString::from(exe);
    path.push(ext);
    path.into()
}

/// `<exe>.new`, the staged file.
pub fn new(exe: &Path) -> PathBuf {
    beside(exe, ".new")
}

/// `<exe>.prev`, the build before the last swap.
pub fn prev(exe: &Path) -> PathBuf {
    beside(exe, ".prev")
}

/// Links the live file as `.prev`, then renames `.new` over it: the one rename that changes the live file.
pub fn swap(exe: &Path) -> io::Result<()> {
    let prev = prev(exe);
    if fs::symlink_metadata(&prev).is_ok() {
        fs::remove_file(&prev)?;
    }
    if exe.exists() {
        fs::hard_link(exe, &prev)?;
    }
    fs::rename(new(exe), exe)
}

/// Renames `.prev` back over the live file.
pub fn back(exe: &Path) -> io::Result<()> {
    fs::rename(prev(exe), exe)
}
