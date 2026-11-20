use std::fs::{self, File};
use std::os::unix::fs::PermissionsExt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Duration;

use blake2::{Blake2b512, Digest};
use ed25519_compact::{KeyPair, Seed};
use farrier_service::{Env, Job, Platform, Restart, Schedule, Service};
use farrier_update::{
    Build, Channel, Daemon, Error, Install, Limits, NoDaemon, Os, Outcome, Product, Reject,
    Restarted, Resting, Staged, Supervised, Target, Updater, fetch, phase,
};

const A: &str = "aaaaaaa000000000000000000000000000000000";
const B: &str = "bbbbbbb000000000000000000000000000000000";
const C: &str = "ccccccc000000000000000000000000000000000";

const GIT: &str = "#!/bin/sh\n[ -f @DIR@/head ] && printf '%s\\trefs/heads/main\\n' \"$(cat @DIR@/head)\"\nexit 0\n";
const CURL: &str = r#"#!/bin/sh
for a; do url=$a; done
[ -f @DIR@/offline ] && { echo "curl: (6) Could not resolve host" >&2; exit 6; }
f="@DIR@/rel/${url#https://github.com/example/fx/releases/download/}"
[ -f "$f" ] || exit 22
cat "$f"
"#;
const CODESIGN: &str = "#!/bin/sh\n[ ! -f @DIR@/unsigned ] || { echo 'not signed' >&2; exit 1; }\n";

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

struct Key(KeyPair, [u8; 8]);

impl Key {
    fn new(n: u8) -> Self {
        Self(KeyPair::from_seed(Seed::new([n; 32])), [n; 8])
    }

    fn public(&self) -> String {
        let bin = [b"Ed".as_slice(), &self.1, self.0.pk.as_ref()].concat();
        format!("untrusted comment: test key\n{}\n", base64(&bin))
    }

    fn sign(&self, data: &[u8], comment: &str) -> String {
        let sig = self.0.sk.sign(Blake2b512::digest(data), None);
        let bin = [b"ED".as_slice(), &self.1, sig.as_ref()].concat();
        let global = self
            .0
            .sk
            .sign([sig.as_ref(), comment.as_bytes()].concat(), None);
        format!(
            "untrusted comment: test signature\n{}\ntrusted comment: {comment}\n{}\n",
            base64(&bin),
            base64(global.as_ref())
        )
    }
}

fn product() -> &'static Product {
    let key: &'static str = Box::leak(Key::new(1).public().into_boxed_str());
    Box::leak(Box::new(Product {
        bin: "fx",
        repo: "example/fx",
        keys: Box::leak(Box::new([key])),
        codesign_id: Some("fx"),
    }))
}

fn build(sha: &str) -> Build {
    Build::parse(sha).unwrap()
}

fn script(sha: &str) -> String {
    format!("#!/bin/sh\necho \"fx 0.1.0 ({sha})\"\n")
}

