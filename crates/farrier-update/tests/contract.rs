use farrier_update::contract::{Reject, Sums, version};
use farrier_update::id::{Build, Channel, Target};

const HEX: &str = "f3a7734493d72de42b0a4ee9a155d306085cb81f904708ea1c11e68c4e4751c5";

#[test]
fn sums_take_only_the_strict_format() {
    let good = format!("{HEX}  fx-aarch64-apple-darwin\n{HEX}  fx-x86_64-unknown-linux-gnu\n");
    let sums = Sums::parse(&good).unwrap();
    assert_eq!(
        sums.names().collect::<Vec<_>>(),
        ["fx-aarch64-apple-darwin", "fx-x86_64-unknown-linux-gnu"]
    );
    for bad in [
        String::new(),
        format!("{HEX}  fx"),
        format!("{HEX}  fx\n\n"),
        format!("{HEX} fx\n"),
        format!("{HEX}  *fx\n"),
        format!("{HEX}  a/fx\n"),
        format!("{HEX}  fx \n"),
        format!("{}  fx\n", HEX.to_uppercase()),
        format!("{}  fx\n", &HEX[1..]),
        format!("{HEX}  fx\n{HEX}  fx\n"),
        format!("# sums\n{HEX}  fx\n"),
    ] {
        assert!(
            matches!(Sums::parse(&bad), Err(Reject::Checksum(_))),
            "{bad:?}"
        );
    }
}

#[test]
fn sums_check_each_asset_against_its_line() {
    let sums = Sums::parse(&format!("{HEX}  fx\n")).unwrap();
    let bytes = b"#!/bin/sh\necho \"fx 0.1.0 (0123456)\"\n";
    assert_eq!(sums.check("fx", bytes), Ok(()));
    assert!(matches!(
        sums.check("fx", b"other"),
        Err(Reject::Checksum(_))
    ));
    assert!(matches!(sums.check("fy", bytes), Err(Reject::NoAsset(_))));
}

#[test]
fn version_reads_the_build_off_the_last_line() {
    let build = Build::parse("0123456789").unwrap();
    assert_eq!(version("fx 0.1.0\nfx 0.1.0 (0123456789)\n"), Some(build));
    assert_eq!(version("fx 0.1.0 (0123456789) extra"), None);
    assert_eq!(version("fx 0.1.0 (012345)"), None);
    assert_eq!(version("fx 0.1.0"), None);
}

#[test]
fn tags_and_assets_follow_the_contract() {
    let build = Build::parse("0123456789abcdef0123456789abcdef01234567").unwrap();
    assert_eq!(Channel::Main.tag(&build), "build-0123456");
    let branch = Channel::Branch("next".to_owned());
    assert_eq!(branch.tag(&build), "build-next-0123456");
    assert_eq!(
        Channel::of_tag("build-0123456", &build),
        Some(Channel::Main)
    );
    assert_eq!(Channel::of_tag("build-next-0123456", &build), Some(branch));
    assert_eq!(Channel::of_tag("build--0123456", &build), None);
    assert_eq!(Channel::of_tag("build-89abcde", &build), None);
    let names: Vec<String> = Target::ALL.iter().map(|t| t.asset("fx")).collect();
    assert_eq!(
        names,
        [
            "fx-aarch64-apple-darwin",
            "fx-x86_64-apple-darwin",
            "fx-x86_64-unknown-linux-gnu",
            "fx-aarch64-unknown-linux-gnu"
        ]
    );
    assert!(build.matches(&Build::parse("0123456").unwrap()));
    assert!(!build.matches(&Build::parse("89abcde").unwrap()));
}
