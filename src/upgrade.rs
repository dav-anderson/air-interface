//! The upgrade engine: bring programs to their target version, one at a time,
//! with a rollback. Both operators' `upgrade` commands call [`upgrade`]; what
//! they print around it, and what they ask first, is theirs.
//!
//! **The target.** A program's `version` in the settings file is an optional
//! pin. With no pin the target is the newest tagged release the release source
//! knows; with a pin it is the pin, which may be older than the newest, and a
//! newer release is reported ("pinned at x (newest y)"), never installed. A
//! program that publishes no releases has its `bin` as the operator's own file
//! and its `version` as the operator's word for it ([`decide`]).
//!
//! **The build.** When the release source has the target, the engine downloads
//! the release binary, checks its SHA-256 against the one the release publishes
//! and makes it executable; a download that fails the check is deleted and
//! nothing is changed. Otherwise the build is the file `bin` names. Either way
//! the build must be what is wanted before anything is changed: a program that
//! prints its version must print the target, and one that does not may not be
//! the very file that is installed ([`obtain`]).
//!
//! **Putting it in place** is two renames in the install folder: the new file
//! is copied beside the installed one as `<name>.new`, the installed one becomes
//! `<name>.previous`, and `<name>.new` becomes the installed one. Both steps are
//! atomic on one filesystem, and everything that can fail (the download, the
//! checks, the copy) happens before the first rename and before any program is
//! stopped ([`swap_in`]).
//!
//! **Keeping the downtime short.** The stop and the start are one service-manager
//! call, with the two renames just before it; the engine then polls the program
//! every half second until it is serving ([`Managed::serving`]). When the new
//! build does not come up, the old binary is put back and started
//! ([`put_back`]), and the run stops there. The next program is not touched
//! until the one before is serving, and a program already at its target is left
//! alone and nothing is stopped.
//!
//! **Wording.** The sentences whose wording differs between the operators are not
//! here; the program says them ([`Managed::say`], [`Say`]), so each operator's
//! output is what it always was.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::exec::Exec;
use crate::files::{find_binary, make_executable, sha256_file};
use crate::release::{self, Source};
use crate::version::same_version;
use crate::{Managed, Operator};

/// What a program's health check answered: whether it is up, and, for a program
/// that reports one, the `version` it is running.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Probe {
    pub up: bool,
    pub version: Option<String>,
}

impl Probe {
    /// Up, with no version.
    pub fn up() -> Probe {
        Probe {
            up: true,
            version: None,
        }
    }

    /// Up, and reporting `version`.
    pub fn reporting(version: &str) -> Probe {
        Probe {
            up: true,
            version: Some(version.to_string()),
        }
    }
}

