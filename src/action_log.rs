//! The action log (`docs/spec/07-the-bridge.md` invariants 4 and 7): one
//! JSON line per decision that acted or tried to act (when, repository,
//! object, action, inputs, outcome), appended, never rewritten.
//!
//! It is an audit cache, not the source of truth: the bridge decides from
//! what GitHub and the tracker show, and from its markers for the effects
//! nobody can observe (a message posted once, a notification sent). Losing
//! the file costs a re-read, never a duplicate.
//!
//! A tick that does nothing writes nothing here either: a decision that
//! found its effect already in place is not an entry, and the file is not
//! even created until there is something to say. A PR the merge pass holds
//! is noted ([`Outcome::Waiting`]) only when the reason it waits, or its
//! head, changes: the tick's report says it every time, the log once. An
//! item or PR under the hands-off label is noted the same way, once when the
//! tick first leaves it alone; the next entry about it that is not that note
//! (the bridge acting on it again) ends it.

use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::github::{RepoSlug, Timestamp};

/// What became of an action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", content = "detail", rename_all = "kebab-case")]
pub enum Outcome {
    Done,
    /// Refused or failed; retried at the next tick.
    Failed(String),
    /// A dry run: the action the tick would take.
    WouldDo,
    /// Nothing was done: a PR the merge pass holds, and why (the inputs),
    /// or an object under the hands-off label ([`HANDS_OFF`]).
    Waiting,
}

/// The action of an [`Outcome::Waiting`] entry.
pub const WAITING: &str = "waiting";

/// The action of the [`Outcome::Waiting`] entry that notes an object under
/// the hands-off label.
pub const HANDS_OFF: &str = "hands-off";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// RFC 3339, UTC.
    pub at: String,
    pub repo: String,
    /// What the action is about: `pr#12`, `ABC-34`, `release acme 1.2.3`.
    pub object: String,
    /// What was done: `merge`, `status To Deploy`, `comment`, `notify`.
    pub action: String,
    /// The facts the decision rested on (head SHA, check state, …).
    pub inputs: Value,
    #[serde(flatten)]
    pub outcome: Outcome,
}

/// The log of one tick, appended to its file when the tick ends.
#[derive(Debug, Default)]
pub struct ActionLog {
    path: Option<PathBuf>,
    entries: Vec<Entry>,
    flushed: usize,
    /// The last waiting note of each (repository, object), read back from
    /// the file the first time one is noted.
    waits: Option<BTreeMap<(String, String), Value>>,
    /// The (repository, object) pairs whose last entry is a hands-off note,
    /// read back from the file the first time one is noted.
    held: Option<std::collections::BTreeSet<(String, String)>>,
}

impl ActionLog {
    /// A log kept in memory only (dry runs, tests).
    pub fn in_memory() -> ActionLog {
        ActionLog::default()
    }

