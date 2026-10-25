//! The GitHub → item sync (`docs/reference/livela-contract.md` §5): a pass a
//! host runs beside the bridge, through the same inputs, that keeps a
//! project's tickets current with their PRs even when no surface is open.
//! It does two things and nothing else:
//!
//! - it links a PR that names a ticket of the project to that ticket
//!   ([`names_item`]), as a tracker's own GitHub integration does; a link a
//!   person removed is never laid again;
//! - it records on each open ticket the status of each PR linked to it
//!   ([`PrStatus`]): draft, open with its checks, merged, released in a
//!   tag, closed.
//!
//! It never moves a status, an assignee, a label or a description: those
//! are the bridge's. It talks to the tracker through the `Tracker` trait and
//! to GitHub through the App, and needs no person's token. The method does
//! not require it.
//!
//! It reads the open PRs, the PRs merged or closed since its last pass, and
//! the PRs linked to tickets still open; its memory (the last pass, the links
//! it laid, the PRs it follows) is a file beside the action log, kept by the
//! host. A value already on the item is never written again, so a pass with
//! nothing new writes nothing in the tracker (contract §2).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::action_log::ActionLog;
use crate::config::Config;
use crate::github::{CompareStatus, GitHub, PrState, PullRequest, RepoSlug, Timestamp};
use crate::job::{Host, PassOutcome, RepositoryHost};
use crate::lock::TickLock;
use crate::method::checks::{self, CheckState};
use crate::method::release::shape::Shape;
use crate::tracker::{Checks, PrStatus, Tracker};

/// One page per pass. GitHub's issue list has a maximum page size of 100.
const PAGE: usize = 100;

/// One pass for a host that embeds the crate: the configuration read at
/// the repository's default branch with the instance overlay as a value
/// (`crate::embed::read_config`), the GitHub client the host built
/// (`crate::host::app_github`). Its memory goes beside `host.log_path`.
pub fn pass_from_repository(host: &RepositoryHost, github: &dyn GitHub) -> PassOutcome {
    let read = match crate::embed::read_config(github, &host.repository, host.instance.as_ref()) {
        Ok(r) => r,
        Err(e) => return PassOutcome::failed(e.to_string()),
    };
    run(
        &read.config,
        host.tracker,
        github,
        host.budget,
        host.log_path.clone(),
        host.env,
    )
}

/// One pass from a configuration file on the host's disk, with the GitHub
/// client the configuration asks for.
pub fn pass(host: &Host) -> PassOutcome {
    let config = match Config::load(host.config_path) {
        Ok(c) => c,
        Err(errors) => {
            return PassOutcome::failed(format!(
                "{}: invalid configuration: {errors}",
                host.config_path.display()
            ));
        }
    };
    let github = match crate::host::github(&config, host.env) {
        Ok(g) => g,
        Err(e) => return PassOutcome::failed(e),
    };
    pass_with(host, &config, &github)
}

/// One pass with a given configuration and GitHub client (tests, and hosts
/// that build their own). `host.config_path` is not read.
pub fn pass_with(host: &Host, config: &Config, github: &dyn GitHub) -> PassOutcome {
    run(
        config,
        host.tracker,
        github,
        host.budget,
        host.log_path.clone(),
        host.env,
    )
}

/// The memory of the sync between its passes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Memory {
    /// The last PR handled, ordered by `(updated_at, number)`. `since` is
    /// absent in a new repository: its first pass starts at history's start.
    since: Option<String>,
    #[serde(default)]
    handled_at_since: BTreeSet<u64>,
    /// `(PR URL, ticket)`: the links this sync laid. One the tracker no
    /// longer has, a person removed: it is never laid again.
    laid: BTreeSet<(String, String)>,
    /// The PRs linked to a ticket still open, by URL: read at each pass
    /// until every ticket they are linked to is resolved.
    followed: BTreeMap<String, u64>,
}

