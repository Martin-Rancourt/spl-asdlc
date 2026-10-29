//! Read-only merge preview, needed because GitHub exposes mergeability but
//! no API listing the conflicting paths. The temporary bare repository is
//! deleted on every exit; credentials live only in the subprocess environment.
use super::{PullRequest, RepoSlug};
use base64::Engine;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn conflicts(repo: &RepoSlug, pr: &PullRequest, token: &str) -> Result<Vec<String>, String> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "spl-asdlc-merge-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&dir).map_err(|_| "cannot create merge preview directory")?;
    let scratch = Scratch(dir);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&scratch.0, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| "cannot secure merge preview directory")?;
    }
    let auth = base64::engine::general_purpose::STANDARD.encode(format!("x-access-token:{token}"));
    let git = |args: &[&str]| {
        Command::new("git")
            .args(["-c", "http.lowSpeedLimit=1", "-c", "http.lowSpeedTime=30"])
            .args(args)
            .current_dir(&scratch.0)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env(
                "GIT_CONFIG_GLOBAL",
                if cfg!(windows) { "NUL" } else { "/dev/null" },
            )
            .env("GIT_CONFIG_COUNT", "1")
            .env("GIT_CONFIG_KEY_0", "http.https://github.com/.extraheader")
            .env("GIT_CONFIG_VALUE_0", format!("AUTHORIZATION: basic {auth}"))
            .output()
            .map_err(|_| "cannot run git merge preview".to_string())
    };
    if !git(&["init", "--bare", "--quiet"])?.status.success() {
        return Err("cannot initialise merge preview".into());
    }
    let url = format!("https://github.com/{repo}.git");
    // Fetch immutable heads, never execute a checkout, hook or project code.
    let base = format!("refs/heads/{}", pr.base_ref);
    if !git(&[
        "fetch",
        "--quiet",
        "--no-tags",
        "--filter=blob:none",
        &url,
        &base,
        &pr.head_sha,
    ])?
    .status
    .success()
    {
        return Err("cannot fetch merge preview refs".into());
    }
    // FETCH_HEAD contains two rows; resolve the base explicitly from the
    // first fetched object rather than trust a moving branch name.
    let fetch = std::fs::read_to_string(scratch.0.join("FETCH_HEAD"))
        .map_err(|_| "cannot read preview refs")?;
    let base_sha = fetch
        .split_whitespace()
        .next()
        .ok_or("empty preview refs")?;
    let out = git(&[
        "merge-tree",
        "--write-tree",
        "--name-only",
        "-z",
        base_sha,
        &pr.head_sha,
    ])?;
    match out.status.code() {
        Some(0) => Ok(Vec::new()),
        Some(1) => paths(&out.stdout),
        _ => Err("git could not compute merge preview".into()),
    }
}

fn paths(bytes: &[u8]) -> Result<Vec<String>, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "merge preview returned non-UTF8 paths")?;
    let mut parts = text.split('\0');
    parts.next().ok_or("merge preview returned no tree")?;
    let mut paths: Vec<String> = parts
        .take_while(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();
    paths.sort();
    paths.dedup();
    if paths.is_empty() {
        return Err("merge preview reported conflicts without paths".into());
    }
    Ok(paths)
}

#[cfg(test)]
mod tests {
    #[test]
    fn paths_stop_before_conflict_messages_and_preserve_spaces() {
        assert_eq!(
            super::paths(b"tree\0a file.md\0src/b.rs\0\x00001\0message\0").unwrap(),
            vec!["a file.md", "src/b.rs"]
        );
        assert!(super::paths(b"tree\0\0").is_err());
    }
}
