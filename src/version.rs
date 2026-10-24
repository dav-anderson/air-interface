//! Versions: what a release tag is, which tag is newer, whether two versions
//! are the same number, and the lines `versions` prints.
//!
//! **Tags.** A tag is `x.y.z` (or, where [`Tags::Loose`] says so, `x.y`), with
//! or without a leading `v`. A tag with anything else in it (`0.7.0-rc1`,
//! `29.0rc1`) is a pre-release or not a version, and is ignored: a release
//! candidate is never installed. Under `Loose`, `x.y` and `x.y.0` are the same
//! number.

use serde::Serialize;

/// Which tags are versions. spi takes `x.y` too (Bitcoin Core tags `29.1`); aso
/// does not (fsp-mint and ecash-wallet are `x.y.z`, and a `0.8` tag is ignored).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tags {
    /// `x.y.z` only.
    Strict,
    /// `x.y` or `x.y.z`.
    Loose,
}

impl Tags {
    /// The numbers of a release tag, digits only, with an optional leading `v`.
    /// `None` for anything else.
    pub fn numbers(self, tag: &str) -> Option<[u64; 3]> {
        let text = tag.strip_prefix('v').unwrap_or(tag);
        let mut out = Vec::new();
        for part in text.split('.') {
            if part.is_empty() || !part.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            out.push(part.parse::<u64>().ok()?);
        }
        let allowed = match self {
            Tags::Strict => 3..=3,
            Tags::Loose => 2..=3,
        };
        if !allowed.contains(&out.len()) {
            return None;
        }
        out.resize(3, 0);
        Some([out[0], out[1], out[2]])
    }

    /// Whether `a` is a higher version than `b`. A tag that is not a version is
    /// never newer than anything.
    pub fn is_newer(self, a: &str, b: &str) -> bool {
        match (self.numbers(a), self.numbers(b)) {
            (Some(x), Some(y)) => x > y,
            _ => false,
        }
    }

    /// The newest release among `tags`, without its `v`. Pre-release and
    /// non-version tags are ignored.
    pub fn newest(self, tags: &[String]) -> Option<String> {
        tags.iter()
            .filter_map(|t| {
                self.numbers(t)
                    .map(|n| (n, t.strip_prefix('v').unwrap_or(t).to_string()))
            })
            .max_by_key(|(n, _)| *n)
            .map(|(_, t)| t)
    }
}

/// Whether two versions are the same number: `1.0` and `1.0.0` are.
pub fn same_version(a: &str, b: &str) -> bool {
    let parts = |v: &str| -> Option<Vec<u64>> {
        let mut n: Vec<u64> = v
            .split('.')
            .map(|p| p.parse().ok())
            .collect::<Option<Vec<_>>>()?;
        while n.len() < 3 {
            n.push(0);
        }
        Some(n)
    };
    match (parts(a), parts(b)) {
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

/// The program's own version from the first line of its `--version`:
/// `<name> <version>`.
pub fn program_version(name: &str, first_line: &str) -> Option<String> {
    let mut words = first_line.split_whitespace();
    (words.next()? == name).then(|| words.next().map(str::to_string))?
}

/// One line of `versions`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Line {
    pub name: String,
    /// `x.y.z`, or `unknown`.
    pub version: String,
    /// Where the version came from, or why it is unknown.
    pub note: String,
}

/// A line with a version.
pub fn known(name: &str, version: &str, note: &str) -> Line {
    Line {
        name: name.into(),
        version: version.into(),
        note: note.into(),
    }
}

/// A line whose version is `unknown`, with the reason.
pub fn unknown(name: &str, why: impl Into<String>) -> Line {
    known(name, "unknown", &why.into())
}

/// Text for a person.
pub fn render(lines: &[Line]) -> String {
    let mut out = String::new();
    for l in lines {
        out.push_str(&format!("{:<32} {:<10} {}\n", l.name, l.version, l.note));
    }
    out
}

/// The line that names this crate: `versions` shows it in both operators.
pub fn crate_line() -> Line {
    known(
        "air-interface",
        crate::VERSION,
        "the shared code that installs, upgrades, schedules and logs the programs",
    )
}
