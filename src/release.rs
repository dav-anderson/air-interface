//! Releases: which versions of a program exist upstream, which is the newest,
//! and what `upgrade check` says about them.
//!
//! **The source is a trait.** [`Source`] answers two questions: which version
//! tags a program has published, and, for one version, where its release binary
//! is (downloaded into a folder the operator gives it) and what SHA-256 the
//! release publishes for it. The operator does the rest itself: it ignores
//! pre-release and non-version tags, picks the newest, compares the downloaded
//! file's SHA-256 with the published one and refuses a build that differs. The
//! real source is [`crate::github::Github`] (GitHub releases, through `curl`).
//! [`NoSource`] knows nothing and is only for tests that need no upstream.
//!
//! **Versions** are read by [`crate::version::Tags`]: each program names its
//! own rule.

use std::fs;
use std::path::{Path, PathBuf};

use crate::exec::Exec;
use crate::files::backup_and_replace;
use crate::version::same_version;
use crate::{Managed, Operator, Say};

/// A release binary that was downloaded, and the SHA-256 its release publishes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Download {
    pub file: PathBuf,
    pub sha256: String,
}

/// Where releases come from.
pub trait Source<P: Managed> {
    /// The version tags `program` has published, as they are tagged (`v0.7.0`).
    /// `Err` with the reason when they cannot be listed.
    fn tags(&self, program: P) -> Result<Vec<String>, String>;

    /// Download the release binary of `program` at `version` for this machine
    /// into the folder `dir` (made if need be). `Err` with the reason when there
    /// is none or it cannot be downloaded.
    fn fetch(&self, program: P, version: &str, dir: &Path) -> Result<Download, String>;
}

/// No release source: nothing is known upstream.
pub struct NoSource;

impl<P: Managed> Source<P> for NoSource {
    fn tags(&self, _program: P) -> Result<Vec<String>, String> {
        Err(none_configured::<P>())
    }

    fn fetch(&self, _program: P, _version: &str, _dir: &Path) -> Result<Download, String> {
        Err(none_configured::<P>())
    }
}

fn none_configured<P: Managed>() -> String {
    format!(
        "no release source is configured in this {}",
        <P::Op as Operator>::NAME
    )
}

/// The newest release `source` knows for `program`. `Ok(None)` when it lists
/// none that is a release; `Err` when it cannot be asked.
pub fn newest_of<P: Managed>(source: &dyn Source<P>, program: P) -> Result<Option<String>, String> {
    Ok(program.tags().newest(&source.tags(program)?))
}

/// Where a downloaded release is kept.
pub fn releases_dir<P: Managed>(op: &P::Op, program: P, version: &str) -> PathBuf {
    op.dir().join("releases").join(program.name()).join(version)
}

/// Remove old downloads of `program` from `releases/`, oldest first (lowest
/// version first): the `release.keep` newest versions stay, and so do the
/// versions in `protect` (the installed one and the one it replaced). A `keep`
/// of 0 keeps every release: nothing is removed. A folder whose name is not a
/// version is left alone. Returns the versions removed, in the order they were
/// removed.
pub fn prune<P: Managed>(op: &P::Op, program: P, protect: &[&str]) -> Result<Vec<String>, String> {
    let keep = op.release_keep();
    if keep == 0 {
        return Ok(Vec::new());
    }
    let root = op.dir().join("releases").join(program.name());
    let Ok(entries) = fs::read_dir(&root) else {
        return Ok(Vec::new());
    };
    let rule = program.tags();
    let mut found: Vec<([u64; 3], String)> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            rule.numbers(&name).map(|n| (n, name))
        })
        .collect();
    found.sort_by_key(|f| f.0);
    let newest = found.len().saturating_sub(keep);
    let mut gone = Vec::new();
    for (i, (_, name)) in found.iter().enumerate() {
        if i >= newest || protect.iter().any(|p| same_version(p, name)) {
            continue;
        }
        let path = root.join(name);
        fs::remove_dir_all(&path).map_err(|e| format!("cannot remove {}: {e}", path.display()))?;
        gone.push(name.clone());
    }
    Ok(gone)
}