/// The sentences the engine needs that each operator words its own way. The
/// program's [`Managed::say`] answers each. Unless the variant says otherwise,
/// the answer is a whole line, name and all.
#[derive(Clone, Copy, Debug)]
pub enum Say<'a> {
    /// `check`: the detail of a program upstream publishes no releases for.
    NoReleases,
    /// `check`: the detail of a program the operator does not manage.
    UnmanagedStatus,
    /// `upgrade`: why a program the operator does not manage is skipped (after
    /// `name: skipped: `).
    Unmanaged,
    /// `upgrade`: why there is no target, for a program with nothing to
    /// download (after `name: skipped: `).
    NoVersionUnpublished,
    /// The same, when the release source lists no release.
    NoVersionNoRelease,
    /// The same, when the newest release could not be found; the reason.
    NoVersionNotFound(&'a str),
    /// Why a program cannot be upgraded: its install is not there (after
    /// `FAIL: name: `). The path is the install.
    NotInstalled(&'a Path),
    /// Why a build cannot be obtained: no file for `bin`; the setting's text
    /// (after `FAIL: name: `).
    BinMissing(&'a str),
    /// Why: `bin` names the installed file itself (after `FAIL: name: `).
    BinIsInstalled(&'a Path),
    /// The line that says what is about to happen: `name: from -> target ...`.
    Plan { from: &'a str, target: &'a str },
    /// The line a dry run says when the build would be downloaded.
    DryRunDownload,
    /// The last line of a program that is upgraded: `name: now target ...`.
    Done { target: &'a str },
    /// What to run as root, in "run ... as root" (`spi upgrade`, `aso`).
    RunAs,
}

/// How an upgrade is run.
pub struct Options<'a, P: Managed> {
    pub dry_run: bool,
    /// How long each program gets to be serving after its restart.
    pub wait_secs: u64,
    /// Where releases come from.
    pub source: &'a dyn Source<P>,
}

/// What `upgrade` did.
#[derive(Debug)]
pub struct Upgraded<P> {
    pub lines: Vec<String>,
    pub ok: bool,
    /// The programs that were changed, or, in a dry run, would be.
    pub changes: Vec<P>,
}

/// Poll `probe` until `done` accepts what it answers, or `wait_secs` run out.
/// Returns the last answer and whether it was accepted.
pub fn wait_for<P: Managed>(
    probe: &dyn Fn(P) -> Probe,
    program: P,
    wait_secs: u64,
    done: &dyn Fn(&Probe) -> bool,
) -> (Probe, bool) {
    let end = Instant::now() + Duration::from_secs(wait_secs);
    loop {
        let answer = probe(program);
        if done(&answer) {
            return (answer, true);
        }
        if Instant::now() >= end {
            return (answer, false);
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// What the operator will install for a program, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decision {
    pub target: String,
    /// "pinned at x (newest y)", when a newer release exists than the pin.
    pub pin_note: Option<String>,
    /// Whether the build is downloaded from the release source (else `bin`).
    pub from_release: bool,
}

/// The target of one program, or why there is none.
pub fn decide<P: Managed>(
    op: &P::Op,
    program: P,
    source: &dyn Source<P>,
) -> Result<Decision, String> {
    let name = program.name();
    let pin = program.pin(op);
    // A build the operator supplied (`bin`) is an override: no download.
    let own = program.own_build(op);
    let newest = if program.publishes() && !own {
        release::newest_of(source, program)
    } else {
        Ok(None)
    };
    match (pin, newest) {
        (Some(pin), Ok(Some(newest))) => Ok(Decision {
            pin_note: program
                .tags()
                .is_newer(&newest, &pin)
                .then(|| format!("{name}: pinned at {pin} (newest {newest})")),
            target: pin,
            from_release: true,
        }),
        (Some(pin), _) => Ok(Decision {
            target: pin,
            pin_note: None,
            from_release: false,
        }),
        (None, Ok(Some(newest))) => Ok(Decision {
            target: newest,
            pin_note: None,
            from_release: true,
        }),
        (None, Ok(None)) if !program.publishes() || own => {
            Err(program.say(op, Say::NoVersionUnpublished))
        }
        (None, Ok(None)) => Err(program.say(op, Say::NoVersionNoRelease)),
        (None, Err(e)) => Err(program.say(op, Say::NoVersionNotFound(&e))),
    }
}

/// The build to install: downloaded and verified, or the one `bin` names. The
/// result is a file that is not the installed one.
pub fn obtain<P: Managed>(
    op: &P::Op,
    exec: &dyn Exec,
    program: P,
    d: &Decision,
    source: &dyn Source<P>,
) -> Result<PathBuf, String> {
    let name = program.name();
    let target = &d.target;
    let installed = program.installed_path(op);
    let new = if d.from_release {
        let dir = release::releases_dir(op, program, target);
        let got = source
            .fetch(program, target, &dir)
            .map_err(|e| format!("cannot get {name} {target} from the release source: {e}"))?;
        let have = sha256_file(&got.file)?;
        if !have.eq_ignore_ascii_case(&got.sha256) {
            let _ = fs::remove_file(&got.file);
            return Err(format!(
                "the download of {name} {target} failed its checksum (the release says {}, the file is {have}): it was deleted and nothing was changed",
                got.sha256
            ));
        }
        make_executable(&got.file)?;
        got.file
    } else {
        let bin = program.bin(op);
        let Some(new) = find_binary(&bin) else {
            return Err(program.say(op, Say::BinMissing(&bin)));
        };
        if fs::canonicalize(&installed).ok().as_deref() == Some(new.as_path()) {
            return Err(program.say(op, Say::BinIsInstalled(&installed)));
        }
        new
    };
    // The new build must be what is wanted. A program that prints its version
    // says what it is. One that cannot is taken as named (a release's checksum
    // was checked above), unless it is the very file that is installed.
    if program.prints_version() {
        match program.reported(op, exec, &new) {
            Ok(found) if same_version(&found, target) => {}
            Ok(found) => {
                return Err(format!(
                    "{} is {found}, but {} names {target}: nothing was changed",
                    new.display(),
                    if d.from_release {
                        "the release"
                    } else {
                        <P::Op as Operator>::CONFIG_NAME
                    }
                ));
            }
            Err(e) => return Err(format!("nothing was changed: {e}")),
        }
    } else if !d.from_release
        && installed.is_file()
        && sha256_file(&new).ok() == sha256_file(&installed).ok()
    {
        return Err(format!(
            "the new build {} is the same file as the installed one, not {target}",
            new.display()
        ));
    }
    Ok(new)
}

fn staged(installed: &Path) -> PathBuf {
    sibling(installed, "new")
}

/// Where the installed file is kept while its replacement is tried.
pub fn previous(installed: &Path) -> PathBuf {
    sibling(installed, "previous")
}

fn sibling(installed: &Path, suffix: &str) -> PathBuf {
    let name = installed
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    installed.with_file_name(format!("{name}.{suffix}"))
}

/// Copy `new` beside `installed`, keep the installed one as `.previous`, and put
/// the copy in its place. Returns the `.previous` file when there was an
/// installed one to keep. `run_as` is what to run as root, for the message when
/// the copy cannot be written.
pub fn swap_in(new: &Path, installed: &Path, run_as: &str) -> Result<Option<PathBuf>, String> {
    let staged = staged(installed);
    let previous = previous(installed);
    if let Some(dir) = installed.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    fs::copy(new, &staged).map_err(|e| {
        format!(
            "cannot write {}: {e} (run {run_as} as root)",
            staged.display()
        )
    })?;
    make_executable(&staged).inspect_err(|_| {
        let _ = fs::remove_file(&staged);
    })?;
    let had = installed.is_file();
    if had && let Err(e) = fs::rename(installed, &previous) {
        let _ = fs::remove_file(&staged);
        return Err(format!(
            "cannot keep {} as {}: {e}",
            installed.display(),
            previous.display()
        ));
    }
    if let Err(e) = fs::rename(&staged, installed) {
        if had {
            let _ = fs::rename(&previous, installed);
        }
        let _ = fs::remove_file(&staged);
        return Err(format!("cannot put {} in place: {e}", installed.display()));
    }
    Ok(had.then_some(previous))
}

/// Put the `.previous` file back as the installed one.
pub fn put_back(installed: &Path) -> Result<(), String> {
    let previous = previous(installed);
    fs::rename(&previous, installed).map_err(|e| {
        format!(
            "cannot put {} back as {}: {e}",
            previous.display(),
            installed.display()
        )
    })
}

/// Upgrade `programs`, in the order given, one at a time. With `dry_run`
/// nothing is changed and each program's plan is said. The first failure stops
/// the run. `probe` asks a program's health check what it answers.
pub fn upgrade<P: Managed>(
    op: &P::Op,
    programs: &[P],
    opts: &Options<P>,
    exec: &dyn Exec,
    probe: &dyn Fn(P) -> Probe,
) -> Upgraded<P> {
    let mut out = Upgraded {
        lines: Vec::new(),
        ok: true,
        changes: Vec::new(),
    };
    let mut already = 0usize;
    for &program in programs {
        let name = program.name();
        if !program.managed(op) {
            out.lines.push(format!(
                "{name}: skipped: {}",
                program.say(op, Say::Unmanaged)
            ));
            continue;
        }
        let decision = match decide(op, program, opts.source) {
            Ok(d) => d,
            Err(why) => {
                out.lines.push(format!("{name}: skipped: {why}"));
                continue;
            }
        };
        match upgrade_one(op, program, &decision, opts, exec, probe) {
            Ok(done) => {
                out.lines.extend(done.lines);
                if done.changed {
                    out.changes.push(program);
                } else {
                    already += 1;
                }
            }
            Err(lines) => {
                out.lines.extend(lines);
                out.ok = false;
                out.lines
                    .push("stopped: the programs after this one were not touched".into());
                return out;
            }
        }
    }
    if out.ok && out.changes.is_empty() && already == programs.len() {
        out.lines
            .push("everything up to date: nothing was stopped".into());
    }
    out
}

struct One {
    lines: Vec<String>,
    /// False when the program was already at its target.
    changed: bool,
}

fn upgrade_one<P: Managed>(
    op: &P::Op,
    program: P,
    d: &Decision,
    opts: &Options<P>,
    exec: &dyn Exec,
    probe: &dyn Fn(P) -> Probe,
) -> Result<One, Vec<String>> {
    let name = program.name();
    let target = d.target.as_str();
    let wait_secs = opts.wait_secs;
    let fail = |why: String| vec![format!("FAIL: {name}: {why}")];
    let mut lines = Vec::new();
    if let Some(note) = &d.pin_note {
        lines.push(note.clone());
    }
    let installed = program.installed_path(op);
    if !installed.is_file() {
        return Err(fail(program.say(op, Say::NotInstalled(&installed))));
    }
    let current = program.reported(op, exec, &installed).ok();
    if current.as_deref().is_some_and(|c| same_version(c, target)) {
        lines.push(format!("{name}: already {target}: nothing to do"));
        return Ok(One {
            lines,
            changed: false,
        });
    }
    let from = current.unwrap_or_else(|| "an unknown version".into());
    // A dry run downloads nothing: it says where the build would come from.
    let new = if opts.dry_run && d.from_release {
        None
    } else {
        match obtain(op, exec, program, d, opts.source) {
            Ok(n) => Some(n),
            Err(why) => {
                lines.push(format!("FAIL: {name}: {why}"));
                return Err(lines);
            }
        }
    };
    lines.push(program.say(
        op,
        Say::Plan {
            from: &from,
            target,
        },
    ));
    if opts.dry_run {
        if new.is_none() {
            lines.push(program.say(op, Say::DryRunDownload));
        }
        lines.push(format!("{name}: dry run: nothing was changed"));
        return Ok(One {
            lines,
            changed: true,
        });
    }
    let Some(new) = new else {
        return Err(fail("no build to install".into()));
    };

    let run_as = program.say(op, Say::RunAs);
    let kept = match swap_in(&new, &installed, &run_as) {
        Ok(p) => p,
        Err(e) => {
            lines.push(format!("FAIL: {name}: {e}"));
            return Err(lines);
        }
    };
    lines.push(format!(
        "{name}: installed {}{}",
        installed.display(),
        kept.as_ref()
            .map(|p| format!(", the old one is {}", p.display()))
            .unwrap_or_default()
    ));

    if program.is_service() {
        let began = Instant::now();
        let restarted = program.restart(op, exec);
        let (last, went) = if restarted.is_ok() {
            wait_for(probe, program, wait_secs, &|p| program.serving(p, target))
        } else {
            (Probe::default(), false)
        };
        if !went {
            // Put the old binary back and start it.
            let why = match &restarted {
                Err(e) => e.clone(),
                Ok(()) => program.not_serving(&last, target, wait_secs),
            };
            let back = put_back(&installed);
            let again = back.is_ok() && program.restart(op, exec).is_ok();
            lines.push(match (back.is_ok(), again) {
                (true, true) => format!("FAIL: {name}: {why}; the old one is back and restarted"),
                (true, false) => format!(
                    "FAIL: {name}: {why}, and the old one is back in place but could not be restarted: restart {} yourself",
                    program.service_name(op)
                ),
                (false, _) => format!(
                    "FAIL: {name}: {why}, and the old one could not be put back: it is {}",
                    previous(&installed).display()
                ),
            });
            return Err(lines);
        }
        lines.extend(program.after_start(op, began.elapsed()));
    }
    if let Err(e) = program.record(op, d.from_release, target, &installed) {
        lines.push(format!("WARNING: {name}: {e}"));
    }
    lines.push(program.say(op, Say::Done { target }));
    if d.from_release {
        match release::prune(op, program, &[target, &from]) {
            Ok(gone) if !gone.is_empty() => lines.push(format!(
                "{name}: removed old downloads: {}",
                gone.join(", ")
            )),
            Ok(_) => {}
            Err(e) => lines.push(format!("WARNING: {name}: {e}")),
        }
    }
    Ok(One {
        lines,
        changed: true,
    })
}
