//! One tick at a time per repository (`docs/spec/07-the-bridge.md`
//! invariant 8): a tick holds an exclusive lock on a file beside its action
//! log for as long as it runs, so that two overlapping ticks (a timer and a
//! run by hand) never decide on the same facts at once. A second tick finds
//! the lock taken, does nothing and says so.
//!
//! The lock is the operating system's `flock`: it goes with the process, so
//! a tick that crashes leaves no stale lock behind. The file itself is empty
//! and stays in place: removing it on release would let a third tick lock a
//! new file while a second still holds the old one. A system without `flock`
//! (Windows) takes no lock: there, one tick at a time is the host's to keep.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

/// The lock of a running tick, released when dropped.
#[derive(Debug)]
pub struct TickLock {
    _file: File,
    path: PathBuf,
}

impl TickLock {
    /// The lock file of the action log at `log`: `<log>.lock`.
    pub fn beside(log: &Path) -> PathBuf {
        let mut name = log.as_os_str().to_owned();
        name.push(".lock");
        PathBuf::from(name)
    }

    /// Take the lock at `path` without waiting: `None` when another tick
    /// holds it. The file and its directory are created when missing.
    pub fn try_acquire(path: &Path) -> io::Result<Option<TickLock>> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            crate::action_log::private_dir(dir)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path)?;
        if !try_lock(&file)? {
            return Ok(None);
        }
        Ok(Some(TickLock {
            _file: file,
            path: path.to_path_buf(),
        }))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// An exclusive, non-blocking `flock` on `file`, held until the file is
/// closed: `false` when another open file holds it.
#[cfg(unix)]
fn try_lock(file: &File) -> io::Result<bool> {
    use std::os::unix::io::AsRawFd;
    // SAFETY: `flock` reads only the descriptor, which `file` keeps open
    // for the duration of the call.
    let r = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if r == 0 {
        return Ok(true);
    }
    let e = io::Error::last_os_error();
    if e.raw_os_error() == Some(libc::EWOULDBLOCK) {
        Ok(false)
    } else {
        Err(e)
    }
}

#[cfg(not(unix))]
fn try_lock(_file: &File) -> io::Result<bool> {
    Ok(true)
}

/// What a tick that found the lock taken says.
pub fn already_running(repo: &str, path: &Path) -> String {
    format!(
        "{repo}: a tick is already running (lock {}): this one does nothing",
        path.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("spl-asdlc-lock-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn the_lock_file_sits_beside_the_action_log() {
        assert_eq!(
            TickLock::beside(Path::new("state/spl-asdlc/acme.jsonl")),
            PathBuf::from("state/spl-asdlc/acme.jsonl.lock")
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_second_tick_finds_the_lock_taken_until_the_first_ends() {
        let path = scratch("held.jsonl.lock");
        let first = TickLock::try_acquire(&path).unwrap().expect("the lock");
        assert!(TickLock::try_acquire(&path).unwrap().is_none());
        drop(first);
        assert!(TickLock::try_acquire(&path).unwrap().is_some());
    }

    #[test]
    fn the_lock_creates_its_directory() {
        let path = scratch("nested").join("deeper").join("acme.jsonl.lock");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
        let lock = TickLock::try_acquire(&path).unwrap().expect("the lock");
        assert_eq!(lock.path(), path);
        assert!(path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn lock_then_log_flush_keeps_the_log_directory_private() {
        use crate::action_log::{ActionLog, Outcome};
        use crate::github::{RepoSlug, Timestamp};
        use serde_json::json;
        use std::os::unix::fs::PermissionsExt;

        let log = scratch("private/nested/acme.jsonl");
        let _ = std::fs::remove_dir_all(log.parent().unwrap());
        let _lock = TickLock::try_acquire(&TickLock::beside(&log))
            .unwrap()
            .expect("the lock");
        let mut action_log = ActionLog::at(log.clone());
        action_log.record(
            Timestamp::parse("2026-10-11T00:00:00Z").unwrap(),
            &RepoSlug::parse("acme/widget").unwrap(),
            "pr#1",
            "merge",
            json!({}),
            Outcome::Done,
        );
        action_log.flush().unwrap();
        assert_eq!(
            std::fs::metadata(log.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
}
