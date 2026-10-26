//! Releases: which tag is a version and which is newest (under each program's own
//! rule), and the pin line in a settings file and its reset. (Moved from aso's and
//! spi's tests; the tag-rule test is new.)

mod common;

use air_minterface::release;
use air_minterface::version::{Tags, same_version};
use common::Prog;

fn strings(tags: &[&str]) -> Vec<String> {
    tags.iter().map(|s| s.to_string()).collect()
}

#[test]
fn release_tags_compare_as_numbers_and_ignore_what_is_not_a_release() {
    // Fails if 0.7.10 sorts below 0.7.9, or a release candidate or a word can be newest.
    let tags = strings(&["v0.9.0-rc1", "v0.7.10", "v0.7.9", "nightly", "0.8"]);
    assert_eq!(Tags::Strict.newest(&tags), Some("0.7.10".to_string()));
    assert!(Tags::Strict.is_newer("0.10.0", "0.9.0"));
    assert!(!Tags::Strict.is_newer("0.9.0", "0.9.0"));
    assert!(!Tags::Strict.is_newer("0.9.0-rc1", "0.8.0"));
    assert_eq!(Tags::Strict.newest(&strings(&["v1.0.0-rc2"])), None);
}

#[test]
fn a_version_may_have_two_numbers_only_where_the_program_says_so() {
    // Fails if the two rules are merged: a `0.8` tag would then win for the
    // strict programs (fsp-mint, the mints), or `29.1` would be ignored for the
    // loose one (bitcoind).
    let tags = strings(&["v0.7.10", "0.8", "29.1rc1"]);
    assert_eq!(Tags::Strict.newest(&tags), Some("0.7.10".to_string()));
    assert_eq!(Tags::Loose.newest(&tags), Some("0.8".to_string()));
    assert_eq!(Tags::Loose.numbers("29.1"), Some([29, 1, 0]));
    assert_eq!(Tags::Strict.numbers("29.1"), None);
    assert!(Tags::Loose.is_newer("29.2", "29.1.9"));
    assert!(!Tags::Loose.is_newer("29.1", "29.1.0"));
    assert_eq!(Tags::Loose.numbers("29"), None);
    assert_eq!(Tags::Loose.numbers("29.0rc1"), None);
}

#[test]
fn two_versions_are_the_same_when_their_numbers_are() {
    // Moved from spi's `a_bitcoind_version_may_have_two_or_three_numbers`.
    assert!(same_version("29.1", "29.1.0"));
    assert!(same_version("29.1.0", "29.1"));
    assert!(!same_version("29.1", "29.1.1"));
    assert!(!same_version("29.1", "30.1"));
    assert!(!same_version("29.x", "29.1"));
}

#[test]
fn a_pin_line_is_removed_from_its_own_section_only() {
    // Fails if the version line of another section goes, or a comment is lost.
    let text = "# top\n[air]\nversion = \"1.0.1\"\n[fsp]\n# pinned for the audit\nversion = \"0.7.0\"\nbin = \"/x\"\n";
    assert_eq!(
        release::without_pin_line(text, "fsp").unwrap(),
        "# top\n[air]\nversion = \"1.0.1\"\n[fsp]\n# pinned for the audit\nbin = \"/x\"\n"
    );
    assert_eq!(
        release::without_pin_line("[fsp]\nbin = \"/x\"\n", "fsp"),
        None
    );
}

