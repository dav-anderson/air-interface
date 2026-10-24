//! air-minterface: the code that operates the programs an AIR runs.
//!
//! An AIR (a server for the Air protocol) runs next to programs that need
//! looking after: the AIR server itself, the mints, a Bitcoin node, a wallet.
//! Two operators look after them, `spi` (a Service Point Instance) and `aso`
//! (an AIR Service Operator). Both install the programs from releases, check
//! their checksums, upgrade them one at a time with a rollback, schedule an
//! upgrade for the night, and keep their logs under a cap. That code is here,
//! once.
//!
//! Nothing here knows a program. What differs between the operators is passed
//! in through two small traits:
//!
//! - [`Operator`]: one operator (`spi`, `aso`): its folders, whether it runs on
//!   macOS, where releases are listed, how many downloads it keeps, the name it
//!   uses in messages and the upgrade job's names and templates.
//! - [`Managed`]: one program the operator looks after (`bitcoind`, `fsp-mint`):
//!   its name, its settings section, its tag rule, where it is installed, how
//!   to restart it and whether it is serving, what to record when it was
//!   installed, and the few sentences whose wording is the operator's own.
//!
//! The upgrade commands themselves (prompts, the order of questions, what is
//! printed around them) stay in each operator; this crate has the parts they
//! share. See DESIGN.md for what belongs here.

use std::path::{Path, PathBuf};
use std::time::Duration;

pub mod air;
pub mod exec;
pub mod files;
pub mod github;
pub mod logs;
pub mod record;
pub mod release;
pub mod schedule;
pub mod template;
pub mod upgrade;
pub mod version;

pub use upgrade::{Probe, Say};

use exec::Exec;
use version::Tags;

/// The version of this crate (shown by `spi versions` and `aso versions`).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// One operator: `spi` or `aso`.
pub trait Operator {
    /// The operator's name in commands and messages (`spi`).
    const NAME: &'static str;
    /// Its settings file (`spi.toml`).
    const CONFIG_NAME: &'static str;
    /// The scheduled-upgrade job: names, templates and wording.
    const UPGRADE_JOB: schedule::Job;

    /// The operator's own folder (releases, records, the pending upgrade).
    fn dir(&self) -> &Path;
    /// The home folder service files are checked against.
    fn home(&self) -> &str;
    /// Whether this is macOS (launchd) rather than Linux (systemd).
    fn mac(&self) -> bool;
    /// The folder the logs go in.
    fn log_dir(&self) -> PathBuf;
    /// The service manager's folder for job files: `/etc/systemd/system` or
    /// `/Library/LaunchDaemons`.
    fn unit_dir(&self) -> PathBuf;
    /// The GitHub API's address (`https://api.github.com`).
    fn release_url(&self) -> String;
    /// How many downloaded releases of a program are kept.
    fn release_keep(&self) -> usize;
}

/// One program an operator looks after. The operator's program enum implements
/// it. Everything with a default is for a program that needs nothing special.
pub trait Managed: Copy + PartialEq + std::fmt::Debug + 'static {
    /// The operator this program belongs to.
    type Op: Operator;

    /// Every program, in the order a full upgrade goes in.
    fn all() -> &'static [Self];
    /// The program's name: its file name, its repository and its name in messages.
    fn name(self) -> &'static str;
    /// The settings section that holds the program's `bin` and `version`.
    fn section(self) -> &'static str;
    /// Whether upstream publishes release binaries for it.
    fn publishes(self) -> bool;
    /// Which release tags count as versions.
    fn tags(self) -> Tags;
    /// Where the service install keeps the program's binary.
    fn installed_path(self, op: &Self::Op) -> PathBuf;
    /// The `bin` setting: the operator's own file for it.
    fn bin(self, op: &Self::Op) -> String;
    /// The pin: the `version` the settings name for it, if any.
    fn pin(self, op: &Self::Op) -> Option<String>;

    /// Whether the operator manages the program at all (spi's `manage = false`).
    fn managed(self, _op: &Self::Op) -> bool {
        true
    }
    /// Whether the operator supplied the build itself, so nothing is downloaded
    /// (spi's `[bitcoind] bin`).
    fn own_build(self, _op: &Self::Op) -> bool {
        false
    }
    /// Whether the program runs as a service. One that does not (a wallet run
    /// on demand) is replaced between two runs and never restarted.
    fn is_service(self) -> bool {
        true
    }
    /// Whether the program prints its own version. One that does not can only
    /// be told apart by the operator's record of the exact file.
    fn prints_version(self) -> bool {
        true
    }

    /// The version of the installed program, or why it cannot be read.
    fn installed_version(self, op: &Self::Op, exec: &dyn Exec) -> Result<String, String>;
    /// The version the program reports for the binary at `path`, or why it
    /// cannot.
    fn reported(self, op: &Self::Op, exec: &dyn Exec, path: &Path) -> Result<String, String>;

    /// Whether what a restarted program answered shows it serving the build
    /// that was installed.
    fn serving(self, probe: &Probe, _target: &str) -> bool {
        probe.up
    }
    /// Why a restarted program is not serving yet, for the line that says it was
    /// rolled back.
    fn not_serving(self, _probe: &Probe, _target: &str, wait_secs: u64) -> String {
        format!("the new build did not become healthy within {wait_secs} s")
    }
    /// Restart the program through the service manager.
    fn restart(self, op: &Self::Op, exec: &dyn Exec) -> Result<(), String>;
    /// The service's name, for "restart it yourself".
    fn service_name(self, op: &Self::Op) -> String;
    /// Lines to add once the restarted program is healthy, after `took`
    /// (aso says how long the service was down and marks its settings applied).
    fn after_start(self, _op: &Self::Op, _took: Duration) -> Vec<String> {
        Vec::new()
    }
    /// Record what was installed. `from_release` says whether the build came
    /// from the release source (else it was the operator's own file).
    fn record(
        self,
        op: &Self::Op,
        from_release: bool,
        version: &str,
        installed: &Path,
    ) -> Result<(), String>;
    /// The sentences whose wording is the operator's own; see [`Say`].
    fn say(self, op: &Self::Op, what: Say<'_>) -> String;

    /// The program with this name.
    fn parse(text: &str) -> Option<Self> {
        Self::all().iter().copied().find(|p| p.name() == text)
    }
}
