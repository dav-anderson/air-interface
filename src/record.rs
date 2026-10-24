//! What an operator recorded when it put a program in place: the version and the
//! SHA-256 of the file. A program that prints no version (air_server, and
//! fsp-mint in aso) is known only through this record, and only for the exact
//! file it was written for: a binary whose hash differs has no known version.
//!
//! Where a record lives is the caller's: spi keeps air_server's at
//! `<air folder>/installed.json` and the rest under `<spi folder>/installed/`,
//! aso keeps all of them under `<aso folder>/installed/`.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::files::{replace_file, sha256_file};

/// The record: what was installed, and the hash of the file it was.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Installed {
    pub version: String,
    pub sha256: String,
}

/// Write the record at `path`: `binary` is the file now installed and `version`
/// what it is. The file is replaced whole and at once.
pub fn write(path: &Path, version: &str, binary: &Path) -> Result<(), String> {
    let record = Installed {
        version: version.to_string(),
        sha256: sha256_file(binary)?,
    };
    let text = serde_json::to_string_pretty(&record).map_err(|e| e.to_string())?;
    replace_file(path, format!("{text}\n").as_bytes())
}

/// The record at `path`, if there is one that reads.
pub fn read(path: &Path) -> Option<Installed> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

/// The version recorded at `path` for exactly this `binary`, if it was.
pub fn recorded(path: &Path, binary: &Path) -> Option<String> {
    let record = read(path)?;
    (sha256_file(binary).ok()? == record.sha256).then_some(record.version)
}

/// Remove the record at `path`: a record that no longer describes the file.
/// A record that is not there is fine.
pub fn remove(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("cannot remove the old record: {e}")),
    }
}
