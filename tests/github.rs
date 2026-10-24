//! The real release source with `curl` and `tar` standing between it and
//! GitHub: the archive for the machine, a checksum that fails, and a release with
//! no archive for the machine. (Moved from spi's tests; the web is served from
//! memory.)

mod common;

use air_interface::github::Github;
use common::{Web, scratch, sha, stub, tarball, web};

const LINUX: &str = "alpha-0.6.0-linux-x86_64.tar.gz";
const MAC: &str = "alpha-0.6.0-macos-aarch64.tar.gz";
const API: &str = "https://releases.example";

/// A web with alpha 0.6.0 (an archive for Linux on x86_64 and one for macOS on
/// aarch64: the same bytes, the requests say which was asked for) and a release
/// candidate. With `lie` the Linux archive's checksum is wrong.
fn releases(lie: bool) -> Web {
    let archive = tarball("alpha", "alpha 0.6.0\n");
    let linux = if lie { "0".repeat(64) } else { sha(&archive) };
    let sums = format!("{linux}  {LINUX}\n{}  {MAC}\n", sha(&archive));
    let asset =
        |n: &str| serde_json::json!({"name": n, "browser_download_url": format!("{API}/dl/{n}")});
    let list = serde_json::json!([
        {"tag_name": "v0.7.0-rc1", "prerelease": true},
        {"tag_name": "v0.6.0", "assets": [asset(LINUX), asset(MAC), asset("SHA256SUMS")]},
    ]);
    web(vec![
        (
            format!("{API}/repos/dav-anderson/alpha/releases?per_page=100"),
            list.to_string().into_bytes(),
        ),
        (format!("{API}/dl/SHA256SUMS"), sums.into_bytes()),
        (format!("{API}/dl/{LINUX}"), archive.clone()),
        (format!("{API}/dl/{MAC}"), archive),
    ])
}

fn asked(web: &Web, name: &str) -> bool {
    web.asked.lock().unwrap().iter().any(|u| u.ends_with(name))
}

#[test]
fn the_archive_for_this_machine_is_the_one_downloaded_and_unpacked() {
    let web = releases(false);
    let op = stub("mac", true);
    let dir = scratch("mac-dl").join("0.6.0");
    let github = Github::for_machine(&op, &web, "macos", "aarch64");
    let got = github.fetch_release("alpha", "0.6.0", &dir).unwrap();
    assert_eq!(std::fs::read_to_string(&got.file).unwrap(), "alpha 0.6.0\n");
    assert!(asked(&web, MAC) && !asked(&web, LINUX));
    // Only the program is left: not the archive, not the unpacked folder.
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&got.file).unwrap().permissions().mode() & 0o111,
            0o111
        );
    }
}

#[test]
fn a_download_that_fails_its_checksum_is_deleted_and_nothing_is_left() {
    let web = releases(true);
    let op = stub("bad-sum", false);
    let dir = scratch("bad").join("0.6.0");
    let github = Github::for_machine(&op, &web, "linux", "x86_64");
    let err = github.fetch_release("alpha", "0.6.0", &dir).unwrap_err();
    assert!(
        err.contains("failed its checksum") && err.contains(&"0".repeat(64)),
        "{err}"
    );
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
}

#[test]
fn a_release_with_no_archive_for_this_machine_is_refused_and_says_so() {
    let web = releases(false);
    let op = stub("no-archive", false);
    let dir = scratch("none").join("0.6.0");
    let github = Github::for_machine(&op, &web, "linux", "riscv64");
    let err = github.fetch_release("alpha", "0.6.0", &dir).unwrap_err();
    assert!(
        err.contains("has no archive for this machine (linux on riscv64)") && err.contains(LINUX),
        "{err}"
    );
    assert!(!dir.exists());
}
