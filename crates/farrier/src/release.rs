use std::fs;
use std::path::{Path, PathBuf};

use farrier_update::contract::{self, Reject, Release, SUMS, SUMS_SIG, Sums};
use farrier_update::id::{Build, Channel, Os, Target};
use farrier_update::tool;

pub const USAGE: &str = "usage: farrier release verify <dir> --repo <owner>/<repo> --tag <tag> --sha <sha40> --bin <bin> --key <file>...";

struct Args {
    dir: PathBuf,
    repo: String,
    tag: String,
    sha: Build,
    bin: String,
    keys: Vec<String>,
}

fn parse(args: &[String]) -> Result<Args, String> {
    let (mut dir, mut repo, mut tag, mut sha, mut bin, mut keys) =
        (None, None, None, None, None, Vec::new());
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let slot = match arg.as_str() {
            "--repo" => &mut repo,
            "--tag" => &mut tag,
            "--sha" => &mut sha,
            "--bin" => &mut bin,
            "--key" => {
                let file = it.next().ok_or(USAGE)?;
                keys.push(fs::read_to_string(file).map_err(|e| format!("--key {file}: {e}"))?);
                continue;
            }
            _ if dir.is_none() && !arg.starts_with('-') => {
                dir = Some(PathBuf::from(arg));
                continue;
            }
            _ => return Err(USAGE.to_owned()),
        };
        *slot = Some(it.next().ok_or(USAGE)?.clone());
    }
    let (Some(dir), Some(repo), Some(tag), Some(sha), Some(bin)) = (dir, repo, tag, sha, bin)
    else {
        return Err(USAGE.to_owned());
    };
    let sha = Build::parse(&sha)
        .filter(|_| sha.len() == 40)
        .ok_or(format!("--sha {sha}: not 40 lowercase hex"))?;
    let repo_ok = repo
        .split_once('/')
        .is_some_and(|(o, r)| !o.is_empty() && !r.is_empty() && !r.contains('/'));
    if !repo_ok {
        return Err(format!("--repo {repo}: not <owner>/<repo>"));
    }
    if Channel::of_tag(&tag, &sha).is_none() {
        return Err(format!("--tag {tag}: not a release tag of {sha}"));
    }
    if keys.is_empty() {
        return Err(USAGE.to_owned());
    }
    Ok(Args {
        dir,
        repo,
        tag,
        sha,
        bin,
        keys,
    })
}

fn read(path: &Path) -> Result<Vec<u8>, Reject> {
    fs::read(path).map_err(|e| Reject::Download(format!("{}: {e}", path.display())))
}

fn sums(args: &Args) -> Result<(), Reject> {
    let text = read(&args.dir.join(SUMS))?;
    let sig = fs::read_to_string(args.dir.join(SUMS_SIG))
        .map_err(|e| Reject::Signature(format!("{SUMS_SIG}: {e}")))?;
    let keys: Vec<&str> = args.keys.iter().map(String::as_str).collect();
    let release = Release {
        repo: &args.repo,
        tag: &args.tag,
        sha: &args.sha,
    };
    contract::verify_sums(&text, &sig, &keys, &release)?;
    let text =
        String::from_utf8(text).map_err(|_| Reject::Checksum(format!("{SUMS} is not UTF-8")))?;
    let sums = Sums::parse(&text)?;
    for name in sums.names() {
        let path = args.dir.join(name);
        if !path.is_file() {
            return Err(Reject::NoAsset(format!(
                "{name} is in {SUMS} but not in {}",
                args.dir.display()
            )));
        }
        sums.check(name, &read(&path)?)?;
    }
    let mine: Vec<String> = Target::ALL.iter().map(|t| t.asset(&args.bin)).collect();
    if let Some(name) = mine
        .iter()
        .find(|name| args.dir.join(name).exists() && !sums.names().any(|n| n == *name))
    {
        return Err(Reject::Checksum(format!("{name} is not in {SUMS}")));
    }
    if !sums.names().any(|n| mine.iter().any(|m| m == n)) {
        return Err(Reject::NoAsset(format!(
            "{SUMS} lists no {}-<triple>",
            args.bin
        )));
    }
    Ok(())
}

fn staged(args: &Args, target: Target, exe: &Path) -> Result<(), Reject> {
    if target.os == Os::MacOs {
        tool::codesign(exe, &args.bin).map_err(Reject::CodeSign)?;
    }
    let out = tool::version(exe).map_err(Reject::Version)?;
    match contract::version(&out) {
        Some(build) if build.matches(&args.sha) => Ok(()),
        _ => Err(Reject::Version(format!(
            "{} --version names no build {}: '{}'",
            exe.display(),
            args.sha,
            out.trim()
        ))),
    }
}

/// `farrier release verify`: checks 2 to 6 of the release contract on a release folder.
pub fn verify(args: &[String]) -> Result<(), String> {
    let args = parse(args)?;
    let has_sums = args.dir.join(SUMS).exists();
    if has_sums {
        sums(&args).map_err(|r| r.to_string())?;
    }
    let exe = Target::host().map(|t| (t, args.dir.join(t.asset(&args.bin))));
    match exe {
        Some((target, exe)) if exe.is_file() => {
            staged(&args, target, &exe).map_err(|r| r.to_string())
        }
        _ if has_sums => Ok(()),
        _ => Err(Reject::NoAsset(format!(
            "{} has neither {SUMS} nor this host's {}",
            args.dir.display(),
            args.bin
        ))
        .to_string()),
    }
}
