//! The real release source: releases are listed and downloaded with `curl`, run
//! through the same [`Exec`] as every other program (no HTTP crate).
//!
//! The GitHub API lists a repository's releases (`dav-anderson/<program>`) and
//! their assets. The archive is `<program>-<version>-<os>-<arch>.tar.gz` (`os`
//! is `linux` or `macos`, `arch` is `x86_64` or `aarch64`) and the release
//! carries a `SHA256SUMS` asset. The archive's SHA-256 is checked against it
//! here, before anything is unpacked. A release marked `prerelease` is never
//! offered and never installed, not even when it is pinned.
//!
//! [`Github::fetch_with`] is the part every source shares (download, checksum,
//! unpack, move, make executable): spi uses it for bitcoind too, whose archive
//! comes from bitcoincore.org and whose `SHA256SUMS` is signed.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::exec::Exec;
use crate::files::{make_executable, sha256_file};
use crate::release::{Download, Source};
use crate::{Managed, Operator};

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

#[derive(Deserialize)]
struct Rel {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<Asset>,
}

/// Where an archive is: its file name, its URL, and the URL of the `SHA256SUMS`
/// that lists it.
pub struct Archive {
    pub name: String,
    pub url: String,
    pub sums_url: String,
}

/// GitHub releases, through `curl`.
pub struct Github<'a> {
    exec: &'a dyn Exec,
    /// The operator's name in messages (`spi`, `aso`).
    who: &'static str,
    api: String,
    /// `linux` or `macos`.
    pub os: &'static str,
    /// `x86_64` or `aarch64`.
    pub arch: String,
}

impl<'a> Github<'a> {
    /// The source for this machine.
    pub fn new<O: Operator>(op: &O, exec: &'a dyn Exec) -> Github<'a> {
        let os = if op.mac() { "macos" } else { "linux" };
        Github::for_machine(op, exec, os, std::env::consts::ARCH)
    }

    /// The source for a machine of `os` and `arch` (the tests name one).
    pub fn for_machine<O: Operator>(
        op: &O,
        exec: &'a dyn Exec,
        os: &'static str,
        arch: &str,
    ) -> Github<'a> {
        Github {
            exec,
            who: O::NAME,
            api: op.release_url(),
            os,
            arch: arch.to_string(),
        }
    }

