//! Stable binary updates, constrained by the host's explicitly chosen major.
use std::path::{Path, PathBuf};
use std::process::Command;

use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};

const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerConfig {
    pub major: u64,
    pub files: Vec<PathBuf>,
    #[serde(default = "interval")]
    pub interval_seconds: u64,
}
fn interval() -> u64 {
    300
}
impl RunnerConfig {
    pub fn read(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let config: Self = toml::from_str(&text).map_err(|e| e.to_string())?;
        if config.files.is_empty() || config.interval_seconds == 0 {
            return Err("runner needs files and a positive interval_seconds".into());
        }
        Ok(config)
    }
}

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}
fn candidate(releases: &[Release], current: &Version, major: u64) -> Option<Version> {
    releases
        .iter()
        .filter(|r| !r.draft && !r.prerelease)
        .filter_map(|r| Version::parse(r.tag_name.strip_prefix('v')?).ok())
        .filter(|v| v.major == major && v.pre.is_empty() && v.build.is_empty() && v > current)
        .max()
}

fn download(url: &str) -> Result<Vec<u8>, String> {
    // Public release endpoints only. Never send the bridge's API credentials.
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(60)))
        .user_agent(concat!("spl-asdlc/", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    agent
        .get(url)
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .with_config()
        .limit(64 * 1024 * 1024)
        .read_to_vec()
        .map_err(|e| e.to_string())
}
fn latest(current: &Version, major: u64) -> Result<Option<Version>, String> {
    let slug = REPOSITORY
        .strip_prefix("https://github.com/")
        .ok_or("not a GitHub repository")?;
    let mut best = None;
    // Do not use /latest: an older major can still receive patches. Exhaust the
    // release list; refuse a truncated history rather than guessing its winner.
    for page in 1..=100 {
        let bytes = download(&format!(
            "https://api.github.com/repos/{slug}/releases?per_page=100&page={page}"
        ))?;
        let releases: Vec<Release> = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        if let Some(v) = candidate(&releases, current, major) {
            best = Some(best.map_or(v.clone(), |old: Version| old.max(v)));
        }
        if releases.len() < 100 {
            return Ok(best);
        }
    }
    Err("release history exceeds the update limit".into())
}
fn target() -> Result<&'static str, String> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-gnu"),
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin"),
        ("windows", "x86_64") => Ok("x86_64-pc-windows-msvc"),
        _ => Err("no published binary for this target".into()),
    }
}
fn verify(bytes: &[u8], sums: &[u8], name: &str) -> Result<(), String> {
    let sums = std::str::from_utf8(sums).map_err(|e| e.to_string())?;
    let expected: Vec<_> = sums
        .lines()
        .filter_map(|line| {
            let (hash, file) = line.split_once(char::is_whitespace)?;
            (file.trim().trim_start_matches('*') == name).then_some(hash)
        })
        .collect();
    if expected.len() != 1 || expected[0] != format!("{:x}", Sha256::digest(bytes)) {
        return Err("release archive checksum missing, ambiguous or invalid".into());
    }
    Ok(())
}
fn installed(path: &Path) -> Result<Version, String> {
    let out = Command::new(path)
        .arg("--version")
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err("binary version check failed".into());
    }
    let text = String::from_utf8(out.stdout).map_err(|e| e.to_string())?;
    Version::parse(
        text.trim()
            .strip_prefix("spl-asdlc ")
            .ok_or("unexpected binary version output")?,
    )
    .map_err(|e| e.to_string())
}
struct Stage(PathBuf);
impl Drop for Stage {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Update the executable at `path`. A changed major is accepted only because
/// the caller explicitly selected it; even then an older version is refused.
/// The caller must run the returned executable on its next pass.
pub fn update(path: &Path, major: u64) -> Result<Option<Version>, String> {
    let path = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
    let lock_path = crate::lock::TickLock::beside(&path);
    let _lock = crate::lock::TickLock::try_acquire(&lock_path)
        .map_err(|e| e.to_string())?
        .ok_or("another updater is running")?;
    let current = installed(&path)?;
    let Some(next) = latest(&current, major)? else {
        return Ok(None);
    };
    let base = format!("spl-asdlc-v{next}-{}", target()?);
    let suffix = if cfg!(windows) { "zip" } else { "tar.gz" };
    let name = format!("{base}.{suffix}");
    let url = format!("{REPOSITORY}/releases/download/v{next}");
    let archive = download(&format!("{url}/{name}"))?;
    let sums = download(&format!("{url}/SHA256SUMS"))?;
    install(&path, &current, &next, &base, &name, &archive, &sums)?;
    Ok(Some(next))
}

fn install(
    path: &Path,
    current: &Version,
    next: &Version,
    base: &str,
    name: &str,
    archive: &[u8],
    sums: &[u8],
) -> Result<(), String> {
    verify(archive, sums, name)?;
    let dir = path.with_file_name(format!(".spl-asdlc-update-{}", std::process::id()));
    std::fs::create_dir(&dir).map_err(|e| e.to_string())?;
    let stage = Stage(dir);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&stage.0, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
    }
    let archive_path = stage.0.join(name);
    std::fs::write(&archive_path, archive).map_err(|e| e.to_string())?;
    let exe_name = if cfg!(windows) {
        "spl-asdlc.exe"
    } else {
        "spl-asdlc"
    };
    // Extract exactly one regular output stream, never archive paths/links.
    // macOS and Windows ship bsdtar; Linux supplies tar.
    let output = Command::new("tar")
        .arg("-xOf")
        .arg(&archive_path)
        .arg(format!("{base}/{exe_name}"))
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() || output.stdout.is_empty() {
        return Err("cannot extract release binary".into());
    }
    let staged = stage.0.join(exe_name);
    std::fs::write(&staged, output.stdout).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| e.to_string())?;
    }
    if installed(&staged)? != *next {
        return Err("downloaded binary does not match release version".into());
    }
    let backup = path.with_extension(format!("previous-{current}"));
    #[cfg(not(windows))]
    {
        std::fs::copy(path, &backup).map_err(|e| e.to_string())?;
        std::fs::rename(&staged, path).map_err(|e| e.to_string())?;
    }
    #[cfg(windows)]
    {
        // Windows cannot replace a mapped executable. Move it aside first,
        // retaining its versioned name while an old supervisor still runs it.
        std::fs::rename(path, &backup).map_err(|e| e.to_string())?;
        if let Err(error) = std::fs::rename(&staged, path) {
            std::fs::rename(&backup, path).map_err(|rollback| {
                format!("install: {error}; restore {}: {rollback}", backup.display())
            })?;
            return Err(error.to_string());
        }
    }
    Ok(())
}