#[test]
fn a_pin_reset_keeps_only_the_newest_backups_the_caller_asks_for() {
    // Moved from spi's `a_pin_reset_keeps_only_the_newest_five_backups`, with the
    // count a parameter. Fails if the count is hard-coded, or the new backup is
    // pruned, or the pin line is not the only change.
    let dir = common::scratch("baks");
    let config = dir.join("stub.toml");
    let original = "[alpha]\n# audit\nversion = \"0.6.0\"\nbin = \"/x\"\n";
    let at = |epoch: i64| dir.join(format!("stub.toml.{epoch}.bak"));
    for keep in [5, 2] {
        std::fs::write(&config, original).unwrap();
        (1..=6).for_each(|epoch| std::fs::write(at(epoch), "old").unwrap());
        let lines = release::reset_pins(&config, &[Prog::Alpha], 100, keep).unwrap();
        assert_eq!(
            lines,
            [format!(
                "reset: removed the `version` pin of alpha from {} (the old file is {})",
                config.display(),
                at(100).display()
            )]
        );
        assert_eq!(
            std::fs::read_to_string(&config).unwrap(),
            "[alpha]\n# audit\nbin = \"/x\"\n"
        );
        assert_eq!(std::fs::read_to_string(at(100)).unwrap(), original);
        let left: Vec<i64> = (1..=100).filter(|e| at(*e).exists()).collect();
        let expected: &[i64] = if keep == 5 {
            &[3, 4, 5, 6, 100]
        } else {
            &[6, 100]
        };
        assert_eq!(left, expected, "keep {keep}");
        (1..=100)
            .filter(|e| at(*e).exists())
            .for_each(|e| std::fs::remove_file(at(e)).unwrap());
    }
}

/// Five downloaded releases 0.1.0 to 0.5.0, with 0.5.0 installed and 0.4.0 the
/// one before it; `keep` is `release.keep`. Returns what `release::prune` removed.
fn pruned_with(name: &str, keep: usize) -> (Vec<String>, Vec<String>) {
    let mut op = common::stub(name, false);
    op.keep = keep;
    let root = op.dir.join("releases/alpha");
    for v in ["0.1.0", "0.2.0", "0.3.0", "0.4.0", "0.5.0"] {
        std::fs::create_dir_all(root.join(v)).unwrap();
    }
    // a folder that is not a version is left alone
    std::fs::create_dir_all(root.join("scratch")).unwrap();
    let gone = release::prune(&op, Prog::Alpha, &["0.5.0", "0.4.0"]).unwrap();
    let mut left: Vec<String> = std::fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .collect();
    left.sort();
    (gone, left)
}

#[test]
fn release_prune_removes_the_oldest_first_and_keep_0_removes_nothing() {
    // Fails if keep 0 deletes anything (it must keep every release), if the
    // newest are removed first (the order of `gone` is the removal order, oldest
    // first), or if the installed release and the one before it can go.
    let (gone, left) = pruned_with("keep-3", 3);
    assert_eq!(gone, ["0.1.0", "0.2.0"]);
    assert_eq!(left, ["0.3.0", "0.4.0", "0.5.0", "scratch"]);
    let (gone, left) = pruned_with("keep-0", 0);
    assert!(gone.is_empty());
    assert_eq!(
        left,
        ["0.1.0", "0.2.0", "0.3.0", "0.4.0", "0.5.0", "scratch"]
    );
    // keep 1 would keep only 0.5.0; 0.4.0 is protected as the one before it
    let (gone, left) = pruned_with("keep-1", 1);
    assert_eq!(gone, ["0.1.0", "0.2.0", "0.3.0"]);
    assert_eq!(left, ["0.4.0", "0.5.0", "scratch"]);
}

#[test]
fn backup_pruning_with_keep_0_leaves_every_backup() {
    // Fails if keep 0 is read as "keep none" and removes the backups of a
    // settings file. 7 backups, epochs 1 to 7: keep 3 leaves 5, 6 and 7.
    let dir = common::scratch("baks-keep-0");
    let config = dir.join("stub.toml");
    std::fs::write(&config, "x").unwrap();
    for epoch in 1..=7 {
        std::fs::write(dir.join(format!("stub.toml.{epoch}.bak")), "old").unwrap();
    }
    let count = || {
        std::fs::read_dir(&dir)
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".bak")
            })
            .count()
    };
    air_minterface::files::prune_backups(&config, "stub.toml", 0);
    assert_eq!(count(), 7);
    air_minterface::files::prune_backups(&config, "stub.toml", 3);
    assert_eq!(count(), 3);
    assert!(dir.join("stub.toml.7.bak").exists() && !dir.join("stub.toml.4.bak").exists());
}
