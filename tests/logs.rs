//! The caps on logs: moving a log over the cap aside, keeping only the newest
//! backups, and a log an operator opens itself. (Moved from aso's tests; spi's
//! and aso's own tests check that their command lines and units call this.)

mod common;

use std::fs;
use std::io::Write;

use air_minterface::logs::{self, Log};
use common::scratch;

fn big(path: &std::path::Path, bytes: usize) {
    fs::write(path, vec![b'x'; bytes]).unwrap();
}

#[test]
fn a_log_over_the_cap_is_moved_aside_and_one_under_is_left() {
    // Fails if the comparison is off by one, or a log under the cap is moved.
    let dir = scratch("rotate");
    let log = dir.join("fsp-mint.log");
    big(&log, 1024 * 1024 + 1);
    let moved = logs::rotate(&log, 1, 4, 1_000).unwrap();
    assert_eq!(moved, Some(dir.join("fsp-mint.log.1000.bak")));
    assert!(!log.exists());
    big(&log, 1024 * 1024);
    assert_eq!(logs::rotate(&log, 1, 4, 2_000).unwrap(), None);
    assert!(log.exists());
}

#[test]
fn only_the_newest_log_keep_backups_stay() {
    // Fails if the oldest backups are kept, or log_keep is ignored.
    let dir = scratch("keep");
    let log = dir.join("air_server.log");
    for epoch in [100, 200, 300, 400, 500] {
        fs::write(dir.join(format!("air_server.log.{epoch}.bak")), "old").unwrap();
    }
    big(&log, 1024 * 1024 + 1);
    logs::rotate(&log, 1, 3, 600).unwrap();
    let mut left: Vec<String> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .collect();
    left.sort();
    assert_eq!(
        left,
        vec![
            "air_server.log.400.bak",
            "air_server.log.500.bak",
            "air_server.log.600.bak"
        ]
    );
}

#[test]
fn two_rotations_in_one_second_do_not_overwrite_each_other() {
    // Fails if the second backup replaces the first.
    let dir = scratch("same-second");
    let log = dir.join("a.log");
    big(&log, 1024 * 1024 + 1);
    logs::rotate(&log, 1, 4, 50).unwrap();
    big(&log, 1024 * 1024 + 1);
    let second = logs::rotate(&log, 1, 4, 50).unwrap();
    assert_eq!(second, Some(dir.join("a.log.51.bak")));
    assert!(dir.join("a.log.50.bak").exists());
}

#[test]
fn a_log_the_operator_opens_is_rotated_first_and_private() {
    // Fails if Log::open appends to an over-size file, or makes it readable by others.
    let dir = scratch("open");
    let path = dir.join("aso-redeem.log");
    big(&path, 1024 * 1024 + 1);
    let mut log = Log::open("stub", &path, 1, 4, 9_000, false).unwrap();
    log.line(9_001, "hello");
    drop(log);
    assert_eq!(fs::read_to_string(&path).unwrap(), "9001 hello\n");
    assert!(dir.join("aso-redeem.log.9000.bak").exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[test]
fn a_running_log_over_the_cap_is_copied_aside_and_emptied_where_the_program_writes() {
    // Fails if the rotation of a running program's log renames the file (the
    // program would go on writing to the .bak, past any cap), leaves the old text
    // in the log, or makes the copy readable by others.
    let dir = scratch("running");
    let log = dir.join("air_server.log");
    let mut program = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
        .unwrap();
    program.write_all(&vec![b'x'; 1024 * 1024 + 1]).unwrap();
    let bak = dir.join("air_server.log.3000.bak");
    assert_eq!(
        logs::rotate_running(&log, 1, 4, 3_000).unwrap(),
        Some(bak.clone())
    );
    assert_eq!(fs::metadata(&bak).unwrap().len(), 1024 * 1024 + 1);
    program.write_all(b"next line\n").unwrap();
    assert_eq!(fs::read_to_string(&log).unwrap(), "next line\n");
    assert_eq!(logs::rotate_running(&log, 1, 4, 3_100).unwrap(), None);
    assert_eq!(fs::read_to_string(&log).unwrap(), "next line\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&bak).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[test]
fn a_max_of_0_never_moves_a_log_and_a_keep_of_0_keeps_every_backup() {
    // Fails if 0 is read as a cap of 0 MB (every log would be moved aside) or as
    // "keep none" (every backup would be removed). A 60 MB log with max_mb 0 stays
    // where it is for both ways of rotating; 10 backups survive keep 0 and the
    // same call with keep 4 leaves the newest 4.
    let dir = scratch("zero");
    let log = dir.join("air_server.log");
    big(&log, 60 * 1024 * 1024);
    for epoch in 1..=10 {
        fs::write(dir.join(format!("air_server.log.{epoch}.bak")), "old").unwrap();
    }
    let count = |dir: &std::path::Path| {
        fs::read_dir(dir)
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
    assert_eq!(logs::rotate(&log, 0, 0, 700).unwrap(), None);
    assert_eq!(logs::rotate_running(&log, 0, 0, 800).unwrap(), None);
    assert_eq!(fs::metadata(&log).unwrap().len(), 60 * 1024 * 1024);
    assert_eq!(count(&dir), 10);
    assert_eq!(logs::prune(&log, 0).unwrap(), 0);
    assert_eq!(count(&dir), 10);
    assert_eq!(logs::prune(&log, 4).unwrap(), 6);
    assert_eq!(count(&dir), 4);
}
