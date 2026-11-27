//! `spl-asdlc release patch --break-glass` (ASDLC-KEUY): the patch the
//! bridge cannot ship because the bridge itself is broken. In a checkout, it
//! opens the patch's release branch from the tag of the release in
//! production, cherry-picks the named commits, each already merged into the
//! default branch, prepares the patch version and its notes
//! (`release prepare`), commits the release and runs the project's tests.
//! It stops before the tag: it pushes nothing, tags nothing, and prints the
//! exact commands a person runs to give the go (business rule L7BG). It talks to
//! no service but the git remote it fetches from.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::Preparation;
use crate::method::release::shape::{Version, template_regex};
use crate::prepare;

/// What the command line asks.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// The repository's working tree.
    pub dir: PathBuf,
    /// The commits the patch carries, each merged into the default branch.
    pub commits: Vec<String>,
    /// The release tag in production; default: the highest release tag.
    pub from: Option<String>,
    /// The project's tests, each run with `sh -c` in the working tree.
    pub tests: Vec<String>,
    /// The remote the default branch is read from and the go pushes to.
    pub remote: String,
    /// The default branch; default: the remote's `HEAD`, else `main`.
    pub default_branch: Option<String>,
    /// `<owner>/<repo>` for the links of the notes, else `product.repository`.
    pub repository: Option<String>,
    /// The date of the CHANGELOG heading (`YYYY-MM-DD`), today (UTC) by default.
    pub date: Option<String>,
    /// The release branch's stamp (`YYYY-MM-DD-HH-MM`), now (UTC) by default.
    pub stamp: Option<String>,
    /// Read the remote as it was last fetched, without fetching.
    pub no_fetch: bool,
}

/// A patch ready for the go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ready {
    /// The release in production the patch starts from.
    pub from: String,
    pub version: Version,
    /// The release tag the go creates.
    pub tag: String,
    /// The patch's release branch, local only.
    pub branch: String,
    /// The release commit at the branch's head: the commit the go tags.
    pub head: String,
    /// The commits carried, in the order the default branch took them:
    /// `(sha, subject)`.
    pub carried: Vec<(String, String)>,
    /// The release notes, written outside the working tree.
    pub notes: PathBuf,
    /// The tests that passed.
    pub tests: Vec<String>,
    /// The commands of the go, in order.
    pub go: Vec<String>,
}

struct Git<'a> {
    dir: &'a Path,
}

impl Git<'_> {
    fn run(&self, args: &[&str]) -> Result<String, String> {
        let out = Command::new("git")
            .arg("-C")
            .arg(self.dir)
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
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    fn ok(&self, args: &[&str]) -> bool {
        Command::new("git")
            .arg("-C")
            .arg(self.dir)
            .args(args)
            .output()
            .is_ok_and(|o| o.status.success())
    }
}

fn now_stamp() -> String {
    let t = time::OffsetDateTime::now_utc();
    format!(
        "{:04}-{:02}-{:02}-{:02}-{:02}",
        t.year(),
        u8::from(t.month()),
        t.day(),
        t.hour(),
        t.minute()
    )
}

/// The highest release tag of the repository, by version: the release in
/// production when `--from` names none.
fn production(git: &Git, template: &str) -> Result<Option<(String, Version)>, String> {
    let re = template_regex(template);
    Ok(git
        .run(&["tag", "--list"])?
        .lines()
        .filter_map(|t| {
            let c = re.captures(t.trim())?;
            Some((t.trim().to_string(), Version::parse(&c["version"])?))
        })
        .max_by_key(|(_, v)| *v))
}