    /// A log appended to `path`.
    pub fn at(path: PathBuf) -> ActionLog {
        ActionLog {
            path: Some(path),
            ..ActionLog::default()
        }
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn record(
        &mut self,
        at: Timestamp,
        repo: &RepoSlug,
        object: &str,
        action: &str,
        inputs: Value,
        outcome: Outcome,
    ) {
        if let Some(held) = &mut self.held {
            held.remove(&(repo.to_string(), object.to_string()));
        }
        self.entries.push(Entry {
            at: at.to_string(),
            repo: repo.to_string(),
            object: object.to_string(),
            action: action.to_string(),
            inputs,
            outcome,
        });
    }

    /// Note why a PR waits, unless the log's last note for it says the
    /// same of the same head. Returns whether it was noted. A log whose file
    /// cannot be read back notes it again: the log is an audit cache.
    pub fn note_waiting(
        &mut self,
        at: Timestamp,
        repo: &RepoSlug,
        object: &str,
        head: &str,
        reason: &str,
    ) -> bool {
        let inputs = json!({"head": head, "reason": reason});
        let key = (repo.to_string(), object.to_string());
        let path = self.path.as_deref();
        let waits = self.waits.get_or_insert_with(|| last_waits(path));
        if waits.get(&key) == Some(&inputs) {
            return false;
        }
        waits.insert(key, inputs.clone());
        self.record(at, repo, object, WAITING, inputs, Outcome::Waiting);
        true
    }

    /// Note that the tick left an object under the hands-off label alone,
    /// unless its last entry already says so. Returns whether it was noted.
    pub fn note_hands_off(
        &mut self,
        at: Timestamp,
        repo: &RepoSlug,
        object: &str,
        label: &str,
    ) -> bool {
        let key = (repo.to_string(), object.to_string());
        let path = self.path.as_deref();
        let held = self.held.get_or_insert_with(|| last_held(path));
        if held.contains(&key) {
            return false;
        }
        self.record(
            at,
            repo,
            object,
            HANDS_OFF,
            json!({"label": label}),
            Outcome::Waiting,
        );
        if let Some(held) = &mut self.held {
            held.insert(key);
        }
        true
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// The failed actions of this tick.
    pub fn failures(&self) -> Vec<&Entry> {
        self.entries
            .iter()
            .filter(|e| matches!(e.outcome, Outcome::Failed(_)))
            .collect()
    }

    /// Append the entries not yet written. Nothing to write touches nothing.
    pub fn flush(&mut self) -> io::Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        if self.flushed == self.entries.len() {
            return Ok(());
        }
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            private_dir(dir)?;
        }
        let mut file = open_private(path)?;
        let mut text = String::new();
        for e in &self.entries[self.flushed..] {
            text.push_str(&serde_json::to_string(e).map_err(io::Error::other)?);
            text.push('\n');
        }
        file.write_all(text.as_bytes())?;
        file.flush()?;
        self.flushed = self.entries.len();
        Ok(())
    }

    /// Read a log file back: its entries and the number of lines that are
    /// not entries (a line cut by a crash, a hand edit).
    pub fn read(path: &Path) -> io::Result<(Vec<Entry>, usize)> {
        let file = match std::fs::File::open(path) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok((Vec::new(), 0)),
            Err(e) => return Err(e),
        };
        let mut entries = Vec::new();
        let mut bad = 0;
        for line in io::BufReader::new(file).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str(&line) {
                Ok(e) => entries.push(e),
                Err(_) => bad += 1,
            }
        }
        Ok((entries, bad))
    }

    /// The default place of a repository's log: the user's state directory
    /// (`$XDG_STATE_HOME`, `~/.local/state`, or `%LOCALAPPDATA%`), under
    /// `spl-asdlc/<owner>-<name>.jsonl`. `None` when no such directory is
    /// known: the caller then asks for `--log`.
    pub fn default_path(repo: &RepoSlug, env: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
        let base = env("XDG_STATE_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                env("LOCALAPPDATA")
                    .filter(|v| !v.is_empty())
                    .map(PathBuf::from)
            })
            .or_else(|| {
                env("HOME")
                    .filter(|v| !v.is_empty())
                    .map(|h| Path::new(&h).join(".local").join("state"))
            })?;
        Some(
            base.join("spl-asdlc")
                .join(format!("{}-{}.jsonl", repo.owner, repo.name)),
        )
    }
}

/// Create `dir` and its missing parents, private to the bridge's account
/// (0700 on unix). A directory that exists is left as it is.
pub(crate) fn private_dir(dir: &Path) -> io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(dir)
}

/// Open the log for appending, private to the bridge's account: on unix a
/// new file is created 0600 and an existing one is narrowed to 0600.
fn open_private(path: &Path) -> io::Result<std::fs::File> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let file = options.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = file.metadata()?.permissions();
        if permissions.mode() & 0o777 != 0o600 {
            permissions.set_mode(0o600);
            file.set_permissions(permissions)?;
        }
    }
    Ok(file)
}

/// The last waiting note of each (repository, object) in a log file.
fn last_waits(path: Option<&Path>) -> BTreeMap<(String, String), Value> {
    let entries = path
        .and_then(|p| ActionLog::read(p).ok())
        .map(|(entries, _)| entries)
        .unwrap_or_default();
    entries
        .into_iter()
        .filter(|e| e.outcome == Outcome::Waiting && e.action == WAITING)
        .map(|e| ((e.repo, e.object), e.inputs))
        .collect()
}