fn write_exe(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn sha256(bytes: &[u8]) -> String {
    let digest = sha2::Sha256::digest(bytes);
    let hex = digest.iter().flat_map(|b| [b >> 4, b & 15]);
    hex.map(|n| char::from_digit(u32::from(n), 16).unwrap())
        .collect()
}

#[derive(Default)]
struct Release {
    key: Option<u8>,
    comment: Option<String>,
    listed: Option<String>,
    body: Option<String>,
    asset: Option<String>,
}

struct Host {
    dir: PathBuf,
}

impl Host {
    fn new(name: &str) -> Option<Self> {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("update")
            .join(name);
        if std::env::var_os("FARRIER_STUBS").is_none() {
            let out = Command::new(std::env::current_exe().unwrap())
                .args([name, "--exact", "--nocapture", "--test-threads=1"])
                .env(
                    "PATH",
                    format!("{}:/usr/bin:/bin", dir.join("bin").display()),
                )
                .env("FARRIER_STUBS", "1")
                .output()
                .unwrap();
            let text = String::from_utf8_lossy(&out.stdout);
            assert!(
                out.status.success() && text.contains("1 passed"),
                "{text}{}",
                String::from_utf8_lossy(&out.stderr)
            );
            return None;
        }
        let _ = fs::remove_dir_all(&dir);
        for (tool, text) in [("git", GIT), ("curl", CURL), ("codesign", CODESIGN)] {
            write_exe(
                &dir.join("bin").join(tool),
                &text.replace("@DIR@", &dir.display().to_string()),
            );
        }
        let host = Self { dir };
        write_exe(&host.exe(), &script(A));
        Some(host)
    }

    fn exe(&self) -> PathBuf {
        self.dir.join("app/fx")
    }

    fn file(&self, ext: &str) -> PathBuf {
        self.dir.join(format!("app/fx{ext}"))
    }

    fn install(&self) -> Install {
        Install {
            exe: self.exe(),
            state: self.dir.join("state"),
        }
    }

    fn updater<'a>(&self, daemon: &'a dyn Daemon) -> Updater<'a> {
        Updater::new(product(), self.install(), Channel::Main)
            .daemon(daemon)
            .limits(Limits {
                download: Duration::from_secs(5),
                probe: Duration::from_secs(5),
                confirm: Duration::ZERO,
            })
    }

    fn touch(&self, name: &str, text: &str) {
        fs::write(self.dir.join(name), text).unwrap();
    }

    fn head(&self, sha: &str) {
        self.touch("head", sha);
    }

    fn release(&self, sha: &str, r: &Release) {
        let tag = format!("build-{}", &sha[..7]);
        let dir = self.dir.join("rel").join(&tag);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let asset = Target::host().unwrap().asset("fx");
        let body = r.body.clone().unwrap_or_else(|| script(sha));
        let name = r.asset.clone().unwrap_or(asset);
        fs::write(dir.join(&name), &body).unwrap();
        let listed = r.listed.clone().unwrap_or(body);
        let sums = format!("{}  {name}\n", sha256(listed.as_bytes()));
        fs::write(dir.join("SHA256SUMS"), &sums).unwrap();
        let comment = (r.comment.clone()).unwrap_or(format!("example/fx {tag} {sha}"));
        let sig = Key::new(r.key.unwrap_or(1)).sign(sums.as_bytes(), &comment);
        fs::write(dir.join("SHA256SUMS.minisig"), sig).unwrap();
    }

    fn live(&self) -> String {
        fs::read_to_string(self.exe()).unwrap()
    }
}

struct Fake {
    admit: Result<(), String>,
    restart: Result<Restarted, String>,
    serving: &'static str,
}

impl Fake {
    fn serving(sha: &'static str) -> Self {
        Self {
            admit: Ok(()),
            restart: Ok(Restarted::Now),
            serving: sha,
        }
    }
}

impl Daemon for Fake {
    fn admit(&self, _: &Staged) -> Result<(), String> {
        self.admit.clone()
    }

    fn restart(&self, _: &Build) -> Result<Restarted, String> {
        self.restart.clone()
    }

    fn serves(&self, to: &Build) -> bool {
        to.matches(&build(self.serving))
    }
}

struct Dies;

impl Daemon for Dies {
    fn restart(&self, _: &Build) -> Result<Restarted, String> {
        panic!("the pass dies after the swap")
    }

    fn serves(&self, _: &Build) -> bool {
        false
    }
}

fn updated(from: &str, to: &str) -> Outcome {
    Outcome::Updated {
        from: Some(build(from)),
        to: build(to),
    }
}

#[test]
fn run_stages_verifies_swaps_and_keeps_the_previous_build() {
    let Some(h) = Host::new("run_stages_verifies_swaps_and_keeps_the_previous_build") else {
        return;
    };
    h.head(B);
    h.release(B, &Release::default());
    let up = h.updater(&NoDaemon);
    assert_eq!(up.run().unwrap(), updated(A, B));
    assert_eq!(h.live(), script(B));
    assert_eq!(fs::read_to_string(h.file(".prev")).unwrap(), script(A));
    assert!(!h.file(".new").exists());
    let st = up.status();
    assert_eq!((st.resting, st.ok), (Resting::Idle, true));
    assert_eq!(st.last, format!("updated {A} -> {B}"));
    assert_eq!(up.run().unwrap(), Outcome::Current(build(B)));
    assert_eq!(up.run().unwrap().exit(), ExitCode::SUCCESS);
}

#[test]
fn run_waits_for_a_head_and_for_its_release() {
    let Some(h) = Host::new("run_waits_for_a_head_and_for_its_release") else {
        return;
    };
    let up = h.updater(&NoDaemon);
    assert!(matches!(up.run().unwrap(), Outcome::Waiting { .. }));
    h.head(B);
    let Outcome::Waiting { tag, .. } = up.run().unwrap() else {
        panic!("not waiting")
    };
    assert_eq!(tag, "build-bbbbbbb");
    assert_eq!(h.live(), script(A));
}

