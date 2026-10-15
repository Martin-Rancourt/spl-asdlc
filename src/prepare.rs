//! `spl-asdlc release prepare` (`docs/reference/05-release-cycle.md` §1): run by
//! the cut's `open` in a checkout of the new release branch, it reads the
//! range along the git graph, computes the next version, writes it into the
//! version files and the CHANGELOG, and writes the release notes. It writes
//! files only: the workflow commits them and tags the first candidate. It
//! never talks to GitHub nor to the tracker.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::Preparation;
use crate::method::release::prepare::{self, Commit};
use crate::method::release::shape::{Version, template_regex};

/// The CHANGELOG the notes are written into, at the repository's root.
pub const CHANGELOG: &str = "CHANGELOG.md";

/// What the command line asks.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// The repository's working tree.
    pub dir: PathBuf,
    /// A version forced instead of the computed one.
    pub version: Option<String>,
    /// Where the release notes are written.
    pub notes: Option<PathBuf>,
    /// The date of the CHANGELOG heading (`YYYY-MM-DD`), today (UTC) by default.
    pub date: Option<String>,
    /// `<owner>/<repo>` for the links, else `product.repository`.
    pub repository: Option<String>,
    /// Compute and print; write no file of the working tree.
    pub dry_run: bool,
}

/// What was prepared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prepared {
    pub version: Version,
    /// The release tag the version will be published under.
    pub tag: String,
    /// The last release the range starts from; `None` for a first release,
    /// measured from the repository's root.
    pub previous_tag: Option<String>,
    /// The commits of the range (merges left out).
    pub commits: usize,
    /// The files written (or that a dry run would write), in order.
    pub files: Vec<String>,
    /// The release notes: the CHANGELOG section of the version.
    pub notes: String,
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map_err(|e| format!("git could not run: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The highest release (by version) whose tag `HEAD` contains: the base of
/// the range. Read along the graph, never by date, so a release whose
/// merge-back reached the branch is found whatever the commits' dates.
pub fn last_release(dir: &Path, template: &str) -> Result<Option<(String, Version)>, String> {
    let re = template_regex(template);
    let tags = git(dir, &["tag", "--merged", "HEAD", "--list"])?;
    Ok(tags
        .lines()
        .filter_map(|t| {
            let c = re.captures(t.trim())?;
            Some((t.trim().to_string(), Version::parse(&c["version"])?))
        })
        .max_by_key(|(_, v)| *v))
}

/// The commits of `base..HEAD`, or of `HEAD`'s whole history without a
/// base; merge commits are left out (their changes are their parents').
pub fn range(dir: &Path, base: Option<&str>) -> Result<Vec<Commit>, String> {
    let spec = match base {
        Some(b) => format!("{b}..HEAD"),
        None => "HEAD".to_string(),
    };
    let log = git(
        dir,
        &["log", "--no-merges", "--format=%H%x1f%s%x1f%b%x1e", &spec],
    )?;
    Ok(log
        .split('\u{1e}')
        .filter_map(|record| {
            let record = record.trim_start_matches(['\n', '\r']);
            let mut parts = record.splitn(3, '\u{1f}');
            let sha = parts.next()?.trim();
            if sha.is_empty() {
                return None;
            }
            Some(Commit {
                sha: sha.to_string(),
                subject: parts.next().unwrap_or_default().trim().to_string(),
                body: parts.next().unwrap_or_default().trim().to_string(),
            })
        })
        .collect())
}

fn today() -> String {
    let d = time::OffsetDateTime::now_utc().date();
    format!("{:04}-{:02}-{:02}", d.year(), u8::from(d.month()), d.day())
}

fn read(dir: &Path, path: &str) -> Result<Option<String>, String> {
    match std::fs::read_to_string(dir.join(path)) {
        Ok(t) => Ok(Some(t)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{path} could not be read: {e}")),
    }
}

/// Prepare the release: compute everything first, then write; a refusal
/// writes nothing.
pub fn run(prep: &Preparation, o: &Options) -> Result<Prepared, String> {
    let dir = if o.dir.as_os_str().is_empty() {
        Path::new(".")
    } else {
        o.dir.as_path()
    };
    let repository = o
        .repository
        .clone()
        .or_else(|| prep.repository.clone())
        .ok_or("the links of the notes need the repository: pass --repository <owner>/<repo>")?;
    if repository.split('/').count() != 2 || repository.split('/').any(str::is_empty) {
        return Err(format!("--repository `{repository}` is not <owner>/<repo>"));
    }
    let date = o.date.clone().unwrap_or_else(today);
    if time::Date::parse(
        &date,
        time::macros::format_description!("[year]-[month]-[day]"),
    )
    .is_err()
    {
        return Err(format!("--date `{date}` is not YYYY-MM-DD"));
    }
    let previous = last_release(dir, &prep.release_tag)?;
    let commits = range(dir, previous.as_ref().map(|(t, _)| t.as_str()))?;
    let since = previous
        .as_ref()
        .map_or("the repository's root".to_string(), |(t, _)| t.clone());
    if commits.is_empty() {
        return Err(format!("nothing to release: no commit since {since}"));
    }
    let version = match &o.version {
        Some(forced) => {
            let v = Version::parse(forced)
                .ok_or_else(|| format!("--version `{forced}` is not X.Y.Z"))?;
            if let Some((tag, last)) = &previous {
                if v <= *last {
                    return Err(format!(
                        "--version {v} is not above the last release, {tag}"
                    ));
                }
            }
            v
        }
        None => {
            let bump = prepare::bump(&commits, &prep.changelog_sections).ok_or_else(|| {
                format!(
                    "nothing to release since {since}: none of its {} commit(s) is a Conventional Commit of a type the CHANGELOG lists, nor a breaking change (force a version with --version)",
                    commits.len()
                )
            })?;
            prepare::next_version(previous.as_ref().map(|(_, v)| *v), bump)
        }
    };
    let tag = prep.release_tag.replace("{version}", &version.to_string());
    if git(
        dir,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/tags/{tag}"),
        ],
    )
    .is_ok()
    {
        return Err(format!("{tag} already exists: {version} is released"));
    }
    let notes = prepare::section(
        version,
        previous.as_ref().map(|(t, _)| t.as_str()),
        &tag,
        &date,
        &commits,
        &prep.changelog_sections,
        &prepare::repository_url(&repository),
    );

    // Every new content first: a refusal writes nothing.
    let mut writes: Vec<(String, String)> = Vec::new();
    if let Some(manifest) = read(dir, "Cargo.toml")? {
        if let Some((old, text)) = prepare::set_cargo_toml(&manifest, version) {
            writes.push(("Cargo.toml".into(), text));
            if let Some(lock) = read(dir, "Cargo.lock")? {
                let text = prepare::set_cargo_lock(&lock, &old, version);
                if text != lock {
                    writes.push(("Cargo.lock".into(), text));
                }
            }
        }
    }
    for f in &prep.version_files {
        let current = writes
            .iter()
            .rev()
            .find(|(p, _)| *p == f.path)
            .map(|(_, t)| t.clone());
        let text = match current {
            Some(t) => t,
            None => read(dir, &f.path)?
                .ok_or_else(|| format!("{}: the version file does not exist", f.path))?,
        };
        let text = prepare::set_pattern(&text, f, version)?;
        writes.retain(|(p, _)| *p != f.path);
        writes.push((f.path.clone(), text));
    }
    let changelog = read(dir, CHANGELOG)?;
    writes.push((
        CHANGELOG.into(),
        prepare::insert(changelog.as_deref(), version, &notes),
    ));
    if !o.dry_run {
        for (path, text) in &writes {
            std::fs::write(dir.join(path), text)
                .map_err(|e| format!("{path} could not be written: {e}"))?;
        }
    }
    if let Some(n) = &o.notes {
        std::fs::write(n, &notes)
            .map_err(|e| format!("{} could not be written: {e}", n.display()))?;
    }
    Ok(Prepared {
        version,
        tag,
        previous_tag: previous.map(|(t, _)| t),
        commits: commits.len(),
        files: writes.into_iter().map(|(p, _)| p).collect(),
        notes,
    })
}

#[cfg(test)]
mod tests;