/// The file of the sync's memory beside the action log
/// (`<owner>-<name>.sync.json` beside `<owner>-<name>.jsonl`).
pub fn memory_path(log: &Path) -> PathBuf {
    let stem = log
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    log.with_file_name(format!("{stem}.sync.json"))
}

fn load(path: &Path) -> Result<Memory, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text)
            .map_err(|e| format!("{} is not the sync's memory: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Memory::default()),
        Err(e) => Err(format!("{} could not be read: {e}", path.display())),
    }
}

fn save(path: &Path, memory: &Memory) -> std::io::Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        crate::action_log::private_dir(dir)?;
    }
    let text = serde_json::to_string_pretty(memory).expect("the memory serialises");
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

/// What one pass did, for its line.
#[derive(Default)]
struct Tally {
    read: usize,
    links: usize,
    statuses: usize,
    failures: Vec<String>,
}

fn run(
    config: &Config,
    tracker: &dyn Tracker,
    github: &dyn GitHub,
    budget: Option<Duration>,
    log_path: Option<PathBuf>,
    env: &dyn Fn(&str) -> Option<String>,
) -> PassOutcome {
    let started = Instant::now();
    let deadline = budget.map(|b| started + b);
    let Some(repo) = RepoSlug::parse(&config.product.repository) else {
        return PassOutcome::failed("product.repository is not <owner>/<repo>");
    };
    let path = log_path
        .or_else(|| ActionLog::default_path(&repo, env))
        .map(|p| memory_path(&p));
    // One sync at a time per repository; its own lock, apart from the tick's.
    let _lock = match &path {
        Some(p) => {
            let lock_path = TickLock::beside(p);
            match TickLock::try_acquire(&lock_path) {
                Ok(Some(l)) => Some(l),
                Ok(None) => {
                    return PassOutcome {
                        ok: true,
                        message: format!(
                            "{repo}: a GitHub sync is already running (lock {}): this one does nothing",
                            lock_path.display()
                        ),
                    };
                }
                Err(e) => {
                    return PassOutcome::failed(format!(
                        "{repo}: the sync's lock {} could not be taken: {e}",
                        lock_path.display()
                    ));
                }
            }
        }
        None => None,
    };
    // Both sides readable before anything is written.
    if let Err(e) = tracker.identity() {
        return PassOutcome::failed(format!("{repo}: the tracker: {e}"));
    }
    if let Err(e) = github.identity() {
        return PassOutcome::failed(format!("{repo}: GitHub identity: {e}"));
    }
    let mut memory = match &path {
        Some(p) => match load(p) {
            Ok(m) => m,
            Err(e) => return PassOutcome::failed(format!("{repo}: {e}")),
        },
        None => Memory::default(),
    };
    let before = memory.clone();
    let since = memory.since.as_deref().and_then(Timestamp::parse);
    let prefixes = match prefixes(config, tracker) {
        Ok(p) => p,
        Err(e) => return PassOutcome::failed(format!("{repo}: the tracker: {e}")),
    };

    let handled: Vec<_> = memory.handled_at_since.iter().copied().collect();
    let listed = match github.updated_pulls(&repo, since, &handled, PAGE) {
        Ok(p) => p,
        Err(e) => return PassOutcome::failed(format!("{repo}: the PRs after the checkpoint: {e}")),
    };
    let mut prs: BTreeMap<String, PullRequest> =
        listed.iter().cloned().map(|p| (p.url.clone(), p)).collect();
    let mut tally = Tally::default();
    for (url, number) in memory.followed.clone() {
        if prs.contains_key(&url) {
            continue;
        }
        match github.pull(&repo, number) {
            Ok(pr) => {
                prs.insert(url, pr);
            }
            Err(e) if e.is_not_found() => {
                memory.followed.remove(&url);
            }
            Err(e) => tally.failures.push(format!("PR #{number}: {e}")),
        }
    }

    let mut releases = Releases::new(config, &repo, github);
    let mut spent = false;
    for pr in listed.iter().chain(
        prs.values()
            .filter(|p| !listed.iter().any(|l| l.url == p.url)),
    ) {
        let checkpointed = listed.iter().any(|p| p.url == pr.url);
        if deadline.is_some_and(|d| Instant::now() >= d) {
            spent = true;
            break;
        }
        tally.read += 1;
        if let Err(e) = link(tracker, &prefixes, pr, &mut memory, &mut tally) {
            tally.failures.push(format!("PR #{}: {e}", pr.number));
            if checkpointed {
                break;
            }
            continue;
        }
        if let Err(e) = record(tracker, &mut releases, pr, &mut memory, &mut tally) {
            tally.failures.push(format!("PR #{}: {e}", pr.number));
            if checkpointed {
                break;
            }
            continue;
        }
        if checkpointed {
            let Some(updated) = pr.updated_at else {
                tally.failures.push(format!(
                    "PR #{}: no updated_at for the checkpoint",
                    pr.number
                ));
                continue;
            };
            if memory.since.as_deref() != Some(&updated.to_string()) {
                memory.since = Some(updated.to_string());
                memory.handled_at_since.clear();
            }
            memory.handled_at_since.insert(pr.number);
            if let Some(p) = &path {
                if let Err(e) = save(p, &memory) {
                    tally.failures.push(format!(
                        "checkpoint after PR #{} could not be written: {e}",
                        pr.number
                    ));
                    break;
                }
            }
        }
    }
    let mut message = format!(
        "{repo}: GitHub sync: {} PR(s) read, {} link(s) laid, {} status(es) recorded",
        tally.read, tally.links, tally.statuses
    );
    let mut saved = true;
    if let (Some(p), true) = (&path, memory != before) {
        if let Err(e) = save(p, &memory) {
            message.push_str(&format!(
                "; its memory {} could not be written: {e}",
                p.display()
            ));
            saved = false;
        }
    }
    if spent {
        message.push_str("; the time budget is spent: the next pass resumes");
    }
    for f in &tally.failures {
        message.push_str(&format!("\n{repo}: {f}"));
    }
    PassOutcome {
        ok: !spent && saved && tally.failures.is_empty(),
        message,
    }
}