/// One line of `upgrade check`. The operator turns it into the JSON it prints
/// (the pin's key there is `spi_toml` or `aso_toml`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckLine {
    pub name: String,
    /// The version of the installed program, or `unknown`.
    pub installed: String,
    /// The `version` in the settings file, or `none`.
    pub pin: String,
    /// The newest tagged release, `unknown`, or `none` when upstream publishes
    /// no releases.
    pub newest: String,
    /// `up_to_date`, `upgrade_available`, `pinned`, `no_releases`, `unmanaged` or
    /// `unknown`.
    pub status: String,
    /// The same in words, with the reason when it is unknown.
    pub detail: String,
}

/// `upgrade check`: one line per program. Reads and runs `--version` only.
pub fn check<P: Managed>(op: &P::Op, exec: &dyn Exec, source: &dyn Source<P>) -> Vec<CheckLine> {
    P::all()
        .iter()
        .map(|&program| check_one(op, exec, source, program))
        .collect()
}

fn check_one<P: Managed>(
    op: &P::Op,
    exec: &dyn Exec,
    source: &dyn Source<P>,
    program: P,
) -> CheckLine {
    let installed = program.installed_version(op, exec);
    let pin = program.pin(op);
    let line = |newest: &str, status: &str, detail: String| CheckLine {
        name: program.name().to_string(),
        installed: installed.clone().unwrap_or_else(|_| "unknown".into()),
        pin: pin.clone().unwrap_or_else(|| "none".into()),
        newest: newest.to_string(),
        status: status.to_string(),
        detail,
    };
    if !program.publishes() {
        return line("none", "no_releases", program.say(op, Say::NoReleases));
    }
    if !program.managed(op) {
        return line("none", "unmanaged", program.say(op, Say::UnmanagedStatus));
    }
    let newest = match newest_of(source, program) {
        Ok(n) => n,
        Err(why) => return line("unknown", "unknown", format!("unknown ({why})")),
    };
    let Some(newest) = newest else {
        return line(
            "unknown",
            "unknown",
            "unknown (the release source lists no release for it)".into(),
        );
    };
    let installed = match &installed {
        Ok(v) => v.clone(),
        Err(why) => {
            return line(
                &newest,
                "unknown",
                format!("unknown (the installed version cannot be read: {why})"),
            );
        }
    };
    let target = pin.clone().unwrap_or_else(|| newest.clone());
    if same_version(&installed, &target) {
        match &pin {
            Some(p) if program.tags().is_newer(&newest, p) => line(
                &newest,
                "pinned",
                format!("pinned at {p} (newest {newest})"),
            ),
            _ => line(&newest, "up_to_date", "up to date".into()),
        }
    } else {
        line(
            &newest,
            "upgrade_available",
            format!("upgrade available ({target})"),
        )
    }
}

/// The last line of `upgrade check`.
pub fn summary(lines: &[CheckLine]) -> String {
    let count = |status: &str| lines.iter().filter(|l| l.status == status).count();
    format!(
        "summary: {} up to date, {} upgrade available, {} pinned, {} unknown, {} without upstream releases",
        count("up_to_date"),
        count("upgrade_available"),
        count("pinned"),
        count("unknown"),
        count("no_releases")
    )
}

/// Text for a person, one line per program, then the summary. `config_name` is
/// the settings file (`spi.toml`) and `name_width` the width of the name column.
pub fn render(lines: &[CheckLine], config_name: &str, name_width: usize) -> String {
    let mut out = String::new();
    for l in lines {
        out.push_str(&format!(
            "{:<name_width$} installed {:<9} {config_name} {:<9} newest {:<9} {}\n",
            l.name, l.installed, l.pin, l.newest, l.detail
        ));
    }
    out.push_str(&summary(lines));
    out.push('\n');
    out
}

/// A program pinned to a version that is not known to be the newest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pinned<P> {
    pub program: P,
    pub pin: String,
    /// The newest release; `None` when the source could not say.
    pub newest: Option<String>,
}

