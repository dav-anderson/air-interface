//! A stub operator and a stub program, so that the shared code is tested without
//! spi or aso: an `Operator` (`Stub`, with its folders in a scratch folder), a
//! `Managed` program (`Prog::Alpha`), a runner that records what it is asked, a
//! release source that serves from memory, and a `curl` that serves files from
//! memory.

// Each test file uses a different part of these helpers, so every file that
// includes this module sees the rest as unused.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use air_minterface::exec::{Exec, Ran, System};
use air_minterface::files::sha256_file;
use air_minterface::record;
use air_minterface::release::{Download, Source};
use air_minterface::schedule::Job;
use air_minterface::version::{Tags, program_version};
use air_minterface::{Managed, Operator, Probe, Say};

/// A folder of its own for every call.
pub fn scratch(name: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!("air-minterface-{}-{n}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

// ---- the operator and its one program ----

pub struct Stub {
    pub dir: PathBuf,
    pub mac: bool,
    pub log_dir: PathBuf,
    pub pin: Option<String>,
    /// `release.keep`: 2 unless a test sets it.
    pub keep: usize,
    pub restarts: Mutex<Vec<String>>,
}

impl Operator for Stub {
    const NAME: &'static str = "stub";
    const CONFIG_NAME: &'static str = "stub.toml";
    const UPGRADE_JOB: Job = Job {
        unit: "st-upgrade",
        label: "org.example.upgrade",
        service: include_str!("stub.service"),
        timer: include_str!("stub.timer"),
        plist: include_str!("stub.plist"),
        all_programs: "alpha (the only one)",
        log_name: "stub-upgrade.log",
    };

    fn dir(&self) -> &Path {
        &self.dir
    }
    fn home(&self) -> &str {
        "/home/test"
    }
    fn mac(&self) -> bool {
        self.mac
    }
    fn log_dir(&self) -> PathBuf {
        self.log_dir.clone()
    }
    fn unit_dir(&self) -> PathBuf {
        self.dir.join("units")
    }
    fn release_url(&self) -> String {
        "https://releases.example".into()
    }
    fn release_keep(&self) -> usize {
        self.keep
    }
}

/// A stub operator in a scratch folder of its own, on Linux or macOS.
pub fn stub(name: &str, mac: bool) -> Stub {
    let dir = scratch(name);
    std::fs::create_dir_all(dir.join("bin")).unwrap();
    Stub {
        log_dir: dir.join("logs"),
        dir,
        mac,
        pin: None,
        keep: 2,
        restarts: Mutex::new(Vec::new()),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prog {
    Alpha,
}

pub fn record_path(op: &Stub) -> PathBuf {
    op.dir.join("installed/alpha.json")
}

impl Managed for Prog {
    type Op = Stub;

    fn all() -> &'static [Prog] {
        &[Prog::Alpha]
    }
    fn name(self) -> &'static str {
        "alpha"
    }
    fn section(self) -> &'static str {
        "alpha"
    }
    fn publishes(self) -> bool {
        true
    }
    fn tags(self) -> Tags {
        Tags::Strict
    }
    fn installed_path(self, op: &Stub) -> PathBuf {
        op.dir.join("bin/alpha")
    }
    fn bin(self, op: &Stub) -> String {
        op.dir.join("build/alpha").display().to_string()
    }
    fn pin(self, op: &Stub) -> Option<String> {
        op.pin.clone()
    }
    fn installed_version(self, op: &Stub, exec: &dyn Exec) -> Result<String, String> {
        self.reported(op, exec, &self.installed_path(op))
    }
    /// A program file is its own text: `alpha 0.5.0` on the first line.
    fn reported(self, _op: &Stub, _exec: &dyn Exec, path: &Path) -> Result<String, String> {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        program_version("alpha", text.lines().next().unwrap_or(""))
            .ok_or_else(|| format!("{} prints no version", path.display()))
    }
    fn restart(self, op: &Stub, _exec: &dyn Exec) -> Result<(), String> {
        op.restarts.lock().unwrap().push("alpha".to_string());
        Ok(())
    }
    fn service_name(self, _op: &Stub) -> String {
        "stub-alpha".into()
    }
    fn record(self, op: &Stub, release: bool, version: &str, file: &Path) -> Result<(), String> {
        if release {
            record::write(&record_path(op), version, file)
        } else {
            Ok(())
        }
    }
    fn say(self, _op: &Stub, what: Say<'_>) -> String {
        match what {
            Say::Plan { from, target } => format!("alpha: {from} -> {target}"),
            Say::Done { target } => format!("alpha: now {target}, healthy"),
            other => format!("alpha: {other:?}"),
        }
    }
}

/// Write the installed program `p` with the version `v`.
pub fn install(op: &Stub, p: Prog, v: &str) {
    std::fs::write(p.installed_path(op), format!("alpha {v}\n")).unwrap();
}

/// A health answer: up and reporting `v`.
pub fn serving(v: &'static str) -> impl Fn(Prog) -> Probe {
    move |_| Probe::reporting(v)
}

pub fn text(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

// ---- the runner ----

type Answer = Box<dyn Fn(&str, &[String]) -> Result<Ran, String> + Send + Sync>;

/// Programs answered by a closure; every call is kept as one line.
pub struct Fake {
    answer: Answer,
    pub calls: Mutex<Vec<String>>,
}

impl Exec for Fake {
    fn run(&self, bin: &str, args: &[String], _env: &[(String, String)]) -> Result<Ran, String> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("{bin} {}", args.join(" ")));
        (self.answer)(bin, args)
    }
}