/// The prefixes of the project's tickets: `tracker.team_keys`, or the
/// tracker's team key.
fn prefixes(config: &Config, tracker: &dyn Tracker) -> crate::tracker::TResult<Vec<String>> {
    if !config.tracker.team_keys.is_empty() {
        return Ok(config.tracker.team_keys.clone());
    }
    Ok(vec![tracker.team_key()?])
}

/// Link `pr` to each ticket of the project it names and does not link yet.
fn link(
    tracker: &dyn Tracker,
    prefixes: &[String],
    pr: &PullRequest,
    memory: &mut Memory,
    tally: &mut Tally,
) -> Result<(), String> {
    let mut keys = candidates(prefixes, &[&pr.head_ref, &pr.title, &pr.body]);
    keys.extend(tracker.keys_in_text(&pr.body));
    keys.sort();
    keys.dedup();
    let named: Vec<String> = keys
        .into_iter()
        .filter(|k| names_item(k, &pr.head_ref, &pr.title, &pr.body))
        .collect();
    if named.is_empty() {
        return Ok(());
    }
    let linked = tracker
        .items_linked_to_pr(&pr.url)
        .map_err(|e| e.to_string())?;
    for key in named {
        let item = match tracker.get(&key) {
            Ok(i) => i,
            Err(e) if e.is_not_found() => continue,
            Err(e) => return Err(e.to_string()),
        };
        let pair = (pr.url.clone(), item.key.clone());
        if linked.iter().any(|k| k.eq_ignore_ascii_case(&item.key)) || memory.laid.contains(&pair) {
            continue;
        }
        tracker
            .link_pr(&item.key, &pr.url, &pr.title)
            .map_err(|e| e.to_string())?;
        memory.laid.insert(pair);
        tally.links += 1;
    }
    Ok(())
}