#[test]
fn every_reject_removes_the_staged_file_and_leaves_the_live_one() {
    let Some(h) = Host::new("every_reject_removes_the_staged_file_and_leaves_the_live_one") else {
        return;
    };
    let mac = Target::host().unwrap().os == Os::MacOs;
    let cases = [
        (
            "Signature",
            Release {
                key: Some(3),
                ..Release::default()
            },
        ),
        (
            "Signature",
            Release {
                comment: Some(format!("example/fx build-ccccccc {C}")),
                ..Release::default()
            },
        ),
        (
            "Checksum",
            Release {
                listed: Some("other".into()),
                ..Release::default()
            },
        ),
        (
            "NoAsset",
            Release {
                asset: Some("fx-sparc-unknown-none".into()),
                ..Release::default()
            },
        ),
        (
            "Version",
            Release {
                body: Some(script(C)),
                ..Release::default()
            },
        ),
        ("CodeSign", Release::default()),
    ];
    h.head(B);
    for (want, release) in cases {
        if want == "CodeSign" {
            if !mac {
                continue;
            }
            h.touch("unsigned", "");
        }
        let _ = fs::remove_dir_all(h.dir.join("state"));
        h.release(B, &release);
        let up = h.updater(&NoDaemon);
        let Outcome::Rejected(reject) = up.run().unwrap() else {
            panic!("{want}: not rejected")
        };
        assert_eq!(reject.parts().0, want, "{reject}");
        assert!(!reject.retry());
        assert!(!h.file(".new").exists(), "{want}");
        assert_eq!(h.live(), script(A), "{want}");
        let next = up.run().unwrap();
        if want == "NoAsset" {
            assert!(matches!(next, Outcome::Rejected(Reject::NoAsset(_))));
        } else {
            assert!(matches!(next, Outcome::Skipped { .. }), "{want}: {next}");
            assert_eq!(next.exit(), ExitCode::FAILURE);
        }
    }
}

#[test]
fn a_download_failure_is_retried_never_marked_failed() {
    let Some(h) = Host::new("a_download_failure_is_retried_never_marked_failed") else {
        return;
    };
    h.head(B);
    h.release(B, &Release::default());
    h.touch("offline", "");
    let up = h.updater(&NoDaemon);
    let Outcome::Rejected(reject) = up.run().unwrap() else {
        panic!("not rejected")
    };
    assert!(reject.retry(), "{reject}");
    assert_eq!(up.status().failed, None);
    fs::remove_file(h.dir.join("offline")).unwrap();
    assert_eq!(up.run().unwrap(), updated(A, B));
}

#[test]
fn a_deferred_build_rests_staged_and_a_later_pass_swaps_it_without_a_download() {
    let Some(h) =
        Host::new("a_deferred_build_rests_staged_and_a_later_pass_swaps_it_without_a_download")
    else {
        return;
    };
    h.head(B);
    h.release(B, &Release::default());
    let busy = Fake {
        admit: Err("a lane is mid-handover".into()),
        ..Fake::serving(B)
    };
    let up = h.updater(&busy);
    let deferred = up.run().unwrap();
    assert_eq!(
        deferred,
        Outcome::Deferred {
            build: build(B),
            why: "a lane is mid-handover".into()
        }
    );
    assert_eq!(up.builds().staged, Some(build(B)));
    assert_eq!(h.live(), script(A));
    fs::remove_dir_all(h.dir.join("rel")).unwrap();
    assert_eq!(h.updater(&NoDaemon).run().unwrap(), updated(A, B));
}

#[test]
fn a_later_restart_rests_swapped_and_the_next_pass_confirms() {
    let Some(h) = Host::new("a_later_restart_rests_swapped_and_the_next_pass_confirms") else {
        return;
    };
    h.head(B);
    h.release(B, &Release::default());
    let later = Fake {
        restart: Ok(Restarted::Later("serve is busy".into())),
        ..Fake::serving(B)
    };
    let pending = h.updater(&later).run().unwrap();
    assert_eq!(
        pending,
        Outcome::Pending {
            build: build(B),
            why: "serve is busy".into()
        }
    );
    assert!(matches!(
        h.updater(&later).status().resting,
        Resting::Swapped { .. }
    ));
    assert_eq!(h.live(), script(B));
    assert_eq!(h.updater(&Fake::serving(B)).run().unwrap(), updated(A, B));
}