/// Prepare the break-glass patch. A refusal before the branch is opened
/// leaves the checkout as it was; a refusal after it (a cherry-pick that
/// does not apply, a failed preparation) removes the branch and returns to
/// the starting point. Failed tests leave the branch for inspection.
/// Nothing is ever pushed nor tagged.
pub fn run(prep: &Preparation, o: &Options) -> Result<Ready, String> {
    let dir = if o.dir.as_os_str().is_empty() {
        Path::new(".")
    } else {
        o.dir.as_path()
    };
    let git = Git { dir };
    if o.commits.is_empty() {
        return Err(
            "a break-glass patch carries at least one commit merged into the default branch: --commit <sha>".into(),
        );
    }
    if o.tests.is_empty() {
        return Err(
            "a break-glass patch runs the project's tests before the go: --test '<command>'".into(),
        );
    }
    let repository = o
        .repository
        .clone()
        .or_else(|| prep.repository.clone())
        .ok_or("the links of the notes need the repository: pass --repository <owner>/<repo>")?;
    if !git.run(&["status", "--porcelain"])?.is_empty() {
        return Err("the working tree has changes: commit or stash them first".into());
    }
    let remote = if o.remote.is_empty() {
        "origin"
    } else {
        o.remote.as_str()
    };
    if !o.no_fetch {
        git.run(&["fetch", "--quiet", "--tags", remote])?;
    }
    let default = match &o.default_branch {
        Some(b) => b.clone(),
        None => git
            .run(&[
                "symbolic-ref",
                "--short",
                &format!("refs/remotes/{remote}/HEAD"),
            ])
            .ok()
            .and_then(|r| r.strip_prefix(&format!("{remote}/")).map(str::to_string))
            .unwrap_or_else(|| "main".to_string()),
    };
    let default_ref = format!("refs/remotes/{remote}/{default}");
    if !git.ok(&["rev-parse", "--verify", "--quiet", &default_ref]) {
        return Err(format!(
            "{remote}/{default} is unknown: name the default branch with --default-branch"
        ));
    }

    // The release in production.
    let re = template_regex(&prep.release_tag);
    let (from, production_version) = match &o.from {
        Some(tag) => {
            let v = re
                .captures(tag)
                .and_then(|c| Version::parse(&c["version"]))
                .ok_or_else(|| {
                    format!("--from {tag} is not a release tag ({})", prep.release_tag)
                })?;
            if !git.ok(&[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("refs/tags/{tag}"),
            ]) {
                return Err(format!("--from {tag}: no such tag"));
            }
            (tag.clone(), v)
        }
        None => production(&git, &prep.release_tag)?.ok_or(
            "no release tag: a patch starts from the release in production, so cut a normal cycle",
        )?,
    };
    let version = crate::verbs::patch_version(production_version);

    // Every commit merged into the default branch, none already in
    // production: nothing unpublished leaves by the side door.
    let mut carried: Vec<String> = Vec::new();
    let mut refused = Vec::new();
    for c in &o.commits {
        let Ok(sha) = git.run(&[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{c}^{{commit}}"),
        ]) else {
            refused.push(format!("{c} is no commit of this repository"));
            continue;
        };
        if carried.contains(&sha) {
            continue;
        }
        if !git.ok(&["merge-base", "--is-ancestor", &sha, &default_ref]) {
            refused.push(format!(
                "{c} is not on {remote}/{default}: a break-glass patch only carries commits already merged into the default branch"
            ));
            continue;
        }
        if git.ok(&[
            "merge-base",
            "--is-ancestor",
            &sha,
            &format!("refs/tags/{from}"),
        ]) {
            refused.push(format!("{c} is already in production ({from})"));
            continue;
        }
        carried.push(sha);
    }
    if !refused.is_empty() {
        return Err(format!("no patch: {}", refused.join("; ")));
    }
    // In the order the default branch took them.
    let order = git.run(&[
        "rev-list",
        "--reverse",
        "--topo-order",
        &format!("refs/tags/{from}..{default_ref}"),
    ])?;
    let position = |sha: &str| order.lines().position(|l| l == sha).unwrap_or(usize::MAX);
    carried.sort_by_key(|s| position(s));

    let stamp = o.stamp.clone().unwrap_or_else(now_stamp);
    let branch = format!("{}{stamp}-utc", prep.branch_prefix);
    if git.ok(&[
        "rev-parse",
        "--verify",
        "--quiet",
        &format!("refs/heads/{branch}"),
    ]) || git.ok(&[
        "rev-parse",
        "--verify",
        "--quiet",
        &format!("refs/remotes/{remote}/{branch}"),
    ]) {
        return Err(format!(
            "{branch} already exists: run again in a minute, or pass another --stamp"
        ));
    }
    let start = git
        .run(&["symbolic-ref", "--quiet", "--short", "HEAD"])
        .or_else(|_| git.run(&["rev-parse", "HEAD"]))?;
    git.run(&[
        "checkout",
        "--quiet",
        "-b",
        &branch,
        &format!("refs/tags/{from}^{{commit}}"),
    ])?;
    let undo = |why: String| -> String {
        let _ = git.run(&["cherry-pick", "--abort"]);
        let _ = git.run(&["reset", "--quiet", "--hard"]);
        let _ = git.run(&["checkout", "--quiet", &start]);
        let _ = git.run(&["branch", "--quiet", "-D", &branch]);
        format!("{why}; {branch} removed, nothing pushed")
    };

    let mut subjects = Vec::new();
    for sha in &carried {
        let parents = git
            .run(&["rev-list", "--parents", "-n", "1", sha])
            .map_err(undo)?;
        let mut args = vec!["cherry-pick", "-x"];
        if parents.split_whitespace().count() > 2 {
            args.extend(["-m", "1"]);
        }
        args.push(sha);
        if let Err(e) = git.run(&args) {
            return Err(undo(format!(
                "{sha} does not apply on {from}: a patch is made of commits that apply cleanly on the release in production ({e})"
            )));
        }
        let subject = git.run(&["log", "-1", "--format=%s", sha]).map_err(undo)?;
        subjects.push((sha.clone(), subject));
    }

    let git_dir = git
        .run(&["rev-parse", "--absolute-git-dir"])
        .map_err(undo)?;
    let notes = PathBuf::from(git_dir).join(format!("spl-asdlc-break-glass-{version}.md"));
    let prepared = prepare::run(
        prep,
        &prepare::Options {
            dir: dir.to_path_buf(),
            version: Some(version.to_string()),
            notes: Some(notes.clone()),
            date: o.date.clone(),
            repository: Some(repository.clone()),
            dry_run: false,
        },
    )
    .map_err(|e| undo(format!("the patch could not be prepared: {e}")))?;
    git.run(&["add", "-A"]).map_err(undo)?;
    git.run(&[
        "commit",
        "--quiet",
        "-m",
        &format!("chore(release): v{version}"),
    ])
    .map_err(undo)?;
    let head = git.run(&["rev-parse", "HEAD"]).map_err(undo)?;

    for t in &o.tests {
        let status = Command::new("sh")
            .arg("-c")
            .arg(t)
            .current_dir(dir)
            .status()
            .map_err(|e| format!("`{t}` could not run: {e}"))?;
        if !status.success() {
            return Err(format!(
                "the tests failed (`{t}`): {branch} is left at {head} for inspection; nothing pushed nor tagged. Fix forward on the default branch, then run the break-glass again"
            ));
        }
    }

    let tag = prepared.tag;
    let go = vec![
        format!("git push {remote} {branch}"),
        format!("git tag -a {tag} {head} -m 'Release {tag}' -m 'Release-Patch-From: {from}'"),
        format!("git push {remote} {tag}"),
        format!(
            "gh release create {tag} --repo {repository} --title '{} {version}' --notes-file {}",
            prep.product,
            notes.display()
        ),
    ];
    Ok(Ready {
        from,
        version,
        tag,
        branch,
        head,
        carried: subjects,
        notes,
        tests: o.tests.clone(),
        go,
    })
}

#[cfg(test)]
mod tests;