    /// `curl -fsSL` with `args`; the body on success.
    pub fn curl(&self, args: &[&str]) -> Result<String, String> {
        let mut all: Vec<String> = ["-fsSL", "--max-time", "900"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        all.extend(args.iter().map(|s| s.to_string()));
        let ran = self.exec.run("curl", &all, &[]).map_err(|e| {
            format!(
                "{e} ({} downloads releases with curl: install curl, or put the build in place and name it with `bin`)",
                self.who
            )
        })?;
        if ran.ok() {
            Ok(ran.stdout)
        } else {
            Err(format!(
                "curl {}: {}",
                args.last().unwrap_or(&""),
                ran.last_line()
            ))
        }
    }

    fn releases(&self, program: &str) -> Result<Vec<Rel>, String> {
        let url = format!(
            "{}/repos/dav-anderson/{program}/releases?per_page=100",
            self.api
        );
        let body = self.curl(&["-H", "Accept: application/vnd.github+json", &url])?;
        let list: Vec<Rel> = serde_json::from_str(&body)
            .map_err(|e| format!("the release list at {url} is not what GitHub sends: {e}"))?;
        Ok(list.into_iter().filter(|r| !r.draft).collect())
    }

    /// The version tags of `program`'s repository, pre-releases left out.
    pub fn tags_of(&self, program: &str) -> Result<Vec<String>, String> {
        Ok(self
            .releases(program)?
            .into_iter()
            .filter(|r| !r.prerelease)
            .map(|r| r.tag_name)
            .collect())
    }

    /// The archive of `program` at `version` for this machine.
    fn archive(&self, program: &str, version: &str) -> Result<Archive, String> {
        let list = self.releases(program)?;
        let Some(rel) = list
            .into_iter()
            .find(|r| r.tag_name.strip_prefix('v').unwrap_or(&r.tag_name) == version)
        else {
            return Err(format!("{program} has no release {version}"));
        };
        if rel.prerelease {
            return Err(format!(
                "{program} {version} is marked as a pre-release on GitHub; {} installs releases only",
                self.who
            ));
        }
        let name = format!("{program}-{version}-{}-{}.tar.gz", self.os, self.arch);
        let find = |wanted: &str| {
            rel.assets
                .iter()
                .find(|a| a.name == wanted)
                .map(|a| a.browser_download_url.clone())
        };
        let Some(url) = find(&name) else {
            let have: Vec<&str> = rel.assets.iter().map(|a| a.name.as_str()).collect();
            return Err(format!(
                "release {} of {program} has no archive for this machine ({} on {}): expected {name}, it has {}",
                rel.tag_name,
                self.os,
                self.arch,
                have.join(", ")
            ));
        };
        let Some(sums_url) = find("SHA256SUMS") else {
            return Err(format!(
                "release {} of {program} has no SHA256SUMS",
                rel.tag_name
            ));
        };
        Ok(Archive {
            name,
            url,
            sums_url,
        })
    }

    /// Download `archive`, check it against its `SHA256SUMS`, unpack it into
    /// `dir`, and leave `dir/<program>` executable. `verify` is shown the
    /// `SHA256SUMS` text and its URL first and may refuse them (spi checks
    /// Bitcoin Core's signatures there).
    pub fn fetch_with(
        &self,
        program: &str,
        archive: &Archive,
        dir: &Path,
        verify: &dyn Fn(&str, &str) -> Result<(), String>,
    ) -> Result<Download, String> {
        let Archive {
            name,
            url,
            sums_url,
        } = archive;
        let sums = self.curl(&[sums_url])?;
        verify(&sums, sums_url)?;
        let Some(published) = sum_for(&sums, name) else {
            return Err(format!("{sums_url} does not list {name}"));
        };
        fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        let file_archive = dir.join(name);
        let unpacked = dir.join("unpacked");
        let result = (|| {
            self.curl(&["-o", &file_archive.display().to_string(), url])?;
            let have = sha256_file(&file_archive)?;
            if !have.eq_ignore_ascii_case(&published) {
                return Err(format!(
                    "the download of {name} failed its checksum (SHA256SUMS says {published}, the file is {have}): it was deleted and nothing was changed"
                ));
            }
            let args = [
                "-xzf".to_string(),
                file_archive.display().to_string(),
                "-C".into(),
                unpacked.display().to_string(),
            ];
            fs::create_dir_all(&unpacked).map_err(|e| e.to_string())?;
            let ran = self.exec.run("tar", &args, &[])?;
            if !ran.ok() {
                return Err(format!("cannot unpack {name}: {}", ran.last_line()));
            }
            let found = find_file(&unpacked, program)
                .ok_or_else(|| format!("{name} holds no file called {program}"))?;
            let file = dir.join(program);
            fs::rename(&found, &file)
                .map_err(|e| format!("cannot move {}: {e}", found.display()))?;
            make_executable(&file)?;
            Ok(Download {
                sha256: sha256_file(&file)?,
                file,
            })
        })();
        let _ = fs::remove_file(&file_archive);
        let _ = fs::remove_dir_all(&unpacked);
        result
    }

    /// Download `program` at `version` from its GitHub release.
    pub fn fetch_release(
        &self,
        program: &str,
        version: &str,
        dir: &Path,
    ) -> Result<Download, String> {
        let archive = self.archive(program, version)?;
        self.fetch_with(program, &archive, dir, &|_, _| Ok(()))
    }
}

/// The SHA-256 `SHA256SUMS` (`<hex>  <name>` lines) gives for `name`.
fn sum_for(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hex, file) = line.split_once(char::is_whitespace)?;
        (file.trim().trim_start_matches('*') == name).then(|| hex.to_string())
    })
}

/// The first file called `name` under `dir`.
fn find_file(dir: &Path, name: &str) -> Option<PathBuf> {
    for entry in fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_file(&path, name) {
                return Some(found);
            }
        } else if path.file_name().is_some_and(|n| n == name) {
            return Some(path);
        }
    }
    None
}

impl<P: Managed> Source<P> for Github<'_> {
    fn tags(&self, program: P) -> Result<Vec<String>, String> {
        if !program.publishes() {
            return Err(format!("{} publishes no releases", program.name()));
        }
        self.tags_of(program.name())
    }

    fn fetch(&self, program: P, version: &str, dir: &Path) -> Result<Download, String> {
        self.fetch_release(program.name(), version, dir)
    }
}