/// The pinned programs among `programs` whose pin is older than the newest
/// release, or whose newest release cannot be found. A program upstream
/// publishes nothing for (air_server) has no pin in this sense: its `version` is
/// the operator's word for the binary.
pub fn pinned<P: Managed>(op: &P::Op, source: &dyn Source<P>, programs: &[P]) -> Vec<Pinned<P>> {
    programs
        .iter()
        .filter(|p| p.publishes())
        .filter_map(|&program| {
            let pin = program.pin(op)?;
            let newest = newest_of(source, program).ok().flatten();
            let held = newest
                .as_deref()
                .is_none_or(|n| program.tags().is_newer(n, &pin));
            held.then_some(Pinned {
                program,
                pin,
                newest,
            })
        })
        .collect()
}

/// The warning that ends `upgrade check` and `upgrade status` and opens
/// `upgrade`: every pinned program, its pin and the newest release. Empty when
/// nothing is pinned behind the newest.
pub fn pin_warning<P: Managed>(held: &[Pinned<P>]) -> Vec<String> {
    if held.is_empty() {
        return Vec::new();
    }
    let (cli, config) = (<P::Op as Operator>::NAME, <P::Op as Operator>::CONFIG_NAME);
    let mut lines =
        vec!["WARNING: these programs are pinned and are not at the newest release:".to_string()];
    for h in held {
        lines.push(format!(
            "  {}: pinned at {} (newest {})",
            h.program.name(),
            h.pin,
            h.newest.as_deref().unwrap_or("unknown")
        ));
    }
    lines.push(format!(
        "`{cli} upgrade` installs the pin unless it is reset. It asks about each pin (--keep-pins keeps them all, --reset-pins resets them all); a reset removes the `version` line from {config} (the file is backed up first)."
    ));
    lines
}

/// `text` (a settings file) without the `version` line of `[section]`, the rest
/// as it is, comments and all. `None` when that section has no such line.
pub fn without_pin_line(text: &str, section: &str) -> Option<String> {
    let mut current = String::new();
    let mut out = String::with_capacity(text.len());
    let mut removed = false;
    for line in text.split_inclusive('\n') {
        let t = line.trim();
        if t.starts_with('[') && !t.starts_with("[[") {
            current = t
                .trim_start_matches('[')
                .split(']')
                .next()
                .unwrap_or("")
                .trim()
                .to_string();
        } else if !removed
            && current == section
            && t.strip_prefix("version")
                .is_some_and(|rest| rest.trim_start().starts_with('='))
        {
            removed = true;
            continue;
        }
        out.push_str(line);
    }
    removed.then_some(out)
}

/// Remove the pins of `programs` from the settings file `config`, after copying
/// it to `<config>.<epoch>.bak`; the newest `keep` backups stay, and the new
/// text replaces the file whole and at once. Returns what was done, a line per
/// program.
pub fn reset_pins<P: Managed>(
    config: &Path,
    programs: &[P],
    epoch: i64,
    keep: usize,
) -> Result<Vec<String>, String> {
    let text =
        fs::read_to_string(config).map_err(|e| format!("cannot read {}: {e}", config.display()))?;
    let mut edited = text.clone();
    let mut lines = Vec::new();
    let mut missing = Vec::new();
    for &program in programs {
        match without_pin_line(&edited, program.section()) {
            Some(next) => edited = next,
            None => missing.push(program),
        }
    }
    if edited != text {
        let backup = backup_and_replace(
            config,
            &edited,
            epoch,
            keep,
            <P::Op as Operator>::CONFIG_NAME,
        )?;
        lines.push(format!(
            "reset: removed the `version` pin of {} from {} (the old file is {})",
            programs
                .iter()
                .filter(|p| !missing.contains(p))
                .map(|p| p.name())
                .collect::<Vec<_>>()
                .join(", "),
            config.display(),
            backup.display()
        ));
    }
    for m in missing {
        lines.push(format!(
            "WARNING: {}: there is no `version` line under [{}] in {} to remove (a pin set in the environment is yours to unset); the newest release is still installed this time",
            m.name(),
            m.section(),
            config.display()
        ));
    }
    Ok(lines)
}