pub fn fake<F>(answer: F) -> Fake
where
    F: Fn(&str, &[String]) -> Result<Ran, String> + Send + Sync + 'static,
{
    Fake {
        answer: Box::new(answer),
        calls: Mutex::new(Vec::new()),
    }
}

pub fn ran(code: i32, stdout: &str, stderr: &str) -> Result<Ran, String> {
    Ok(Ran {
        code: Some(code),
        stdout: stdout.to_string(),
        stderr: stderr.to_string(),
    })
}

/// A runner that accepts every command.
pub fn accepting() -> Fake {
    fake(|_, _| ran(0, "", ""))
}

// ---- the release source ----

/// Alpha's tags and builds served from memory. `fetch` writes the build into the
/// folder it is given and says the SHA-256 of what it wrote, unless the build
/// publishes another one (a corrupt download).
pub struct FakeSource {
    tags: Vec<String>,
    /// (version, the file's text, a SHA-256 to publish instead).
    builds: Vec<(String, String, Option<String>)>,
    /// `"alpha <version>"` for each fetch asked for, in order.
    pub fetched: Mutex<Vec<String>>,
}

impl FakeSource {
    pub fn new() -> FakeSource {
        FakeSource {
            tags: Vec::new(),
            builds: Vec::new(),
            fetched: Mutex::new(Vec::new()),
        }
    }

    pub fn tags(mut self, tags: &[&str]) -> FakeSource {
        self.tags = tags.iter().map(|t| t.to_string()).collect();
        self
    }

    pub fn build(self, version: &str, text: &str) -> FakeSource {
        self.corrupt_build(version, text, None)
    }

    pub fn corrupt_build(mut self, version: &str, text: &str, sha: Option<&str>) -> FakeSource {
        let sha = sha.map(str::to_string);
        self.builds.push((version.into(), text.into(), sha));
        self
    }
}

impl Source<Prog> for FakeSource {
    fn tags(&self, _program: Prog) -> Result<Vec<String>, String> {
        Ok(self.tags.clone())
    }

    fn fetch(&self, _program: Prog, version: &str, dir: &Path) -> Result<Download, String> {
        self.fetched
            .lock()
            .unwrap()
            .push(format!("alpha {version}"));
        let (_, text, sha) = self
            .builds
            .iter()
            .find(|(v, _, _)| v == version)
            .ok_or_else(|| format!("no release binary of alpha {version}"))?;
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let file = dir.join("alpha");
        std::fs::write(&file, text).map_err(|e| e.to_string())?;
        let sha256 = sha.clone().map_or_else(|| sha256_file(&file), Ok)?;
        Ok(Download { file, sha256 })
    }
}

// ---- curl and tar ----

/// `curl` serving files from memory by URL (a URL it does not have is a 404, as
/// `curl -f` reports it), and the system's `tar`.
pub struct Web {
    pub files: Vec<(String, Vec<u8>)>,
    pub asked: Mutex<Vec<String>>,
}

impl Exec for Web {
    fn run(&self, bin: &str, args: &[String], env: &[(String, String)]) -> Result<Ran, String> {
        if bin != "curl" {
            return System.run(bin, args, env);
        }
        let url = args.last().cloned().unwrap_or_default();
        self.asked.lock().unwrap().push(url.clone());
        let Some((_, body)) = self.files.iter().find(|(u, _)| *u == url) else {
            return ran(22, "", "curl: (22) The requested URL returned error: 404\n");
        };
        match args.iter().position(|a| a == "-o") {
            Some(at) => {
                std::fs::write(&args[at + 1], body).map_err(|e| e.to_string())?;
                ran(0, "", "")
            }
            None => ran(0, &String::from_utf8_lossy(body), ""),
        }
    }
}

pub fn web(files: Vec<(String, Vec<u8>)>) -> Web {
    Web {
        files,
        asked: Mutex::new(Vec::new()),
    }
}

/// A `.tar.gz` holding the one file `path` (text `text`), made with the
/// system's tar.
pub fn tarball(path: &str, text: &str) -> Vec<u8> {
    let dir = scratch("tarball");
    let file = dir.join("in").join(path);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(file, text).unwrap();
    let made = std::process::Command::new("tar")
        .current_dir(dir.join("in"))
        .args(["-czf", "../out.tar.gz", "."])
        .status()
        .unwrap();
    assert!(made.success());
    std::fs::read(dir.join("out.tar.gz")).unwrap()
}

/// The SHA-256 of `bytes`.
pub fn sha(bytes: &[u8]) -> String {
    let file = scratch("sha").join("f");
    std::fs::write(&file, bytes).unwrap();
    sha256_file(&file).unwrap()
}