/// The (repository, object) pairs whose last entry in a log file is a
/// hands-off note.
fn last_held(path: Option<&Path>) -> std::collections::BTreeSet<(String, String)> {
    let entries = path
        .and_then(|p| ActionLog::read(p).ok())
        .map(|(entries, _)| entries)
        .unwrap_or_default();
    let mut held = std::collections::BTreeSet::new();
    for e in entries {
        let key = (e.repo, e.object);
        if e.outcome == Outcome::Waiting && e.action == HANDS_OFF {
            held.insert(key);
        } else {
            held.remove(&key);
        }
    }
    held
}

/// One line of the human-readable account of a tick (`tick --dry-run`
/// prints these).
pub fn describe(e: &Entry) -> String {
    let outcome = match &e.outcome {
        Outcome::Done => "done".to_string(),
        Outcome::Failed(d) => format!("FAILED: {d}"),
        Outcome::WouldDo => "would do".to_string(),
        Outcome::Waiting if e.action == HANDS_OFF => {
            let label = e.inputs["label"].as_str().unwrap_or_default();
            return format!("{} {} {}: hands-off — {label}", e.at, e.repo, e.object);
        }
        Outcome::Waiting => {
            let reason = e.inputs["reason"].as_str().unwrap_or_default();
            return format!("{} {} {}: waiting — {reason}", e.at, e.repo, e.object);
        }
    };
    format!(
        "{} {} {}: {} — {}",
        e.at, e.repo, e.object, e.action, outcome
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn repo() -> RepoSlug {
        RepoSlug::parse("acme/widget").unwrap()
    }

    fn at() -> Timestamp {
        Timestamp::parse("2026-10-05T12:00:00Z").unwrap()
    }

    #[test]
    fn a_tick_that_did_nothing_creates_no_file() {
        let dir = std::env::temp_dir().join(format!("spl-asdlc-log-{}", std::process::id()));
        let path = dir.join("nothing.jsonl");
        let mut log = ActionLog::at(path.clone());
        log.flush().unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn entries_are_appended_once_and_read_back() {
        let dir = std::env::temp_dir().join(format!("spl-asdlc-log-a-{}", std::process::id()));
        let path = dir.join("acme-widget.jsonl");
        let _ = std::fs::remove_file(&path);
        let mut log = ActionLog::at(path.clone());
        log.record(
            at(),
            &repo(),
            "pr#12",
            "merge",
            json!({"head": "abc"}),
            Outcome::Done,
        );
        log.record(
            at(),
            &repo(),
            "ABC-3",
            "status To Deploy",
            json!({}),
            Outcome::Failed("refused".into()),
        );
        log.flush().unwrap();
        log.flush().unwrap();
        let mut second = ActionLog::at(path.clone());
        second.record(at(), &repo(), "pr#13", "comment", json!({}), Outcome::Done);
        second.flush().unwrap();
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"{\"cut by a cra")
            .unwrap();
        let (entries, bad) = ActionLog::read(&path).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(bad, 1);
        assert_eq!(entries[1].outcome, Outcome::Failed("refused".into()));
        assert_eq!(log.failures().len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_default_path_follows_the_state_directory() {
        let env = |k: &str| match k {
            "HOME" => Some("/u".to_string()),
            _ => None,
        };
        let p = ActionLog::default_path(&repo(), &env).unwrap();
        assert!(
            p.ends_with(".local/state/spl-asdlc/acme-widget.jsonl"),
            "{p:?}"
        );
        let xdg = |k: &str| (k == "XDG_STATE_HOME").then(|| "/state".to_string());
        assert_eq!(
            ActionLog::default_path(&repo(), &xdg).unwrap(),
            PathBuf::from("/state/spl-asdlc/acme-widget.jsonl")
        );
        assert!(ActionLog::default_path(&repo(), &|_| None).is_none());
    }

    #[test]
    fn why_a_pr_waits_is_noted_only_when_it_changes_across_ticks() {
        let dir = std::env::temp_dir().join(format!("spl-asdlc-log-w-{}", std::process::id()));
        let path = dir.join("acme-widget.jsonl");
        let _ = std::fs::remove_file(&path);
        let other = RepoSlug::parse("acme/gadget").unwrap();
        let tick = |notes: &[(&RepoSlug, &str, &str)]| {
            let mut log = ActionLog::at(path.clone());
            let noted: Vec<bool> = notes
                .iter()
                .map(|(repo, head, reason)| log.note_waiting(at(), repo, "pr#7", head, reason))
                .collect();
            log.flush().unwrap();
            noted
        };
        assert_eq!(tick(&[(&repo(), "h1", "a draft")]), vec![true]);
        assert_eq!(
            tick(&[(&repo(), "h1", "a draft"), (&repo(), "h1", "a draft")]),
            vec![false, false],
            "the same reason at the same head: noted once, whatever the tick"
        );
        assert_eq!(
            tick(&[
                (&repo(), "h2", "a draft"),
                (&repo(), "h2", "red"),
                (&other, "h2", "red")
            ]),
            vec![true, true, true],
            "a new head, a new reason, another repository's PR"
        );
        let (entries, _) = ActionLog::read(&path).unwrap();
        assert_eq!(entries.len(), 4);
        assert!(entries.iter().all(|e| e.outcome == Outcome::Waiting));
        assert_eq!(
            describe(&entries[0]),
            "2026-10-05T12:00:00Z acme/widget pr#7: waiting — a draft"
        );
        assert!(
            ActionLog::in_memory().note_waiting(at(), &repo(), "pr#7", "h2", "red"),
            "a log without a file remembers nothing"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_hands_off_object_is_noted_when_it_starts_not_at_every_tick() {
        let dir = std::env::temp_dir().join(format!("spl-asdlc-log-h-{}", std::process::id()));
        let path = dir.join("acme-widget.jsonl");
        let _ = std::fs::remove_file(&path);
        let label = "spl-asdlc:hands-off";
        let tick = |f: &dyn Fn(&mut ActionLog) -> bool| {
            let mut log = ActionLog::at(path.clone());
            let noted = f(&mut log);
            log.flush().unwrap();
            noted
        };
        let note = |log: &mut ActionLog| log.note_hands_off(at(), &repo(), "ABC-3", label);
        assert!(tick(&note), "the first tick that leaves it alone");
        assert!(!tick(&note), "still hands off: not noted again");
        // Handed back: the bridge acts on it again, then a person takes it
        // over once more.
        assert!(!tick(&|log| {
            log.record(
                at(),
                &repo(),
                "ABC-3",
                "status To Deploy",
                json!({}),
                Outcome::Done,
            );
            false
        }));
        assert!(tick(&note), "taken over again after the bridge acted");
        let (entries, _) = ActionLog::read(&path).unwrap();
        let actions: Vec<&str> = entries.iter().map(|e| e.action.as_str()).collect();
        assert_eq!(actions, vec![HANDS_OFF, "status To Deploy", HANDS_OFF]);
        assert_eq!(entries[0].outcome, Outcome::Waiting);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn the_log_is_private_to_the_bridges_account() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        let base = std::env::temp_dir().join(format!("spl-asdlc-log-p-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let dir = base.join("spl-asdlc");
        let path = dir.join("acme-widget.jsonl");
        let mut log = ActionLog::at(path.clone());
        log.record(at(), &repo(), "pr#12", "merge", json!({}), Outcome::Done);
        log.flush().unwrap();
        assert_eq!(mode(&dir), 0o700, "a directory it creates");
        assert_eq!(mode(&path), 0o600, "a file it creates");

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o664)).unwrap();
        let mut next = ActionLog::at(path.clone());
        next.record(at(), &repo(), "pr#13", "merge", json!({}), Outcome::Done);
        next.flush().unwrap();
        assert_eq!(mode(&path), 0o600, "an existing file is narrowed");
        assert_eq!(ActionLog::read(&path).unwrap().0.len(), 2);
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn a_line_says_what_was_done_and_how_it_ended() {
        let mut log = ActionLog::in_memory();
        log.record(at(), &repo(), "pr#12", "merge", json!({}), Outcome::WouldDo);
        assert!(describe(&log.entries()[0]).ends_with("pr#12: merge — would do"));
    }
}