#[test]
fn a_build_that_does_not_serve_is_rolled_back_and_skipped_until_the_head_moves() {
    let Some(h) =
        Host::new("a_build_that_does_not_serve_is_rolled_back_and_skipped_until_the_head_moves")
    else {
        return;
    };
    h.head(B);
    h.release(B, &Release::default());
    let old = Fake::serving(A);
    let up = h.updater(&old);
    let back = up.run().unwrap();
    let Outcome::RolledBack { to, serving, .. } = &back else {
        panic!("{back}")
    };
    assert_eq!((to, *serving), (&build(B), true));
    assert_eq!(back.exit(), ExitCode::FAILURE);
    assert_eq!(h.live(), script(A));
    assert_eq!(up.status().failed, Some(build(B)));
    assert!(matches!(up.run().unwrap(), Outcome::Skipped { .. }));
    h.head(C);
    h.release(C, &Release::default());
    assert_eq!(h.updater(&Fake::serving(C)).run().unwrap(), updated(A, C));
}

#[test]
fn a_supervised_service_that_fails_to_restart_is_rolled_back() {
    let Some(h) = Host::new("a_supervised_service_that_fails_to_restart_is_rolled_back") else {
        return;
    };
    h.head(B);
    h.release(B, &Release::default());
    let env = Env {
        platform: Platform::Launchd,
        home: h.dir.join("home"),
        config_home: h.dir.join("home/.config"),
        uid: 501,
        tools: h.dir.join("no-tools").into(),
        backoff: Duration::ZERO,
    };
    let job = Job::new(
        "dev.example.fx",
        [h.exe().into_os_string(), "serve".into()],
        Schedule::KeepAlive(Restart::Always),
    )
    .unwrap();
    let service = Service::new(job, &env);
    let supervised = Supervised::new(&service, |_: &Build| true);
    let back = h.updater(&supervised).run().unwrap();
    let Outcome::RolledBack { why, serving, .. } = &back else {
        panic!("{back}")
    };
    assert!(why.contains("launchctl"), "{why}");
    assert!(!serving);
    assert_eq!(h.live(), script(A));
}

