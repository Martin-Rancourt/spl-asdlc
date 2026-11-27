//! `spl-asdlc rules check` in a checkout (ASDLC-YX9K): the registry and the
//! files its rules cite are read from a commit through `git`, never from
//! the working tree nor the Git host, so a PR's base and head are read the
//! same way. The judgement is `crate::method::business_rules::check`'s.

use std::path::Path;
use std::process::Command;

use crate::method::business_rules::REGISTRY_DIR;
use crate::method::business_rules::check::{Report, check};

fn git(dir: &Path, args: &[&str]) -> Result<std::process::Output, String> {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map_err(|e| format!("git could not run: {e}"))
}

/// The commit `rev` names in the checkout `dir`, resolved once.
pub fn resolve(dir: &Path, rev: &str) -> Result<String, String> {
    let spec = format!("{rev}^{{commit}}");
    let out = git(dir, &["rev-parse", "--verify", "--quiet", &spec])?;
    if !out.status.success() {
        return Err(format!(
            "`{rev}` names no commit in {}: fetch it first (a shallow clone lacks the base)",
            dir.display()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// A file's text at `commit`: `None` when the commit has no such path.
fn read(dir: &Path, commit: &str, path: &str) -> Result<Option<String>, String> {
    let object = format!("{commit}:{path}");
    let kind = git(dir, &["cat-file", "-t", &object])?;
    if !kind.status.success() {
        return Ok(None);
    }
    if String::from_utf8_lossy(&kind.stdout).trim() != "blob" {
        // A directory is present, and holds no test name.
        return Ok(Some(String::new()));
    }
    let out = git(dir, &["cat-file", "blob", &object])?;
    if !out.status.success() {
        return Err(format!(
            "{path} could not be read at {commit}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(Some(String::from_utf8_lossy(&out.stdout).into_owned()))
}

/// The registry's files at `commit`, `(path, text)`, one level deep; empty
/// when the directory is absent (the first rule creates it).
fn registry(dir: &Path, commit: &str) -> Result<Vec<(String, String)>, String> {
    let out = git(
        dir,
        &["ls-tree", "-z", commit, "--", &format!("{REGISTRY_DIR}/")],
    )?;
    if !out.status.success() {
        return Err(format!(
            "{REGISTRY_DIR}/ could not be listed at {commit}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let mut files = Vec::new();
    for entry in String::from_utf8_lossy(&out.stdout).split('\0') {
        // `<mode> <type> <object>\t<path>`: the files only.
        let Some((meta, path)) = entry.split_once('\t') else {
            continue;
        };
        if meta.split(' ').nth(1) != Some("blob") {
            continue;
        }
        let text = read(dir, commit, path)?
            .ok_or_else(|| format!("{path} is listed but absent at {commit}"))?;
        files.push((path.to_string(), text));
    }
    Ok(files)
}

/// The registry checked at `commit` in the checkout `dir`; `at` names the
/// commit in the errors.
pub fn check_at(dir: &Path, commit: &str, at: &str) -> Result<Report, String> {
    let files = registry(dir, commit)?;
    check(&files, at, |path: &str| read(dir, commit, path))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git_ok(dir: &Path, args: &[&str]) {
        let out = git(dir, args).unwrap();
        assert!(out.status.success(), "git {args:?}: {out:?}");
    }

    fn commit(dir: &Path, files: &[(&str, &str)], message: &str) -> String {
        for (path, text) in files {
            let p = dir.join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        git_ok(dir, &["add", "-A"]);
        git_ok(
            dir,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@example.invalid",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                message,
            ],
        );
        resolve(dir, "HEAD").unwrap()
    }

    const RULE: &str = "# K7QF — A green PR merges first\n\nStatus: Enforced\n\n## Rule\n\nGreen first.\n\n## Evidence\n\nASDLC-1.\n\n## Test\n\n- `tests/merge.rs`: `green_prs_merge_first`\n";

    #[test]
    fn the_registry_is_read_from_each_commit_never_from_the_working_tree() {
        let scratch =
            std::env::temp_dir().join(format!("spl-asdlc-rules-check-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&scratch).unwrap();
        let dir = scratch.as_path();
        git_ok(dir, &["init", "-q"]);
        git_ok(dir, &["config", "commit.gpgsign", "false"]);
        let empty = commit(dir, &[("README.md", "x")], "root");
        assert_eq!(check_at(dir, &empty, "HEAD").unwrap(), Report::default());
        let head = commit(
            dir,
            &[(".spl-asdlc/business-rules/K7QF-green-first.md", RULE)],
            "a rule",
        );
        let r = check_at(dir, &head, "HEAD").unwrap();
        assert_eq!(r.errors, vec!["K7QF: `tests/merge.rs` is absent at HEAD"]);
        // An uncommitted test file changes nothing: the commit is read.
        std::fs::create_dir_all(dir.join("tests")).unwrap();
        std::fs::write(dir.join("tests/merge.rs"), "fn green_prs_merge_first() {}").unwrap();
        assert_eq!(check_at(dir, &head, "HEAD").unwrap().errors.len(), 1);
        let fixed = commit(dir, &[], "the test");
        assert!(check_at(dir, &fixed, "HEAD").unwrap().errors.is_empty());
        assert!(resolve(dir, "no-such-ref").is_err());
        let _ = std::fs::remove_dir_all(&scratch);
    }
}