/// Supervise short-lived ticks, checking updates before each pass. The
/// supervisor itself remains tiny; all bridge work runs in the installed image.
pub fn runner(file: &Path) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    loop {
        let config = RunnerConfig::read(file)?;
        let result = Command::new(&exe)
            .arg("update")
            .arg("--major")
            .arg(config.major.to_string())
            .status()
            .map_err(|e| e.to_string())?;
        if !result.success() {
            eprintln!("update failed; keeping installed binary");
        }
        let mut tick = Command::new(&exe);
        tick.arg("tick");
        for file in &config.files {
            tick.arg("--file").arg(file);
        }
        let status = tick.status().map_err(|e| e.to_string())?;
        if !status.success() {
            eprintln!("tick exited with {status}");
        }
        std::thread::sleep(std::time::Duration::from_secs(config.interval_seconds));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn releases() -> Vec<Release> {
        serde_json::from_str(
            r#"[
            {"tag_name":"v2.0.0"}, {"tag_name":"v1.19.0"},
            {"tag_name":"v1.18.5"}, {"tag_name":"v1.20.0-rc.1"},
            {"tag_name":"v1.21.0","prerelease":true},
            {"tag_name":"v1.22.0","draft":true}, {"tag_name":"staging"}
        ]"#,
        )
        .unwrap()
    }
    #[test]
    fn updates_patches_and_minors_without_crossing_the_requested_major() {
        assert_eq!(
            candidate(&releases(), &Version::new(1, 18, 0), 1),
            Some(Version::new(1, 19, 0))
        );
        assert_eq!(
            candidate(&releases()[2..3], &Version::new(1, 18, 0), 1),
            Some(Version::new(1, 18, 5))
        );
        assert_eq!(
            candidate(&releases(), &Version::new(1, 18, 0), 2),
            Some(Version::new(2, 0, 0))
        );
    }
    #[test]
    fn never_downgrades_even_when_config_requests_an_older_major() {
        assert_eq!(candidate(&releases(), &Version::new(2, 0, 0), 1), None);
        assert_eq!(candidate(&releases(), &Version::new(1, 19, 0), 1), None);
        assert_eq!(candidate(&releases(), &Version::new(1, 23, 0), 1), None);
    }
    #[test]
    fn checksum_must_name_the_exact_archive_once() {
        let hash = format!("{:x}", Sha256::digest(b"archive"));
        assert!(
            verify(
                b"archive",
                format!("{hash}  a.tar.gz\n").as_bytes(),
                "a.tar.gz"
            )
            .is_ok()
        );
        assert!(
            verify(
                b"changed",
                format!("{hash}  a.tar.gz\n").as_bytes(),
                "a.tar.gz"
            )
            .is_err()
        );
        assert!(
            verify(
                b"archive",
                format!("{hash}  other.tar.gz\n").as_bytes(),
                "a.tar.gz"
            )
            .is_err()
        );
        assert!(
            verify(
                b"archive",
                format!("{hash}  a.tar.gz\n{hash}  a.tar.gz\n").as_bytes(),
                "a.tar.gz"
            )
            .is_err()
        );
    }
    #[test]
    fn runner_config_requires_an_explicit_major() {
        assert!(toml::from_str::<RunnerConfig>("files = ['config.toml']").is_err());
        let c: RunnerConfig = toml::from_str("major = 2\nfiles = ['config.toml']").unwrap();
        assert_eq!(c.major, 2);
        assert_eq!(c.interval_seconds, 300);
    }

    #[test]
    #[cfg(unix)]
    fn installation_checks_version_and_preserves_a_rollback_binary() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("spl-update-test-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let cleanup = Stage(dir);
        let path = cleanup.0.join("spl-asdlc");
        let old = b"#!/bin/sh\necho spl-asdlc 1.0.0\n";
        std::fs::write(&path, old).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        let base = "spl-asdlc-v1.1.0-test";
        let input = cleanup.0.join(base);
        std::fs::create_dir(&input).unwrap();
        let binary = input.join("spl-asdlc");
        for (reported, succeeds) in [("9.0.0", false), ("1.1.0", true)] {
            std::fs::write(&binary, format!("#!/bin/sh\necho spl-asdlc {reported}\n")).unwrap();
            let archive = Command::new("tar")
                .arg("-czf")
                .arg("-")
                .arg("-C")
                .arg(&cleanup.0)
                .arg(base)
                .output()
                .unwrap();
            assert!(archive.status.success());
            let sums = format!("{:x}  a.tar.gz\n", Sha256::digest(&archive.stdout));
            let result = install(
                &path,
                &Version::new(1, 0, 0),
                &Version::new(1, 1, 0),
                base,
                "a.tar.gz",
                &archive.stdout,
                sums.as_bytes(),
            );
            assert_eq!(result.is_ok(), succeeds, "{result:?}");
            if !succeeds {
                assert_eq!(std::fs::read(&path).unwrap(), old);
            }
        }
        assert_eq!(installed(&path).unwrap(), Version::new(1, 1, 0));
        assert_eq!(
            std::fs::read(path.with_extension("previous-1.0.0")).unwrap(),
            old
        );
    }
}
