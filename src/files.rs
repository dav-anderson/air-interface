//! Files, written the one safe way: a whole file at once. A write goes to a
//! temporary file beside the target and is renamed over it, so a crash in the
//! middle leaves the old file, never half of a new one. Also the file modes
//! (macOS and Linux; the mode calls exist on Unix only, so they are here, behind
//! one `cfg`), the SHA-256 of a file, the binary a setting names, and the
//! `<name>.<epoch>.bak` copies an edit of a settings file leaves.

use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// Write `bytes` to `path` as a whole: a temporary file beside it, then a rename.
pub fn replace_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    fs::rename(&tmp, path).map_err(|e| format!("cannot replace {}: {e}", path.display()))
}

/// The SHA-256 of a file, as hex.
pub fn sha256_file(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    Ok(Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// Set the permission bits of `path`.
#[cfg(unix)]
pub fn set_mode(path: &Path, mode: u32) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|e| format!("cannot set the mode of {}: {e}", path.display()))
}

/// Where there are no mode bits there is nothing to set.
#[cfg(not(unix))]
pub fn set_mode(_path: &Path, _mode: u32) -> Result<(), String> {
    Ok(())
}

/// Mark a file executable (mode 0755).
pub fn make_executable(path: &Path) -> Result<(), String> {
    set_mode(path, 0o755)
}

/// The binary `spec` names (a path, or a name on PATH), as an absolute path.
pub fn find_binary(spec: &str) -> Option<PathBuf> {
    let found = if spec.contains('/') {
        let p = PathBuf::from(spec);
        p.is_file().then_some(p)
    } else {
        let path = std::env::var_os("PATH")?;
        std::env::split_paths(&path)
            .map(|d| d.join(spec))
            .find(|p| p.is_file())
    };
    found.and_then(|p| fs::canonicalize(p).ok())
}

/// Keep the newest `keep` `<name>.<epoch>.bak` files next to `config`. A `keep`
/// of 0 keeps all of them: nothing is removed.
pub fn prune_backups(config: &Path, name: &str, keep: usize) {
    if keep == 0 {
        return;
    }
    let dir = config.parent().filter(|d| !d.as_os_str().is_empty());
    let mut found: Vec<(i64, PathBuf)> = fs::read_dir(dir.unwrap_or(Path::new(".")))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let file = e.file_name().to_string_lossy().to_string();
            let epoch = file
                .strip_prefix(name)?
                .strip_prefix('.')?
                .strip_suffix(".bak")?;
            Some((epoch.parse::<i64>().ok()?, e.path()))
        })
        .collect();
    found.sort_by_key(|f| std::cmp::Reverse(f.0));
    for (_, path) in found.into_iter().skip(keep) {
        let _ = fs::remove_file(path);
    }
}

/// Copy `config` to `<config>.<epoch>.bak`, replace it with `edited`, and keep
/// the newest `keep` backups (all of them when `keep` is 0). Returns the
/// backup's path. `fallback_name` is the
/// backup's base name for a path that has none.
pub fn backup_and_replace(
    config: &Path,
    edited: &str,
    epoch: i64,
    keep: usize,
    fallback_name: &str,
) -> Result<PathBuf, String> {
    let name = config
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| fallback_name.to_string());
    let backup = config.with_file_name(format!("{name}.{epoch}.bak"));
    if config.is_file() {
        fs::copy(config, &backup)
            .map_err(|e| format!("cannot back up {}: {e}", config.display()))?;
    }
    replace_file(config, edited.as_bytes())?;
    prune_backups(config, &name, keep);
    Ok(backup)
}
