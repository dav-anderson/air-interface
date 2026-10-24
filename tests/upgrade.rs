//! The upgrade engine against a stub operator: what is downloaded, checked and
//! put in place, and what is left alone when something is wrong. (Moved from
//! spi's `versions_upgrade`; spi's and aso's tests check their own wording,
//! restart commands, rollbacks and prompts through the real programs.)
//!
//! Nothing is slept for: a health check that answers at once is waited for zero
//! seconds.

mod common;

use air_minterface::release::{self, Source};
use air_minterface::upgrade::{self, Options, Upgraded};
use air_minterface::{Managed, Probe};
use common::{FakeSource, Prog, Stub, accepting, install, record_path, serving, stub, text};

/// Upgrade `programs` with `source`, the health check answering `probe`.
fn run(
    op: &Stub,
    programs: &[Prog],
    dry_run: bool,
    source: &dyn Source<Prog>,
    probe: &dyn Fn(Prog) -> Probe,
) -> Upgraded<Prog> {
    let opts = Options {
        dry_run,
        wait_secs: 0,
        source,
    };
    upgrade::upgrade(op, programs, &opts, &accepting(), probe)
}

fn alpha_at_0_5_0(name: &str) -> Stub {
    let op = stub(name, false);
    install(&op, Prog::Alpha, "0.5.0");
    op
}

#[test]
fn with_no_pin_the_newest_release_is_downloaded_verified_and_installed() {
    let op = alpha_at_0_5_0("newest");
    let source = FakeSource::new()
        .tags(&["v0.6.0", "v0.7.0"])
        .build("0.7.0", "alpha 0.7.0\n");
    let done = run(&op, &[Prog::Alpha], false, &source, &serving("0.7.0"));
    assert!(done.ok, "{:?}", done.lines);
    assert_eq!(text(&Prog::Alpha.installed_path(&op)), "alpha 0.7.0\n");
    assert_eq!(
        text(&Prog::Alpha.installed_path(&op).with_extension("previous")),
        "alpha 0.5.0\n"
    );
    assert_eq!(*source.fetched.lock().unwrap(), ["alpha 0.7.0"]);
    assert_eq!(done.lines[0], "alpha: 0.5.0 -> 0.7.0");
    assert_eq!(done.lines.last().unwrap(), "alpha: now 0.7.0, healthy");
    assert_eq!(*op.restarts.lock().unwrap(), ["alpha"]);
    // The downloaded file is executable, and the operator recorded what it installed.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(Prog::Alpha.installed_path(&op))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o111, 0o111);
    }
    let record: serde_json::Value =
        serde_json::from_slice(&std::fs::read(record_path(&op)).unwrap()).unwrap();
    assert_eq!(
        record,
        serde_json::json!({
            "version": "0.7.0",
            "sha256": "cd74fe408374b6dd7a6b17cf3e5a4d20967d86ab3524c640a1945c0ea6a49b29"
        })
    );
}

#[test]
fn a_pin_older_than_the_newest_installs_the_pin_and_says_the_newest() {
    let mut op = alpha_at_0_5_0("pinned-older");
    op.pin = Some("0.6.0".into());
    let source = FakeSource::new()
        .tags(&["v0.6.0", "v0.7.0"])
        .build("0.6.0", "alpha 0.6.0\n")
        .build("0.7.0", "alpha 0.7.0\n");
    let done = run(&op, &[Prog::Alpha], false, &source, &serving("0.6.0"));
    assert!(done.ok, "{:?}", done.lines);
    assert_eq!(done.lines[0], "alpha: pinned at 0.6.0 (newest 0.7.0)");
    assert_eq!(text(&Prog::Alpha.installed_path(&op)), "alpha 0.6.0\n");
    // The newer release is reported, never installed.
    assert_eq!(*source.fetched.lock().unwrap(), ["alpha 0.6.0"]);
}

#[test]
fn a_download_that_fails_its_checksum_changes_nothing() {
    let op = alpha_at_0_5_0("bad-sum");
    let wrong = "0".repeat(64);
    let source =
        FakeSource::new()
            .tags(&["v0.7.0"])
            .corrupt_build("0.7.0", "alpha 0.7.0\n", Some(&wrong));
    let done = run(&op, &[Prog::Alpha], false, &source, &serving("0.7.0"));
    assert!(!done.ok);
    assert!(
        done.lines.iter().any(|l| l.starts_with(
            "FAIL: alpha: the download of alpha 0.7.0 failed its checksum (the release says 0000"
        ) && l.ends_with("it was deleted and nothing was changed")),
        "{:?}",
        done.lines
    );
    assert_eq!(text(&Prog::Alpha.installed_path(&op)), "alpha 0.5.0\n");
    assert!(
        !Prog::Alpha
            .installed_path(&op)
            .with_extension("previous")
            .exists()
    );
    assert!(
        !Prog::Alpha
            .installed_path(&op)
            .with_extension("new")
            .exists()
    );
    assert!(
        !release::releases_dir(&op, Prog::Alpha, "0.7.0")
            .join("alpha")
            .exists()
    );
    assert!(op.restarts.lock().unwrap().is_empty());
    assert!(!record_path(&op).exists());
}

#[test]
fn a_download_that_is_not_the_version_the_release_names_changes_nothing() {
    let op = alpha_at_0_5_0("wrong-build");
    let source = FakeSource::new()
        .tags(&["v0.7.0"])
        .build("0.7.0", "alpha 0.6.1\n");
    let done = run(&op, &[Prog::Alpha], false, &source, &serving("0.7.0"));
    assert!(!done.ok);
    assert!(
        done.lines[0].contains("is 0.6.1, but the release names 0.7.0: nothing was changed"),
        "{:?}",
        done.lines
    );
    assert_eq!(text(&Prog::Alpha.installed_path(&op)), "alpha 0.5.0\n");
    assert!(op.restarts.lock().unwrap().is_empty());
}

#[test]
fn a_program_already_at_the_newest_release_is_left_alone_and_nothing_is_stopped() {
    let op = stub("already-newest", false);
    install(&op, Prog::Alpha, "0.7.0");
    let source = FakeSource::new()
        .tags(&["v0.7.0"])
        .build("0.7.0", "alpha 0.7.0\n");
    let done = run(&op, &[Prog::Alpha], false, &source, &serving("0.7.0"));
    assert!(done.ok, "{:?}", done.lines);
    assert!(done.changes.is_empty());
    assert_eq!(
        done.lines,
        [
            "alpha: already 0.7.0: nothing to do",
            "everything up to date: nothing was stopped"
        ]
    );
    assert!(source.fetched.lock().unwrap().is_empty());
    assert!(op.restarts.lock().unwrap().is_empty());
    assert_eq!(text(&Prog::Alpha.installed_path(&op)), "alpha 0.7.0\n");
}
