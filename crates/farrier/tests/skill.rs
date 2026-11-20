use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn farrier(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_farrier"))
        .args(args)
        .output()
        .unwrap()
}

fn files(root: &Path) -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                dirs.push(path);
            } else {
                let text = fs::read_to_string(&path).unwrap();
                out.push((path.strip_prefix(root).unwrap().to_path_buf(), text));
            }
        }
    }
    out.sort();
    out
}

fn fresh(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    if dir.exists() {
        fs::remove_dir_all(&dir).unwrap();
    }
    dir
}

#[test]
fn skill_writes_the_repo_skill_folder_and_a_rerun_changes_nothing() {
    let to = fresh("skill-to");
    let skill = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skill/farrier");
    for _ in 0..2 {
        let out = farrier(&["skill", "--to", to.to_str().unwrap()]);
        assert!(out.status.success(), "{out:?}");
        assert!(out.stdout.is_empty() && out.stderr.is_empty(), "{out:?}");
        assert_eq!(files(&to.join("farrier")), files(&skill));
    }
    assert!(to.join("farrier/SKILL.md").is_file());
}

#[test]
fn skill_refuses_a_missing_target_with_the_usage() {
    for args in [
        &["skill"][..],
        &["skill", "--to"],
        &["skill", "--into", "x"],
        &[],
    ] {
        let out = farrier(args);
        assert!(!out.status.success());
        assert!(String::from_utf8_lossy(&out.stderr).contains("usage: farrier skill --to <dir>"));
    }
}

#[test]
fn skill_reports_a_target_it_cannot_write() {
    let to = fresh("skill-blocked");
    fs::create_dir_all(&to).unwrap();
    fs::write(to.join("farrier"), "a file, not a folder").unwrap();
    let out = farrier(&["skill", "--to", to.to_str().unwrap()]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("farrier skill: "));
}
