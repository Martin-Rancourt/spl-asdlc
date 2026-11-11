//! The unified release note (spec 05 §12): with `release.note_writer`, the
//! first candidate of a release gets a task « Write the unified release
//! note », and with `release.note_reviewer` a review task it blocks. The last
//! of them blocks the release item, so the GO, and with it the publication,
//! waits for it through the release item's own blockers (spec 05 §8).

use serde_json::json;

use super::identity;
use super::merge::for_tracker;
use super::release::{Candidate, section};
use super::{Report, Tick};
use crate::action_log::ActionLog;
use crate::config::Release;
use crate::method::release::findings;
use crate::method::texts;
use crate::method::vocabulary::Status;
use crate::tracker::{Item, ItemKind, NewItem, RelationKind};

/// The two steps of the unified release note.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Write,
    Review,
}

impl Step {
    fn key(self) -> &'static str {
        match self {
            Step::Write => "write",
            Step::Review => "review",
        }
    }
}

/// The marker of a release's note task: one per version and step, whatever
/// the candidate.
pub fn marker(version: &str, step: Step) -> String {
    texts::marker(
        "release-note",
        &[("version", version), ("step", step.key())],
    )
}

/// The paths of the release's note in the repository: the CHANGELOG, and the
/// version's own notes file with `release.changelog_dir`. With a writer
/// configured they are bookkeeping after the candidate (spec 05 §8): the
/// unified note is written on the release branch after the cut.
pub fn note_paths(release: &Release, version: &str) -> Vec<String> {
    let mut out = vec![crate::prepare::CHANGELOG.to_string()];
    if let Some(dir) = &release.changelog_dir {
        out.push(format!("{}/{version}.md", dir.trim_end_matches('/')));
    }
    out
}

/// The task title of a step.
pub fn title(step: Step, version: &str) -> String {
    match step {
        Step::Write => format!("Write the unified release note of {version}"),
        Step::Review => format!("Review the unified release note of {version}"),
    }
}

/// The task description of a step: its contract and its protocol.
pub fn body(step: Step, release: &str, version: &str, branch: &str, paths: &[String]) -> String {
    let files = paths
        .iter()
        .map(|p| format!("`{p}`"))
        .collect::<Vec<_>>()
        .join(" and ");
    let work = match step {
        Step::Write => format!(
            "## Goal\n\n\
             One release note for {version}, written for the user, in place of the tickets' notes put one after the other.\n\n\
             ## What to do\n\n\
             1. Read the release note the cut prepared for {version} on the release branch `{branch}`: its section in {files}, and the release item {release}, whose `## Release note` lists each ticket by its own note.\n\
             2. Rewrite it as one digestible note: tickets about the same thing merged into one entry, in the words of the person who uses the product, without ticket ids or internal vocabulary.\n\
             3. Commit it on `{branch}`, in {files} only, the version's heading kept: through a PR whose base is `{branch}` and which changes nothing else.\n\
             4. Mark this task Done once the note is on `{branch}`.\n\n\
             ## Non-goals\n\n\
             No change to any other file, no change to the version, no new candidate: the GO reads the note at the branch's tip.\n\n\
             ## Acceptance criteria\n\n\
             - [ ] The unified note of {version} is on `{branch}`, in {files}.\n"
        ),
        Step::Review => format!(
            "## Goal\n\n\
             The unified release note of {version} approved before its publication.\n\n\
             ## What to do\n\n\
             1. Read the unified note on the release branch `{branch}`, in {files}.\n\
             2. Approve it: mark this task Done. To change it, change it on `{branch}` as the writing task says, then mark this task Done.\n\n\
             ## Non-goals\n\n\
             No change to any other file.\n\n\
             ## Acceptance criteria\n\n\
             - [ ] The unified note of {version} on `{branch}` is approved.\n"
        ),
    };
    format!(
        "{work}\nThe release {release} is not published while this task is open.\n\n{}\n\n{}\n",
        texts::KEEP_MARKER,
        marker(version, step)
    )
}