/// Record the status of `pr` on each open ticket it is linked to, and
/// follow it while one is open.
fn record(
    tracker: &dyn Tracker,
    releases: &mut Releases,
    pr: &PullRequest,
    memory: &mut Memory,
    tally: &mut Tally,
) -> Result<(), String> {
    let linked = tracker
        .items_linked_to_pr(&pr.url)
        .map_err(|e| e.to_string())?;
    let mut open = Vec::new();
    for key in linked {
        match tracker.get(&key) {
            Ok(item) if !item.state.is_resolved() && !item.archived => open.push(item.key),
            Ok(_) => {}
            Err(e) if e.is_not_found() => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    if open.is_empty() {
        memory.followed.remove(&pr.url);
        return Ok(());
    }
    memory.followed.insert(pr.url.clone(), pr.number);
    if !tracker.holds_pr_status() {
        return Ok(());
    }
    let status = status_of(pr, releases)?;
    for key in open {
        if tracker
            .pr_status(&key, &pr.url)
            .map_err(|e| e.to_string())?
            .as_ref()
            == Some(&status)
        {
            continue;
        }
        tracker
            .record_pr_status(&key, &pr.url, &status)
            .map_err(|e| e.to_string())?;
        tally.statuses += 1;
    }
    Ok(())
}

/// Where a PR stands.
fn status_of(pr: &PullRequest, releases: &mut Releases) -> Result<PrStatus, String> {
    Ok(match pr.state {
        PrState::Closed => PrStatus::Closed,
        PrState::Merged => match pr.merge_commit_sha.as_deref() {
            Some(sha) => match releases.containing(sha)? {
                Some(tag) => PrStatus::Released(tag),
                None => PrStatus::Merged,
            },
            None => PrStatus::Merged,
        },
        PrState::Open if pr.draft => PrStatus::Draft,
        PrState::Open => PrStatus::Open(match checks::state(&pr.checks, &BTreeSet::new()) {
            CheckState::Green => Checks::Passing,
            CheckState::Red => Checks::Failing,
            CheckState::None => Checks::None,
            CheckState::Pending | CheckState::Unreadable => Checks::Pending,
        }),
    })
}

/// The release tags of the product, read once per pass, oldest version
/// first, and which one first holds a commit.
struct Releases<'a> {
    shape: Shape,
    repo: &'a RepoSlug,
    github: &'a dyn GitHub,
    tags: Option<Vec<String>>,
    found: BTreeMap<String, Option<String>>,
}

impl<'a> Releases<'a> {
    fn new(config: &Config, repo: &'a RepoSlug, github: &'a dyn GitHub) -> Releases<'a> {
        Releases {
            shape: Shape::of(&config.product.name, &config.release),
            repo,
            github,
            tags: None,
            found: BTreeMap::new(),
        }
    }

    /// The oldest release tag whose commit holds `sha`, or `None`.
    fn containing(&mut self, sha: &str) -> Result<Option<String>, String> {
        if let Some(t) = self.found.get(sha) {
            return Ok(t.clone());
        }
        if self.tags.is_none() {
            let mut tags: Vec<_> = self
                .github
                .tags(self.repo, self.shape.release_tag_prefix())
                .map_err(|e| e.to_string())?
                .into_iter()
                .filter_map(|t| self.shape.parse_release(&t.name).map(|v| (v, t.name)))
                .collect();
            tags.sort();
            self.tags = Some(tags.into_iter().map(|(_, n)| n).collect());
        }
        let tags = self.tags.as_deref().unwrap_or_default();
        let holds = |tag: &str| -> Result<bool, String> {
            self.github
                .compare(self.repo, sha, tag)
                .map(|c| matches!(c.status, CompareStatus::Ahead | CompareStatus::Identical))
                .map_err(|e| e.to_string())
        };
        // Not in the newest release: in none.
        let found = match tags.last() {
            Some(newest) if holds(newest)? => {
                let mut first = newest.clone();
                for tag in tags {
                    if holds(tag)? {
                        first = tag.clone();
                        break;
                    }
                }
                Some(first)
            }
            _ => None,
        };
        self.found.insert(sha.to_string(), found.clone());
        Ok(found)
    }
}