#[test]
fn a_pass_that_dies_after_the_swap_resumes_from_state_json() {
    let Some(h) = Host::new("a_pass_that_dies_after_the_swap_resumes_from_state_json") else {
        return;
    };
    h.head(B);
    h.release(B, &Release::default());
    let died = catch_unwind(AssertUnwindSafe(|| h.updater(&Dies).run()));
    assert!(died.is_err());
    assert_eq!(h.live(), script(B));
    let json = fs::read_to_string(h.dir.join("state/state.json")).unwrap();
    assert!(json.contains(r#""resting":"swapped""#), "{json}");
    assert_eq!(h.updater(&NoDaemon).run().unwrap(), updated(A, B));
    assert_eq!(fs::read_to_string(h.file(".prev")).unwrap(), script(A));
}

#[test]
fn stage_and_swap_are_two_calls_that_rest_on_disk() {
    let Some(h) = Host::new("stage_and_swap_are_two_calls_that_rest_on_disk") else {
        return;
    };
    h.release(B, &Release::default());
    let up = h.updater(&NoDaemon);
    assert_eq!(up.stage(&build(B)).unwrap(), Outcome::Staged(build(B)));
    fs::remove_dir_all(h.dir.join("rel")).unwrap();
    assert_eq!(up.stage(&build(B)).unwrap(), Outcome::Staged(build(B)));
    assert!(matches!(up.swap(&build(C)), Err(Error::Refused(_))));
    assert_eq!(h.live(), script(A));
    assert_eq!(h.updater(&NoDaemon).swap(&build(B)).unwrap(), updated(A, B));
    assert_eq!(up.swap(&build(B)).unwrap(), Outcome::Current(build(B)));
}

#[test]
fn pause_stops_run_only_and_plan_changes_nothing() {
    let Some(h) = Host::new("pause_stops_run_only_and_plan_changes_nothing") else {
        return;
    };
    h.head(B);
    h.release(B, &Release::default());
    let up = h.updater(&NoDaemon);
    assert_eq!(up.plan().unwrap(), Outcome::Would(Box::new(updated(A, B))));
    assert!(!h.dir.join("state/state.json").exists() && !h.file(".new").exists());
    let Outcome::Paused { since } = up.pause().unwrap() else {
        panic!("not paused")
    };
    assert_eq!(up.pause().unwrap(), Outcome::Paused { since });
    assert_eq!(up.run().unwrap(), Outcome::Paused { since });
    assert_eq!(up.stage(&build(B)).unwrap(), Outcome::Staged(build(B)));
    assert_eq!(up.resume().unwrap(), Outcome::Resumed);
    assert_eq!(up.run().unwrap(), updated(A, B));
}

#[test]
fn a_second_pass_while_one_holds_the_lock_is_refused() {
    let Some(h) = Host::new("a_second_pass_while_one_holds_the_lock_is_refused") else {
        return;
    };
    let up = h.updater(&NoDaemon);
    up.pause().unwrap();
    let held = File::options().write(true).open(h.dir.join("state/lock"));
    let held = held
        .or_else(|_| File::create(h.dir.join("state/lock")))
        .unwrap();
    held.lock().unwrap();
    assert!(matches!(up.run(), Err(Error::Busy)));
}

#[test]
fn phase_prints_one_line_the_other_side_parses() {
    let Some(h) = Host::new("phase_prints_one_line_the_other_side_parses") else {
        return;
    };
    let up = h.updater(&NoDaemon);
    let (b, c) = (build(B), build(C));
    let line = phase::run(&up, phase::Phase::Builds, &b);
    assert!(!line.contains('\n'));
    let reply = phase::parse(&line).unwrap();
    assert_eq!((reply.outcome, reply.builds.live), (None, Some(build(A))));
    h.release(
        C,
        &Release {
            listed: Some("x\"\n\\".into()),
            ..Release::default()
        },
    );
    let rejected = phase::parse(&phase::run(&up, phase::Phase::Stage, &c)).unwrap();
    assert!(matches!(
        rejected.outcome,
        Some(Outcome::Rejected(Reject::Checksum(_)))
    ));
    h.release(B, &Release::default());
    let staged = phase::parse(&phase::run(&up, phase::Phase::Stage, &b)).unwrap();
    assert_eq!(
        (staged.outcome, staged.builds.staged),
        (Some(Outcome::Staged(b.clone())), Some(b.clone()))
    );
    let refused = phase::parse(&phase::run(&up, phase::Phase::Swap, &c));
    assert!(matches!(refused, Err(Error::Reply(why)) if why.contains("is not staged")));
    let swapped = phase::parse(&phase::run(&up, phase::Phase::Swap, &b)).unwrap();
    assert_eq!(swapped.outcome, Some(updated(A, B)));
    assert_eq!(swapped.builds.live, Some(b));
    assert!(phase::parse("{\"outcome\":\"updated\"}").is_err());
    assert!(phase::parse("not json").is_err());
}

#[test]
fn fetch_verifies_a_build_for_a_target_into_a_path() {
    let Some(h) = Host::new("fetch_verifies_a_build_for_a_target_into_a_path") else {
        return;
    };
    h.release(B, &Release::default());
    let to = h.dir.join("fx-copy");
    let host = Target::host().unwrap();
    let staged = fetch(product(), &Channel::Main, &build(B), host, &to).unwrap();
    assert_eq!((staged.path, staged.build), (to.clone(), build(B)));
    assert_eq!(fs::read_to_string(&to).unwrap(), script(B));
    let other = *Target::ALL.iter().find(|t| **t != host).unwrap();
    let missing = fetch(
        product(),
        &Channel::Main,
        &build(B),
        other,
        &h.dir.join("x"),
    );
    assert!(matches!(missing, Err(Reject::NoAsset(_))));
}

#[test]
fn farrier_update_adds_at_most_twelve_crates_to_a_lock_file() {
    let out = Command::new(env!("CARGO"))
        .args(["tree", "--locked", "--offline", "-p", "farrier-update"])
        .args(["-e", "normal", "--target", "all", "--prefix", "none"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    let mut crates: Vec<&str> = text.lines().filter_map(|l| l.split(' ').next()).collect();
    crates.sort_unstable();
    crates.dedup();
    assert!(crates.contains(&"farrier-service"), "{crates:?}");
    assert!(crates.len() <= 12, "{} crates: {crates:?}", crates.len());
}