/// The note tasks of the release, created at its first candidate, then
/// related: writing blocks review, the last one blocks the release item.
/// A task found again by its marker is never created twice; a relation
/// already there is not written again. Tasks are created at the first
/// candidate only (`create`): a writer configured while a release is in
/// flight takes effect at the next release. Without a writer, nothing.
#[allow(clippy::too_many_arguments)]
pub fn ensure(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    item: &Item,
    children: &[Item],
    c: &Candidate,
    branch: &str,
) {
    let create = c.rc == 1;
    let release = section(t);
    let Some(writer) = &release.note_writer else {
        return;
    };
    let version = c.version.to_string();
    let paths = note_paths(release, &version);
    let mut steps = vec![(Step::Write, writer)];
    if let Some(reviewer) = &release.note_reviewer {
        steps.push((Step::Review, reviewer));
    }
    let mut keys = Vec::new();
    for (step, who) in steps {
        let m = marker(&version, step);
        let found = match identity::find(t, log, report, children, &m, |ch| {
            texts::has_marker(&ch.description, &m)
        }) {
            Ok(f) => f,
            Err(e) => {
                t.unreadable(report, &item.key, e);
                return;
            }
        };
        let key = match found.as_slice() {
            [one] => one.key.clone(),
            [] if !create => return,
            [] => {
                let assignee = t.tracker.user(who).ok();
                let made = identity::create(
                    t,
                    log,
                    report,
                    &item.key,
                    &format!("create \"{}\"", title(step, &version)),
                    json!({"step": step.key(), "assignee": assignee.as_ref().map(|u| u.name.as_str())}),
                    &NewItem {
                        title: title(step, &version),
                        description: for_tracker(
                            t,
                            &body(step, &item.key, &version, branch, &paths),
                        ),
                        labels: Vec::new(),
                        status: Status::Ready,
                        priority: None,
                        assignee,
                        parent: Some(item.key.clone()),
                        kind: ItemKind::Task,
                    },
                );
                match made {
                    Some(i) => i.key,
                    None => return,
                }
            }
            many => {
                let keys: Vec<&str> = many.iter().map(|i| i.key.as_str()).collect();
                t.unreadable(
                    report,
                    &item.key,
                    format!(
                        "several {} tasks for {version}: {}",
                        step.key(),
                        keys.join(", ")
                    ),
                );
                return;
            }
        };
        keys.push(key);
    }
    keys.push(item.key.clone());
    for pair in keys.windows(2) {
        block(t, log, report, &pair[0], &pair[1]);
    }
}

/// `a` blocks `b`, unless it already does.
fn block(t: &Tick, log: &mut ActionLog, report: &mut Report, a: &str, b: &str) {
    let present = match t.tracker.relations(b) {
        Ok(r) if r.cut => {
            t.unreadable(
                report,
                b,
                "the relation list is cut: whether the note task blocks it cannot be told",
            );
            return;
        }
        Ok(r) => r
            .blocked_by
            .into_iter()
            .any(|r| r.other.eq_ignore_ascii_case(a)),
        Err(e) => {
            t.unreadable(report, b, format!("relations: {e}"));
            return;
        }
    };
    if present {
        return;
    }
    let mut blockers_of = |x: &str| -> Result<Vec<String>, String> {
        t.tracker
            .relations(x)
            .map(|r| r.blocked_by.into_iter().map(|r| r.other).collect())
            .map_err(|e| e.to_string())
    };
    if let Err(e) = findings::may_block(a, b, &mut blockers_of) {
        t.unreadable(report, a, format!("{a} cannot block {b}: {e}"));
        return;
    }
    t.act(
        log,
        report,
        a,
        &format!("relate Blocks {b}"),
        json!({}),
        || {
            t.tracker
                .relate(a, b, RelationKind::Blocks)
                .map_err(|e| e.to_string())
        },
    );
}
