use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use blake2::{Blake2b512, Digest};
use ed25519_compact::{KeyPair, Seed};
use farrier_update::id::{Os, Target};

const REPO: &str = "example/fx";
const TAG: &str = "build-0123456";
const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ABC[(n >> (18 - 6 * i) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

struct Key {
    pair: KeyPair,
    id: [u8; 8],
}

impl Key {
    fn new(n: u8) -> Self {
        Self {
            pair: KeyPair::from_seed(Seed::new([n; 32])),
            id: [n; 8],
        }
    }

    fn public(&self) -> String {
        let bin = [b"Ed".as_slice(), &self.id, self.pair.pk.as_ref()].concat();
        format!("untrusted comment: test key\n{}\n", base64(&bin))
    }

    fn sign(&self, data: &[u8], comment: &str) -> String {
        let sig = self.pair.sk.sign(Blake2b512::digest(data), None);
        let bin = [b"ED".as_slice(), &self.id, sig.as_ref()].concat();
        let global = self
            .pair
            .sk
            .sign([sig.as_ref(), comment.as_bytes()].concat(), None);
        format!(
            "untrusted comment: test signature\n{}\ntrusted comment: {comment}\n{}\n",
            base64(&bin),
            base64(global.as_ref())
        )
    }
}

fn field(case: &str, key: &str) -> Option<String> {
    case.lines().find_map(|line| {
        let (k, v) = line.split_once(" = ")?;
        (k == key).then(|| v.trim_matches('"').to_owned())
    })
}

fn copy(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let path = entry.unwrap().path();
        fs::copy(&path, to.join(path.file_name().unwrap())).unwrap();
    }
}

fn expected(reject: &str) -> &str {
    let mac = Target::host().is_some_and(|t| t.os == Os::MacOs);
    if mac && matches!(reject, "Version" | "pass") {
        "CodeSign"
    } else {
        reject
    }
}

#[test]
fn every_fixture_release_gets_its_reject() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/releases");
    let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("releases");
    let _ = fs::remove_dir_all(&tmp);
    let (release, backup, other) = (Key::new(1), Key::new(2), Key::new(3));
    let keys = tmp.join("keys");
    fs::create_dir_all(&keys).unwrap();
    fs::write(keys.join("release.pub"), release.public()).unwrap();
    fs::write(keys.join("backup.pub"), backup.public()).unwrap();
    let mut seen = Vec::new();
    for entry in fs::read_dir(&root).unwrap() {
        let case_dir = entry.unwrap().path();
        let name = case_dir.file_name().unwrap().to_str().unwrap().to_owned();
        let case = fs::read_to_string(case_dir.join("case.toml")).unwrap();
        let dir = tmp.join(&name);
        copy(&case_dir, &dir);
        fs::remove_file(dir.join("case.toml")).unwrap();
        let signer = match field(&case, "key").unwrap().as_str() {
            "release" => Some(&release),
            "backup" => Some(&backup),
            "other" => Some(&other),
            _ => None,
        };
        let tag = field(&case, "tag").unwrap_or(TAG.to_owned());
        if let Some(key) = signer {
            let comment = field(&case, "signed").unwrap_or(format!("{REPO} {tag} {SHA}"));
            let sums = fs::read(dir.join("SHA256SUMS")).unwrap();
            fs::write(dir.join("SHA256SUMS.minisig"), key.sign(&sums, &comment)).unwrap();
        }
        let out = Command::new(env!("CARGO_BIN_EXE_farrier"))
            .args(["release", "verify"])
            .arg(&dir)
            .args(["--repo", REPO, "--tag", &tag, "--sha", SHA, "--bin", "fx"])
            .arg("--key")
            .arg(keys.join("release.pub"))
            .arg("--key")
            .arg(keys.join("backup.pub"))
            .output()
            .unwrap();
        let err = String::from_utf8_lossy(&out.stderr);
        let reject = field(&case, "reject").unwrap();
        let want = expected(&reject);
        if want == "pass" {
            assert!(out.status.success(), "{name}: {err}");
        } else {
            assert!(!out.status.success(), "{name} passed, wants {want}");
            assert!(err.contains(&format!(": {want}: ")), "{name}: {err}");
        }
        seen.push(name);
    }
    seen.sort();
    assert_eq!(
        seen,
        [
            "another-key",
            "another-release",
            "backup-key",
            "extra-line",
            "missing-asset",
            "no-signature",
            "other-build",
            "release-key",
            "version-tag",
            "version-tag-other",
            "wrong-checksum"
        ]
    );
}

#[test]
fn bad_arguments_are_refused() {
    let out = Command::new(env!("CARGO_BIN_EXE_farrier"))
        .args(["release", "verify", ".", "--repo", REPO, "--tag", TAG])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("usage: farrier release verify"));
}