/// The identifiers of the shape `<prefix>-<code>` in the texts, for each
/// prefix, as written (the tracker resolves them).
fn candidates(prefixes: &[String], texts: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    for text in texts {
        let lower = text.to_ascii_lowercase();
        for prefix in prefixes {
            let needle = format!("{}-", prefix.to_ascii_lowercase());
            let mut from = 0;
            while let Some(k) = lower[from..].find(&needle) {
                let start = from + k;
                let code_start = start + needle.len();
                let code: String = text[code_start..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric())
                    .collect();
                let before_ok = start == 0 || !text.as_bytes()[start - 1].is_ascii_alphanumeric();
                if before_ok && !code.is_empty() {
                    out.push(format!("{}-{code}", &text[start..code_start - 1]));
                }
                from = start + 1;
            }
        }
    }
    out
}

/// The spans of `<letters and digits>-<code>` in `hay`, the code matched
/// without case and followed by no letter or digit; the prefix is any short
/// run before the dash (the human prefix of a Livela id may change).
fn id_spans(hay: &str, code: &str) -> Vec<(usize, usize)> {
    let h = hay.to_ascii_lowercase();
    let needle = format!("-{}", code.to_ascii_lowercase());
    let bytes = h.as_bytes();
    let alnum = |i: usize| bytes.get(i).is_some_and(|b| b.is_ascii_alphanumeric());
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(k) = h[from..].find(&needle) {
        let dash = from + k;
        let end = dash + needle.len();
        let mut start = dash;
        while start > 0 && alnum(start - 1) && dash - (start - 1) <= 8 {
            start -= 1;
        }
        if start < dash && (start == 0 || !alnum(start - 1)) && !alnum(end) {
            out.push((start, end));
        }
        from = dash + 1;
    }
    out
}

const ITEM_SCHEME: &str = "livela://item/";

/// Whether a PR names the ticket `key` (`TD-7K3F`), by the rules a tracker
/// links on (Livela's `pr_names_item`, which `docs/agents/process.md` step 5
/// asks of a PR): its branch (`claude/td-7k3f-fix`: the id right after the
/// start or a `/`, then a `-` or the end), its title (`feat: … (TD-7K3F)`),
/// or its body on purpose only: a line that starts with `Refs`, `Closes`,
/// `Fixes`, `Resolves` or `Linear:` followed by the id or a link carrying
/// it, or the item's address `livela://item/TD-7K3F`. A body that mentions a
/// ticket in passing names none.
pub fn names_item(key: &str, branch: &str, title: &str, body: &str) -> bool {
    let Some(code) = key
        .rsplit('-')
        .next()
        .filter(|c| !c.is_empty() && *c != key)
    else {
        return false;
    };
    let b = branch.as_bytes();
    let in_branch = id_spans(branch, code)
        .into_iter()
        .any(|(s, e)| (s == 0 || b[s - 1] == b'/') && (e == b.len() || b[e] == b'-'));
    if in_branch || !id_spans(title, code).is_empty() {
        return true;
    }
    let item_link = body.match_indices(ITEM_SCHEME).any(|(i, m)| {
        let id: String = body[i + m.len()..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect();
        id_spans(&id, code)
            .into_iter()
            .any(|(s, e)| s == 0 && e == id.len())
    });
    item_link
        || body.lines().any(|l| {
            let l = l
                .trim_start()
                .trim_start_matches(['-', '*', '>'])
                .trim_start();
            let lower = l.to_ascii_lowercase();
            ["refs", "closes", "fixes", "resolves", "linear"]
                .iter()
                .any(|w| {
                    // The word itself (not "referenced"), then a colon or a
                    // space, then the id.
                    lower
                        .strip_prefix(w)
                        .is_some_and(|rest| rest.starts_with(':') || rest.starts_with(' '))
                        && !id_spans(&l[w.len()..], code).is_empty()
                })
        })
}

#[cfg(test)]
mod tests;
