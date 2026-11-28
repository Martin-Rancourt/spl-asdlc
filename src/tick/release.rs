//! The release pass (`docs/reference/05-release-cycle.md`, spec 07 step 2), for
//! the newest candidate of the product:
//!
//! 1. the release item, found by its label and marker or created;
//! 2. the candidate's record (comment and `## Candidates` line), the
//!    embarked tickets labelled; once the candidate's gate is green, and
//!    never before, the angle sub-items and the consolidation item of the
//!    candidate, the consolidation item held until every angle report is
//!    merged, then the brief (BR-ASDLC-004, BR-ASDLC-020); a candidate with no QA
//!    angle (`release.angles = []`) gets the brief alone; the consolidation item of
//!    an older candidate Done; a red gate whose automatic rerun settled gets
//!    its urgent gate-fix ticket, which a later green gate closes
//!    (BR-ASDLC-016);
//! 3. the angle PRs: each sub-item follows its PR (In Progress, In Review,
//!    back In Progress on changes requested); merged into the release
//!    branch when green and carrying a valid report alone, with no
//!    approval, the sub-item To Deploy;
//! 4. the coordinator's verdict PR, merged once every angle PR is; the
//!    accepted findings of the merged verdict filed, the coordination item
//!    Done;
//! 5. the GO (which needs the verdict `go`), and on GO the release tag on the candidate and its GitHub
//!    release published, with the notes the cut prepared;
//! 6. otherwise the HOLD said once, the escalation when the passes are
//!    spent, the stagnation notice;
//! 7. once the release is published: the consolidation items Done, then the
//!    closure (`closure.rs`); the closure of the cycle before, when the next
//!    cycle was cut before it ended, goes on beside it.
//!
//! Points the specification leaves open are not settled here; each waits,
//! as `.spl-asdlc/open-points.md` lists them (the human GO, a raised `max_rc_passes`).

use serde_json::json;

use crate::action_log::ActionLog;
use crate::config::{LaterPasses, Release};
use crate::github::{CompareStatus, Comparison, GitRef, ObjectType, PullRequest, same_login};
use crate::method::checks::{self, CheckState};
use crate::method::release::findings;
use crate::method::release::gate;
use crate::method::release::go::{self, AngleState, GoFacts, Policy, VerdictState};
use crate::method::release::note;
use crate::method::release::prepare;
use crate::method::release::prs;
use crate::method::release::report::{self, Finding, Report as AngleReport};
use crate::method::release::shape::{Shape, Version};
use crate::method::release::verdict;
use crate::method::review::{self, Approval, BranchState};
use crate::method::texts::{self, AngleLine, AngleScope, BridgeItem, protocol};
use crate::method::vocabulary::{self, Status};
use crate::tracker::{Item, ItemKind, NewItem, Priority, RelationKind};

use super::merge::{for_tracker, tracker_said};
use super::{Report, Tick, closure, identity, method_texts};

/// The candidate in flight, as the tags say.
pub struct Candidate {
    pub tag: String,
    pub version: Version,
    pub rc: u64,
    pub sha: String,
    pub class: String,
    pub tagged_at: Option<crate::github::Timestamp>,
    /// The rc of its cycle's first candidate: 1, or after an abandoned cycle
    /// of the same version, the candidate after the last one that cycle
    /// recorded (spec 05 §13, ASDLC-R7K6). Set by `cycle_start`.
    pub first_rc: u64,
}

impl Candidate {
    /// The candidate's pass in its cycle: 1 for the cycle's first
    /// candidate, whatever its rc (`max_rc_passes`, `later_passes`).
    pub fn pass(&self) -> u64 {
        self.rc.saturating_sub(self.first_rc) + 1
    }
}

/// The `[release]` section: every configuration has one.
pub(crate) fn section<'a>(t: &Tick<'a>) -> &'a Release {
    &t.config.release
}

/// Run the pass. Returns whether it merged a PR (the tick's one merge).
///
/// Every cycle in flight gets its pass (ASDLC-MG26): the newest candidate's,
/// and each patch cycle opened beside it from the release in production
/// (`patches_in_flight`). A patch goes first, since it carries a fix to
/// production; the tick still merges one PR at most, so the first cycle that
/// merged ends it. The cycles never touch each other: each has its own
/// release item, release branch and candidate.
pub fn pass(t: &Tick, log: &mut ActionLog, report: &mut Report) -> bool {
    let release = section(t);
    let shape = Shape::of(&t.config.product.name, release);
    let (candidate, tags) = match newest(t, &shape) {
        Ok(Some(found)) => found,
        Ok(None) => return false,
        Err(e) => {
            t.unreadable(report, "the release candidate", e);
            return false;
        }
    };
    overtaken(t, log, report, &shape, &tags, &candidate);
    let patches = match patches_in_flight(t, &shape, &tags, &candidate) {
        Ok(p) => p,
        Err(e) => {
            // The newest cycle's pass does not wait on the patches' read.
            t.unreadable(report, "the patch cycles", e);
            Vec::new()
        }
    };
    for patch in patches {
        if cycle_pass(t, log, report, &shape, &tags, patch) {
            return true;
        }
    }
    cycle_pass(t, log, report, &shape, &tags, candidate)
}

/// The patch cycles in flight below the newest candidate (ASDLC-MG26):
/// versions above the release in production whose first candidate the cut
/// workflow tagged as a patch (`Release-Patch-From:`), each with its newest
/// candidate, highest version first. A lower version without that line is
/// an older cycle, left to `overtaken`; one at or below production was
/// published or abandoned, never in flight.
pub fn patches_in_flight(
    t: &Tick,
    shape: &Shape,
    tags: &[GitRef],
    newest: &Candidate,
) -> Result<Vec<Candidate>, String> {
    let mut lower: Vec<(Version, &GitRef, u64)> = Vec::new();
    let mut first: Vec<(Version, &GitRef, u64)> = Vec::new();
    for r in tags {
        let Some((v, rc)) = shape.parse_candidate(&r.name) else {
            continue;
        };
        if v >= newest.version {
            continue;
        }
        match lower.iter_mut().find(|(lv, _, _)| *lv == v) {
            Some(l) if rc > l.2 => *l = (v, r, rc),
            Some(_) => {}
            None => lower.push((v, r, rc)),
        }
        match first.iter_mut().find(|(fv, _, _)| *fv == v) {
            Some(f) if rc < f.2 => *f = (v, r, rc),
            Some(_) => {}
            None => first.push((v, r, rc)),
        }
    }
    if lower.is_empty() {
        return Ok(Vec::new());
    }
    let Some((_, production)) = production(t, shape)? else {
        return Ok(Vec::new());
    };
    lower.retain(|(v, _, _)| *v > production);
    lower.sort_by_key(|(v, _, _)| std::cmp::Reverse(*v));
    let mut patches = Vec::new();
    for (version, tag, rc) in lower {
        let Some((_, opening, _)) = first.iter().find(|(v, _, _)| *v == version) else {
            continue;
        };
        if !opens_a_patch(t, opening)? {
            continue;
        }
        patches.push(read_candidate(t, tag, version, rc)?);
    }
    Ok(patches)
}

/// Whether a cycle's first candidate was cut as a patch: its annotation (a
/// real tag object only) carries `Release-Patch-From:`.
fn opens_a_patch(t: &Tick, tag: &GitRef) -> Result<bool, String> {
    Ok(patch_from_tag(t, tag)?.is_some())
}

/// The release a tag's annotation says it patches (`Release-Patch-From:`),
/// a real tag object only.
fn patch_from_tag(t: &Tick, tag: &GitRef) -> Result<Option<String>, String> {
    if tag.object_type != ObjectType::Tag {
        return Ok(None);
    }
    let o = t
        .github
        .tag_object(&t.repo, &tag.sha)
        .map_err(|e| e.to_string())?;
    Ok(gate::patch_from_annotation(Some(&o.message)))
}

/// The release the cycle of `c` patches, when it is a patch cycle: read off
/// its first candidate (`c.first_rc`), which the cut workflow's `patch`
/// action annotated (ASDLC-MG26). `None` for a normal cycle.
pub(crate) fn cycle_patches(
    t: &Tick,
    shape: &Shape,
    c: &Candidate,
) -> Result<Option<String>, String> {
    let first = shape.candidate_tag(c.version, c.first_rc.max(1));
    let refs = t.github.tags(&t.repo, &first).map_err(|e| e.to_string())?;
    match refs.iter().find(|r| r.name == first) {
        Some(r) => patch_from_tag(t, r),
        None => Ok(None),
    }
}

/// The release a published release tag says it patches: the GO copies the
/// patch cycle's `Release-Patch-From:` line into it, which the publication
/// workflow reads to carry the patch into the open cycle, never `main`.
pub(crate) fn release_patches(t: &Tick, tag: &str) -> Result<Option<String>, String> {
    let refs = t.github.tags(&t.repo, tag).map_err(|e| e.to_string())?;
    match refs.iter().find(|r| r.name == tag) {
        Some(r) => patch_from_tag(t, r),
        None => Ok(None),
    }
}

/// The release in production: the highest release tag of the product with a
/// published (not draft) GitHub release.
pub fn production(t: &Tick, shape: &Shape) -> Result<Option<(String, Version)>, String> {
    let tags = t
        .github
        .tags(&t.repo, shape.release_tag_prefix())
        .map_err(|e| e.to_string())?;
    let mut releases: Vec<(String, Version)> = tags
        .iter()
        .filter_map(|r| shape.parse_release(&r.name).map(|v| (r.name.clone(), v)))
        .collect();
    releases.sort_by_key(|a| std::cmp::Reverse(a.1));
    for (tag, version) in releases {
        match t.github.release(&t.repo, &tag) {
            Ok(r) if !r.draft => return Ok(Some((tag, version))),
            Ok(_) => continue,
            Err(e) if e.is_not_found() => continue,
            Err(e) => return Err(format!("the release {tag}: {e}")),
        }
    }
    Ok(None)
}

/// One cycle's pass, for its newest candidate. Returns whether it merged.
fn cycle_pass(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    shape: &Shape,
    tags: &[GitRef],
    mut candidate: Candidate,
) -> bool {
    let cfg = t.config;
    let release = section(t);
    let version = candidate.version;
    let object = format!("release {} {version}", cfg.product.name);
    let release_tag = shape.release_tag(version);
    let published = match t.github.release(&t.repo, &release_tag) {
        Ok(r) if !r.draft => Some(r),
        Ok(_) => None,
        Err(e) if e.is_not_found() => None,
        Err(e) => {
            t.unreadable(report, &object, format!("the release {release_tag}: {e}"));
            return false;
        }
    };
    if published.is_some() {
        // Released without a release item of the bridge's (before it took
        // the product over): not its cycle, nothing to do.
        match find_release_item(t, shape, version) {
            Ok(Some(_)) => {}
            Ok(None) => return false,
            Err(e) => {
                t.unreadable(report, &object, e);
                return false;
            }
        }
    }
    let Some(item) = release_item(t, log, report, shape, version) else {
        return false;
    };
    candidate.first_rc = match cycle_start(t, shape, version, candidate.rc) {
        Ok(first) => first,
        Err(e) => {
            t.unreadable(report, &object, e);
            return false;
        }
    };
    if item.state.is_resolved() {
        // A closed cycle: what its archiving left is retried (BR-ASDLC-019).
        if let Some(published) = &published {
            closure::archive_closed(t, log, report, shape, &candidate, &item, published);
        }
        return false;
    }
    if let Some(published) = published {
        // The release published ends every overall review of the cycle
        // (BR-ASDLC-020); the closure stays the fallback.
        match release_children(t, &item) {
            Ok(children) => end_consolidations(t, log, report, &children, None),
            Err(e) => t.unreadable(report, &item.key, format!("children: {e}")),
        }
        closure::close(t, log, report, shape, &candidate, &item, &published, None);
        return false;
    }
    let branch = match release_branch(t, shape, &candidate) {
        Ok(b) => b,
        Err(e) => {
            t.unreadable(report, &object, e);
            return false;
        }
    };
    let label = vocabulary::release_label(&cfg.product.name, &version.to_string());
    let previous = previous_candidate(shape, tags, &candidate);
    // The range is walked (commit → PR → ticket) once per candidate: until
    // its comment is recorded, which happens only after every embarked
    // ticket carries the label.
    let candidate_marker = texts::marker("candidate", &[("tag", &candidate.tag)]);
    let comments = match t.tracker.comments(&item.key) {
        Ok(c) => c,
        Err(e) => {
            t.unreadable(report, &item.key, format!("comments: {e}"));
            return false;
        }
    };
    let said = |marker: &str| {
        comments
            .iter()
            .any(|c| c.by_bridge && texts::has_marker(&c.body, marker))
    };
    let recorded = said(&candidate_marker);
    // A candidate recorded while no QA angle was configured keeps none:
    // angles configured later apply from the next candidate.
    let without_angles =
        release.angles.is_empty() || said(&texts::no_qa_angle_marker(&candidate.tag));
    if recorded || label_embarked(t, log, report, shape, &candidate, &label) {
        record_candidate(t, log, report, &item, &candidate, previous.as_deref());
    }
    // The unified release note's tasks, from the first candidate on: each
    // blocks the release item, so the GO waits for them; tasks not in place
    // with their relations hold the GO this tick, fail-closed.
    let notes_ready = release.note_writer.is_none()
        || match release_children(t, &item) {
            Ok(children) => {
                super::release_note::ensure(t, log, report, &item, &children, &candidate, &branch)
            }
            Err(e) => {
                t.unreadable(report, &item.key, format!("children: {e}"));
                false
            }
        };
    if !t.in_time() {
        report.out_of_time = true;
        return false;
    }
    let offered = if without_angles {
        Vec::new()
    } else {
        offered(t, &candidate)
    };
    let gate = match gate_run(t, &candidate) {
        Ok(run) => run,
        Err(e) => {
            t.unreadable(
                report,
                &item.key,
                format!("the gate of {}: {e}", candidate.tag),
            );
            return false;
        }
    };
    // A red gate is never a silence (BR-ASDLC-016): its gate-fix ticket,
    // once the automatic rerun settled; a green one closes those it made moot.
    gate_fix(
        t,
        log,
        report,
        shape,
        &item,
        &candidate,
        &branch,
        &label,
        gate.as_ref(),
    );
    // The round opens on a green gate only (BR-ASDLC-004); until then the
    // children are read, never created.
    let round = qa_round(
        t,
        report,
        shape,
        &item,
        &candidate,
        previous.as_deref(),
        gate,
        without_angles,
    );
    let children = match round {
        Round::Wait => match release_children(t, &item) {
            Ok(c) => c,
            Err(e) => {
                t.unreadable(report, &item.key, format!("children: {e}"));
                Vec::new()
            }
        },
        Round::Open => open_round(t, log, report, &item, &candidate, &branch, &label, &offered)
            .map(|(children, _)| children)
            .unwrap_or_default(),
        Round::Brief(b) => {
            // The round first, its relations, the brief last (REQ-REL-046).
            // A relation not written holds the brief, fail-closed: the brief
            // is what starts the angles' work, and a consolidation item not
            // yet blocked by them could be served first. The next tick lays
            // the relations, then posts the brief.
            match open_round(t, log, report, &item, &candidate, &branch, &label, &offered) {
                Some((children, true)) => {
                    post_brief(t, log, report, &item, &candidate, &b);
                    note_into_item(t, log, report, shape, &item, &candidate);
                    children
                }
                Some((children, false)) => children,
                None => Vec::new(),
            }
        }
    };
    // A newer candidate tagged ends the overall review of the older ones.
    end_consolidations(t, log, report, &children, Some(&candidate.tag));

    let mut merged = false;
    // A draft angle PR is out of the flow, as any draft (`docs/method.md`):
    // it drives nothing, is never merged, and counts for nothing in the GO.
    let angle_prs: Vec<PullRequest> = match t.github.open_pulls(&t.repo, Some(&branch)) {
        Ok(p) => p.into_iter().filter(|p| !p.draft).collect(),
        Err(e) => {
            t.unreadable(report, &object, format!("the PRs of {branch}: {e}"));
            return false;
        }
    };
    let mut states: Vec<(String, AngleState)> = offered
        .iter()
        .map(|a| {
            let state = if children
                .iter()
                .any(|c| angle_of(c, &candidate.tag).as_deref() == Some(a.as_str()))
            {
                AngleState::Offered
            } else {
                AngleState::NotOffered
            };
            (a.clone(), state)
        })
        .collect();
    for pr in &angle_prs {
        let Some((cand, angle)) = prs::parse_angle_branch(&pr.head_ref) else {
            continue;
        };
        if cand != candidate.tag || angle == verdict::KEY {
            continue;
        }
        // An angle PR from a fork is out of the flow: anyone can name a
        // fork's branch `qa/<candidate>/<angle>`, so it is never merged,
        // drives no sub-item, counts for nothing in the GO and is told
        // nothing (not even which sub-item to name).
        if pr.cross_repository {
            continue;
        }
        let state = angle_pr(
            t,
            log,
            report,
            pr,
            &candidate,
            &branch,
            &angle,
            &children,
            &mut merged,
        );
        if let Some(s) = states.iter_mut().find(|(a, _)| *a == angle) {
            s.1 = state;
        }
    }
    let merged_pulls = match t.github.merged_pulls(&t.repo, Some(&branch), None, 500) {
        Ok(m) => m,
        Err(e) => {
            t.unreadable(
                report,
                &item.key,
                format!("the merged PRs of {branch}: {e}"),
            );
            return merged;
        }
    };
    let reports = merged_angles(
        t,
        log,
        report,
        &item,
        &candidate,
        &children,
        &mut states,
        &merged_pulls,
    );
    // The coordinator's verdict, for a candidate that offered its angles;
    // a later pass under the lighter policy counts the first pass's angles
    // and asks no verdict of its own.
    let verdict = if without_angles || lighter_pass(release, &candidate) {
        VerdictState::NotNeeded
    } else {
        candidate_verdict(
            t,
            log,
            report,
            &item,
            &candidate,
            &branch,
            &label,
            &children,
            &states,
            reports.as_deref(),
            &angle_prs,
            &merged_pulls,
            &mut merged,
        )
    };

    if merged {
        return true;
    }
    // A later pass under the lighter policy offers no angle: its GO counts
    // the angle PRs merged in the first pass (spec 05 §7-8).
    let (states, without_angles) = if !without_angles && lighter_pass(release, &candidate) {
        match first_pass_angles(t, shape, &item, &candidate, &branch, &angle_prs) {
            Ok(x) => x,
            Err(e) => {
                t.unreadable(report, &item.key, e);
                return false;
            }
        }
    } else {
        (states, without_angles)
    };
    let Some(decision) = decide(
        t,
        report,
        &item,
        &candidate,
        &branch,
        &states,
        without_angles,
        verdict,
    ) else {
        return false;
    };
    if decision.go && notes_ready {
        on_go(t, log, report, shape, &item, &candidate, &branch);
        return false;
    }
    hold(t, log, report, &item, &candidate, &decision);
    escalate(t, log, report, &item, &candidate, &decision, &branch);
    stagnation(t, log, report, &item, &candidate, &decision);
    // With `auto_candidate`, the next candidate once the fixes are merged.
    super::auto_candidate::cut(
        t, log, report, &item, &candidate, &branch, &label, &decision,
    );
    false
}

/// The cycle just below the newest candidate's version, when the next cycle
/// was cut before its closure ended (its note's channel failing, say): its
/// closure goes on beside the next cycle until its release item is Done,
/// then only what its archiving left is retried, as for the newest cycle.
/// One cycle back is enough: a cycle is cut after the one before it was
/// published and merged back.
fn overtaken(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    shape: &Shape,
    tags: &[GitRef],
    newest: &Candidate,
) {
    let Some((tag, version, rc)) = tags
        .iter()
        .filter_map(|r| shape.parse_candidate(&r.name).map(|(v, rc)| (r, v, rc)))
        .filter(|(_, v, _)| *v < newest.version)
        .max_by_key(|(_, v, rc)| (*v, *rc))
    else {
        return;
    };
    let object = format!("release {} {version}", shape.product);
    let release_tag = shape.release_tag(version);
    let published = match t.github.release(&t.repo, &release_tag) {
        Ok(r) if !r.draft => r,
        Ok(_) => return,
        Err(e) if e.is_not_found() => return,
        Err(e) => {
            t.unreadable(report, &object, format!("the release {release_tag}: {e}"));
            return;
        }
    };
    // Only a cycle of the bridge's, never one it did not open (§14).
    let item = match find_release_item(t, shape, version) {
        Ok(Some(i)) => i,
        Ok(None) => return,
        Err(e) => {
            t.unreadable(report, &object, e);
            return;
        }
    };
    if item.state.is_resolved() && (item.archived || !t.tracker.archives()) {
        return;
    }
    let candidate = match read_candidate(t, tag, version, rc) {
        Ok(c) => c,
        Err(e) => {
            t.unreadable(report, &object, e);
            return;
        }
    };
    if item.state.is_resolved() {
        closure::archive_closed(t, log, report, shape, &candidate, &item, &published);
        return;
    }
    match release_children(t, &item) {
        Ok(children) => end_consolidations(t, log, report, &children, None),
        Err(e) => t.unreadable(report, &item.key, format!("children: {e}")),
    }
    closure::close(
        t,
        log,
        report,
        shape,
        &candidate,
        &item,
        &published,
        Some(newest),
    );
}

/// The newest candidate of the product and the tags it was read from.
pub fn newest(t: &Tick, shape: &Shape) -> Result<Option<(Candidate, Vec<GitRef>)>, String> {
    let tags = t
        .github
        .tags(&t.repo, shape.candidate_tag_prefix())
        .map_err(|e| e.to_string())?;
    let Some((tag, version, rc)) = shape.newest_candidate(tags.iter().map(|r| r.name.as_str()))
    else {
        return Ok(None);
    };
    let tag_ref = tags
        .iter()
        .find(|r| r.name == tag)
        .ok_or("the candidate tag vanished")?;
    let candidate = read_candidate(t, tag_ref, version, rc)?;
    Ok(Some((candidate, tags)))
}

fn read_candidate(t: &Tick, tag: &GitRef, version: Version, rc: u64) -> Result<Candidate, String> {
    let (sha, message, tagged_at) = match tag.object_type {
        ObjectType::Tag => {
            let o = t
                .github
                .tag_object(&t.repo, &tag.sha)
                .map_err(|e| e.to_string())?;
            (o.target_sha, Some(o.message), o.tagger_date)
        }
        _ => {
            let c = t
                .github
                .commit(&t.repo, &tag.sha)
                .map_err(|e| e.to_string())?;
            (c.sha, None, c.committer_date)
        }
    };
    Ok(Candidate {
        tag: tag.name.clone(),
        version,
        rc,
        sha,
        class: gate::class_of_annotation(message.as_deref())
            .name()
            .to_string(),
        tagged_at,
        first_rc: 1,
    })
}

/// The rc of the first candidate of `version`'s cycle in flight, below or at
/// `rc`: 1, or, when cycles of the version were abandoned, the candidate
/// after the last one their release items recorded (spec 05 §13: the next
/// cut starts a new cycle, whose passes count from its own first candidate).
pub fn cycle_start(t: &Tick, shape: &Shape, version: Version, rc: u64) -> Result<u64, String> {
    let v = version.to_string();
    let label = vocabulary::release_label(&shape.product, &v);
    let want = BridgeItem::Release {
        product: shape.product.clone(),
        version: vocabulary::normalise_version(&v),
    };
    let canceled: Vec<Item> = t
        .tracker
        .list_by_label(&label)
        .map_err(|e| format!("the items labelled {label}: {e}"))?
        .into_iter()
        .filter(|i| i.state.status == Some(Status::Canceled))
        .filter(|i| texts::bridge_item(&i.description, i.parent.as_deref()).as_ref() == Some(&want))
        .collect();
    let mut last = 0;
    for item in &canceled {
        let comments = t
            .tracker
            .comments(&item.key)
            .map_err(|e| format!("the comments of {}: {e}", item.key))?;
        if !comments
            .iter()
            .any(|c| c.by_bridge && texts::records_abandon(&c.body))
        {
            continue;
        }
        for n in (last + 1)..rc {
            let marker = texts::marker("candidate", &[("tag", &shape.candidate_tag(version, n))]);
            if comments
                .iter()
                .any(|c| c.by_bridge && texts::has_marker(&c.body, &marker))
            {
                last = n;
            }
        }
    }
    Ok(last + 1)
}

/// The candidate before `c` in its cycle; none for a cycle's first.
fn previous_candidate(shape: &Shape, tags: &[GitRef], c: &Candidate) -> Option<String> {
    tags.iter()
        .filter_map(|r| {
            shape
                .parse_candidate(&r.name)
                .map(|(v, rc)| (r.name.clone(), v, rc))
        })
        .filter(|(_, v, rc)| *v == c.version && *rc < c.rc && *rc >= c.first_rc)
        .max_by_key(|(_, _, rc)| *rc)
        .map(|(n, _, _)| n)
}

/// The release branch that holds the candidate: exactly one, or unreadable.
pub fn release_branch(t: &Tick, shape: &Shape, c: &Candidate) -> Result<String, String> {
    let branches = t
        .github
        .branches(&t.repo, &shape.branch_prefix)
        .map_err(|e| e.to_string())?;
    let mut holding = Vec::new();
    for b in branches.iter().filter(|b| shape.is_release_branch(&b.name)) {
        let status = t
            .github
            .compare_status(&t.repo, &c.sha, &b.name)
            .map_err(|e| e.to_string())?;
        if matches!(status, CompareStatus::Ahead | CompareStatus::Identical) {
            holding.push(b.name.clone());
        }
    }
    match holding.len() {
        1 => Ok(holding.remove(0)),
        0 => Err(format!("no release branch holds {}", c.tag)),
        _ => Err(format!(
            "several release branches hold {}: {}",
            c.tag,
            holding.join(", ")
        )),
    }
}

/// The release item: an item with the release label whose marker names this
/// product and version; created when there is none (spec 05 §2).
fn release_item(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    shape: &Shape,
    version: Version,
) -> Option<Item> {
    let product = &shape.product;
    let v = version.to_string();
    let label = vocabulary::release_label(product, &v);
    let object = format!("release {product} {v}");
    let want = BridgeItem::Release {
        product: product.clone(),
        version: vocabulary::normalise_version(&v),
    };
    let found: Vec<Item> = match t.tracker.list_by_label(&label) {
        Ok(items) => {
            let mut found = Vec::new();
            for item in items {
                match cycle_was_abandoned(t, &item) {
                    Ok(false) => found.push(item),
                    Ok(true) => {}
                    Err(e) => {
                        t.unreadable(report, &object, e);
                        return None;
                    }
                }
            }
            found
        }
        Err(e) => {
            t.unreadable(report, &object, format!("the items labelled {label}: {e}"));
            return None;
        }
    };
    let marker = texts::marker(
        "release",
        &[
            ("product", product),
            ("version", &vocabulary::normalise_version(&v)),
        ],
    );
    let mut mine = match identity::find(t, log, report, &found, &marker, |i| {
        texts::bridge_item(&i.description, i.parent.as_deref()).as_ref() == Some(&want)
    }) {
        Ok(m) => m,
        Err(e) => {
            t.unreadable(report, &object, e);
            return None;
        }
    };
    match mine.len() {
        1 => return Some(in_progress(t, log, report, mine.remove(0))),
        0 => {}
        _ => {
            t.unreadable(
                report,
                &object,
                format!(
                    "several release items: {}",
                    mine.iter()
                        .map(|i| i.key.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            );
            return None;
        }
    }
    let release = section(t);
    let angles: Vec<AngleLine> = release
        .angles
        .iter()
        .map(|a| AngleLine {
            key: a.key.clone(),
            title: a.title.clone(),
            assignee: a.assignee.clone(),
        })
        .collect();
    let description = for_tracker(t, &texts::release_description(product, &v, &label, &angles));
    t.act(
        log,
        report,
        &object,
        &format!("ensure label {label}"),
        json!({}),
        || t.tracker.ensure_label(&label).map_err(|e| e.to_string()),
    )?;
    // The owner decides the GO: the item is theirs, so that no agent that
    // takes unassigned items takes it; unassigned when the tracker does not
    // know them (`doctor` reports it).
    let assignee = t.tracker.user(&t.config.owner.tracker_user).ok();
    identity::create(
        t,
        log,
        report,
        &object,
        "create release item",
        json!({"label": label, "assignee": assignee.as_ref().map(|u| u.name.as_str())}),
        &NewItem {
            title: texts::release_title(product, &v),
            description,
            labels: vec![label.clone()],
            // Its cycle runs from its creation to its closure, which moves
            // it to Done.
            status: Status::InProgress,
            priority: Some(Priority::Urgent),
            assignee,
            parent: None,
            kind: ItemKind::Ticket,
        },
    )
}

/// The release item In Progress while its cycle runs: one an earlier bridge
/// created in Ready (or a person put back in Backlog) is moved, once, with
/// its assignee kept.
fn in_progress(t: &Tick, log: &mut ActionLog, report: &mut Report, mut item: Item) -> Item {
    if !matches!(item.state.status, Some(Status::Ready | Status::Backlog)) {
        return item;
    }
    let moved = t.act(
        log,
        report,
        &item.key,
        "status In Progress (release cycle)",
        json!({}),
        || {
            t.tracker
                .set_status(&item.key, Status::InProgress, None)
                .map_err(|e| e.to_string())
        },
    );
    if moved.is_some() {
        item.state = vocabulary::ItemState::of(Status::InProgress);
    }
    item
}

/// The release item of a version: an item with the release label whose
/// marker names this product and version. Several is an error.
pub fn find_release_item(
    t: &Tick,
    shape: &Shape,
    version: Version,
) -> Result<Option<Item>, String> {
    let v = version.to_string();
    let label = vocabulary::release_label(&shape.product, &v);
    let want = BridgeItem::Release {
        product: shape.product.clone(),
        version: vocabulary::normalise_version(&v),
    };
    let candidates: Vec<Item> = t
        .tracker
        .list_by_label(&label)
        .map_err(|e| format!("the items labelled {label}: {e}"))?
        .into_iter()
        .filter(|i| texts::bridge_item(&i.description, i.parent.as_deref()).as_ref() == Some(&want))
        .collect();
    let mut mine = Vec::new();
    for item in candidates {
        if !cycle_was_abandoned(t, &item)? {
            mine.push(item);
        }
    }
    match mine.len() {
        0 => Ok(None),
        1 => Ok(Some(mine.remove(0))),
        _ => Err(format!(
            "several release items: {}",
            mine.iter()
                .map(|i| i.key.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// A canceled release item starts a new cycle only when the bridge's
/// `release abandon` verb recorded that gesture. A tracker-only status
/// change must not discard the cycle's blockers or reopen its candidate.
fn cycle_was_abandoned(t: &Tick, item: &Item) -> Result<bool, String> {
    if item.state.status != Some(Status::Canceled) {
        return Ok(false);
    }
    Ok(t.tracker
        .comments(&item.key)
        .map_err(|e| format!("the comments of {}: {e}", item.key))?
        .iter()
        .any(|c| c.by_bridge && texts::records_abandon(&c.body)))
}

/// The candidate's comment, once, and its line in `## Candidates`.
fn record_candidate(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    item: &Item,
    c: &Candidate,
    previous: Option<&str>,
) {
    let marker = texts::marker("candidate", &[("tag", &c.tag)]);
    match tracker_said(t, &item.key, &marker) {
        Ok(true) => {}
        Ok(false) => {
            let (paths, paths_cut) = match previous.map(|p| t.github.compare(&t.repo, p, &c.sha)) {
                None => (Vec::new(), false),
                Some(Ok(cmp)) => (cmp.files, cmp.files_cut),
                Some(Err(e)) => {
                    // Never a comment with a range that was not read: it would
                    // be said once, and wrong.
                    t.unreadable(
                        report,
                        &item.key,
                        format!("the range since {}: {e}", previous.unwrap_or_default()),
                    );
                    return;
                }
            };
            let r = section(t);
            let scope = if r.angles.is_empty() {
                AngleScope::NoQaAngle
            } else if lighter_pass(r, c) {
                AngleScope::LighterPass
            } else {
                AngleScope::Configured
            };
            let offered = offered(t, c);
            let body = for_tracker(
                t,
                &texts::candidate_comment(
                    &c.tag, &c.class, previous, None, &paths, paths_cut, &offered, scope,
                ),
            );
            t.act(
                log,
                report,
                &item.key,
                "comment candidate",
                json!({"candidate": c.tag}),
                || {
                    t.tracker
                        .comment(&item.key, &body)
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                },
            );
        }
        Err(e) => t.unreadable(report, &item.key, format!("comments: {e}")),
    }
    let line = texts::candidate_log_line(&c.tag, &c.class, previous);
    let description = texts::append_candidate(&item.description, &c.tag, &line);
    if description != item.description {
        t.act(
            log,
            report,
            &item.key,
            "describe candidates",
            json!({"candidate": c.tag}),
            || {
                t.tracker
                    .update_description(&item.key, &description)
                    .map_err(|e| e.to_string())
            },
        );
    }
}

/// Whether the candidate has no QA angle: none is configured
/// (`release.angles = []`), or none was when the candidate was recorded
/// (angles configured later apply from the next candidate).
pub fn without_angles(t: &Tick, item: &Item, c: &Candidate) -> Result<bool, String> {
    Ok(section(t).angles.is_empty()
        || tracker_said(t, &item.key, &texts::no_qa_angle_marker(&c.tag))?)
}

/// Whether the candidate is a later pass under the lighter policy
/// (`later_passes = "gate-and-coordinator"`): it offers no angle, and its GO
/// counts the angles of the first pass (spec 05 §7-8).
pub fn lighter_pass(release: &Release, c: &Candidate) -> bool {
    c.pass() > 1 && release.later_passes == LaterPasses::GateAndCoordinator
}

/// The angles offered on this candidate (spec 05 §7, `later_passes`).
pub fn offered(t: &Tick, c: &Candidate) -> Vec<String> {
    let release = section(t);
    if lighter_pass(release, c) {
        return Vec::new();
    }
    release.angles.iter().map(|a| a.key.clone()).collect()
}

/// The angles a later pass under the lighter policy answers with (spec 05
/// §7-8, REQ-REL-054): those of the cycle's first candidate, each merged
/// once its angle PR was merged into the release branch, in review while
/// that PR is open, missing otherwise. A first candidate recorded with no
/// QA angle leaves none to count: the second value says so, and the GO then
/// rests on the gate and the conditions that involve no angle.
pub fn first_pass_angles(
    t: &Tick,
    shape: &Shape,
    item: &Item,
    c: &Candidate,
    branch: &str,
    open: &[PullRequest],
) -> Result<(Vec<(String, AngleState)>, bool), String> {
    let release = section(t);
    let first = shape.candidate_tag(c.version, c.first_rc);
    if tracker_said(t, &item.key, &texts::no_qa_angle_marker(&first))? {
        return Ok((Vec::new(), true));
    }
    let merged = t
        .github
        .merged_pulls(&t.repo, Some(branch), None, 500)
        .map_err(|e| format!("the merged PRs of {branch}: {e}"))?;
    let of = |p: &PullRequest, angle: &str| {
        prs::parse_angle_branch(&p.head_ref) == Some((first.clone(), angle.to_string()))
    };
    let states = release
        .angles
        .iter()
        .map(|a| {
            let state = if merged.iter().any(|p| of(p, &a.key)) {
                AngleState::Merged
            } else if open.iter().any(|p| of(p, &a.key)) {
                AngleState::InReview
            } else {
                AngleState::NotOffered
            };
            (a.key.clone(), state)
        })
        .collect();
    Ok((states, false))
}

/// The commits of the range, each to its merged PR, each PR to its ticket;
/// the tickets of the team that are past the merge get the release label
/// (spec 05 §2, the brief: "found by commit → PR → linked ticket").
pub fn embarked(t: &Tick, shape: &Shape, c: &Candidate) -> Result<Vec<(String, Vec<u64>)>, String> {
    Ok(embarked_items(t, shape, c)?.tickets)
}

/// What a candidate embarks: its tickets, each with its PRs in the range,
/// and apart, the bridge's own sub-items linked to those PRs (the angle and
/// consolidation items of their review rounds), which are not tickets: never
/// labelled, never in the brief or the note, never judged (spec 05 §11).
#[derive(Debug, Default)]
pub struct Embarked {
    pub tickets: Vec<(String, Vec<u64>)>,
    pub sub_items: Vec<String>,
    /// The tickets with nothing to deploy whose PRs are in the range, with
    /// those PRs: their merge delivered them, so no release embarks them,
    /// but the note announces them and the closure archives them
    /// (BR-ASDLC-019).
    pub delivered: Vec<(String, Vec<u64>)>,
    /// The PRs of the range that no ticket of the team describes, the
    /// release's own bookkeeping set apart: the note and the brief announce
    /// them under "No linked ticket".
    pub untracked: Vec<note::Untracked>,
}

/// Whether a merged PR is the release's own bookkeeping by its branch, never
/// announced whatever it links: a merge back of a release branch or the
/// resolution of its conflict, an angle report PR of a release QA.
fn release_bookkeeping(pr: &crate::github::CommitPull, shape: &Shape) -> bool {
    let head = pr.head_ref.as_str();
    head.starts_with(&shape.branch_prefix)
        || head.starts_with(review::MERGE_BACK_RESOLUTION_PREFIX)
        || head.starts_with(review::ANGLE_BRANCH_PREFIX)
}

/// Whether a merged PR's title marks it `chore(release)`: bookkeeping only
/// when it describes no ticket of the team; one that does embarks it like
/// any other PR (ASDLC-F37V).
fn release_chore_title(pr: &crate::github::CommitPull) -> bool {
    pr.title.trim_start().starts_with("chore(release)")
}

/// Add a PR to an entry of `(key, PRs)`, once.
fn add_pr(to: &mut Vec<(String, Vec<u64>)>, key: &str, pr: u64) {
    match to.iter_mut().find(|(k, _)| k == key) {
        Some((_, prs)) if prs.contains(&pr) => {}
        Some((_, prs)) => prs.push(pr),
        None => to.push((key.to_string(), vec![pr])),
    }
}

/// The candidate's range (REQ-REL-018), with the release it starts from:
/// the highest published release below the candidate's version, or, for the
/// product's first release (`None`), the repository's root, so that the
/// whole history is the range. One definition for the brief, the embarked
/// tickets and the closure; unreadable is an error, never
/// an empty range.
pub fn release_range(
    t: &Tick,
    shape: &Shape,
    c: &Candidate,
) -> Result<(Option<String>, Comparison), String> {
    let tags = t
        .github
        .tags(&t.repo, shape.release_tag_prefix())
        .map_err(|e| e.to_string())?;
    match shape.previous_release(tags.iter().map(|r| r.name.as_str()), Some(c.version)) {
        Some((tag, _)) => t
            .github
            .compare(&t.repo, &tag, &c.sha)
            .map(|cmp| (Some(tag), cmp))
            .map_err(|e| e.to_string()),
        None => t
            .github
            .since_root(&t.repo, &c.sha)
            .map(|cmp| (None, cmp))
            .map_err(|e| e.to_string()),
    }
}

pub fn embarked_items(t: &Tick, shape: &Shape, c: &Candidate) -> Result<Embarked, String> {
    let commits = release_range(t, shape, c)?.1.commits;
    let team = t.tracker.team_key().map_err(|e| e.to_string())?;
    let mut keys = t.config.tracker.team_keys.clone();
    if keys.is_empty() {
        keys.push(team);
    }
    let mut out = Embarked::default();
    for commit in commits {
        // A patch's commit is a cherry-pick of a PR merged on the default
        // branch: the PR is found through the commit it copied (ASDLC-MG26).
        let sha = commit.picked_from.as_deref().unwrap_or(&commit.sha);
        let pulls = t
            .github
            .commit_pulls(&t.repo, sha)
            .map_err(|e| e.to_string())?;
        let Some(pr) = pulls
            .iter()
            .find(|p| p.merge_commit_sha.as_deref() == Some(sha))
        else {
            continue;
        };
        if release_bookkeeping(pr, shape) {
            continue;
        }
        let linked: Vec<String> = t
            .tracker
            .items_linked_to_pr(&pr.url)
            .map_err(|e| e.to_string())?
            .into_iter()
            .filter(|key| keys.iter().any(|k| key.starts_with(&format!("{k}-"))))
            .collect();
        // The PR's review round items are linked to it beside its ticket:
        // recognised as in the PR pass (by marker, or by a parent linked to
        // the same PR), they are set apart.
        let split = super::link::split_linked(t.tracker, linked).map_err(|e| e.to_string())?;
        // A `chore(release)` PR no ticket describes is the release's own
        // bookkeeping: neither announced nor its round's items archived.
        if split.tickets.is_empty() && release_chore_title(pr) {
            continue;
        }
        for sub in split.sub_items {
            if !out.sub_items.contains(&sub.key) {
                out.sub_items.push(sub.key);
            }
        }
        if split.tickets.is_empty() {
            if !out.untracked.iter().any(|u| u.number == pr.number) {
                out.untracked.push(note::Untracked {
                    number: pr.number,
                    title: pr.title.clone(),
                    body: pr.body.clone(),
                });
            }
            continue;
        }
        for ticket in split.tickets {
            // Nothing to deploy: its merge delivered it, no release embarks
            // it (spec 04 §10).
            if super::merge::nothing_to_deploy(t, &ticket) {
                add_pr(&mut out.delivered, &ticket.key, pr.number);
            } else {
                add_pr(&mut out.tickets, &ticket.key, pr.number);
            }
        }
    }
    Ok(out)
}

/// Label every ticket the candidate embarks; whether all of them carry the
/// label now (anything unread or refused is retried at the next tick).
fn label_embarked(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    shape: &Shape,
    c: &Candidate,
    label: &str,
) -> bool {
    let tickets = match embarked(t, shape, c) {
        Ok(k) => k,
        Err(e) => {
            t.unreadable(report, &c.tag, format!("the embarked tickets: {e}"));
            return false;
        }
    };
    let mut all = true;
    for (key, _) in tickets {
        if !t.in_time() {
            report.out_of_time = true;
            return false;
        }
        let item = match t.tracker.get(&key) {
            Ok(i) => i,
            Err(e) => {
                t.unreadable(report, &key, e);
                all = false;
                continue;
            }
        };
        // Its PR is merged into the candidate: it ships, whatever its status
        // says yet (a merge outside the bridge is swept later in the tick).
        if item.state.status == Some(Status::Canceled) || item.labels.iter().any(|l| l == label) {
            continue;
        }
        all &= t
            .act(
                log,
                report,
                &key,
                &format!("label {label}"),
                json!({"candidate": c.tag}),
                || t.tracker.add_label(&key, label).map_err(|e| e.to_string()),
            )
            .is_some();
    }
    all
}

/// What the gate of the candidate allows of its QA round.
enum Round {
    /// The gate is red, pending or never ran, or the brief's facts could not
    /// be read: nothing of the round is created, no agent starts.
    Wait,
    /// The gate is green and the brief already posted: the round is
    /// completed when a creation failed at an earlier tick.
    Open,
    /// The gate is green and the brief, its facts read, is to be posted
    /// once the round is open.
    Brief(Box<texts::Brief>),
}

/// The angle sub-items, the consolidation item and the brief come only once
/// the candidate's own gate run is green (REQ-REL-046/048, BR-ASDLC-004).
/// The brief's facts are read before anything is created: facts that cannot
/// be read open nothing, so that no agent starts without its brief.
#[allow(clippy::too_many_arguments)]
fn qa_round(
    t: &Tick,
    report: &mut Report,
    shape: &Shape,
    item: &Item,
    c: &Candidate,
    previous: Option<&str>,
    gate: Option<crate::github::WorkflowRun>,
    without_angles: bool,
) -> Round {
    let Some(run) = gate.filter(|r| gate::is_green(Some(r))) else {
        return Round::Wait;
    };
    let marker = texts::marker("brief", &[("candidate", &c.tag)]);
    match tracker_said(t, &item.key, &marker) {
        Ok(true) => return Round::Open,
        Ok(false) => {}
        Err(e) => {
            t.unreadable(report, &item.key, format!("comments: {e}"));
            return Round::Wait;
        }
    }
    match brief_facts(t, shape, c, previous, &run) {
        Ok(b) => Round::Brief(Box::new(texts::Brief {
            no_qa_angle: without_angles,
            ..b
        })),
        Err(e) => {
            t.unreadable(report, &item.key, format!("the brief of {}: {e}", c.tag));
            Round::Wait
        }
    }
}

/// The angle sub-items and the consolidation item of the candidate,
/// created when missing (spec 05 §3, §4; a child found by its marker or by
/// its exact title is not missing), the consolidation item blocked by
/// every angle sub-item of the candidate (BR-ASDLC-020; written right after
/// the items, repaired at every later tick; one that fails is reported and
/// retried). Returns the release item's children after the creations, with
/// `false` when a relation could not be read or written (the brief then
/// waits), or `None` when the children could not be read.
#[allow(clippy::too_many_arguments)]
fn open_round(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    item: &Item,
    c: &Candidate,
    branch: &str,
    label: &str,
    offered: &[String],
) -> Option<(Vec<Item>, bool)> {
    let release = section(t);
    let mut children = match release_children(t, item) {
        Ok(c) => c,
        Err(e) => {
            t.unreadable(report, &item.key, format!("children: {e}"));
            return None;
        }
    };
    let product = &t.config.product.name;
    let v = c.version.to_string();
    // The key the candidate's QA items are created under, once one is.
    let mut milestone: Option<String> = None;
    for angle in &release.angles {
        if !offered.contains(&angle.key) {
            continue;
        }
        let line = AngleLine {
            key: angle.key.clone(),
            title: angle.title.clone(),
            assignee: angle.assignee.clone(),
        };
        let title = texts::release_angle_title(product, &v, c.rc, &line);
        // Found by its marker or by its exact title (REQ-REL-048), or by its
        // identity comment: a marker an agent edited does not make the item
        // invisible.
        let marker = texts::marker("angle", &[("scope", &c.tag), ("angle", &angle.key)]);
        match identity::find(t, log, report, &children, &marker, |ch| {
            angle_of(ch, &c.tag).as_deref() == Some(angle.key.as_str()) || ch.title == title
        }) {
            Ok(found) if !found.is_empty() => {
                renew(&mut children, found);
                continue;
            }
            Ok(_) => {}
            Err(e) => {
                t.unreadable(report, &item.key, e);
                return None;
            }
        }
        // The texts of the version the project is pinned on at the
        // candidate, read only for an item to create (BR-ASDLC-032).
        let pinned =
            match method_texts::texts(t, &c.tag, protocol::Job::ReleaseAngle, Some(&angle.key)) {
                Ok(p) => p,
                Err(e) => {
                    let what = format!("the QA items of {}", c.tag);
                    method_texts::unreadable(t, log, report, &item.key, &c.tag, &what, &e);
                    return None;
                }
            };
        let path = report::report_path(&release.record, &c.tag, &angle.key);
        let (_, body) = texts::release_angle_item(
            product,
            &v,
            &c.tag,
            c.rc,
            &line,
            branch,
            &path,
            label,
            angle.notes.as_deref(),
            &pinned,
        );
        let assignee = angle.assignee.as_ref().and_then(|a| t.tracker.user(a).ok());
        let parent = qa_parent(t, log, report, item, c, &mut milestone)?;
        if let Some(created) = create_child(t, log, report, &parent, &title, &body, label, assignee)
        {
            children.push(as_release_child(created, item));
        }
    }
    let title_of = format!("{product} {v} (rc.{})", c.rc);
    let title = texts::consolidation_title(&title_of);
    let marker = texts::marker("consolidation", &[("scope", &c.tag)]);
    let consolidated = match identity::find(t, log, report, &children, &marker, |ch| {
        texts::bridge_item(&ch.description, ch.parent.as_deref())
            == Some(BridgeItem::Consolidation {
                scope: c.tag.clone(),
            })
            || ch.title == title
    }) {
        Ok(found) => {
            let any = !found.is_empty();
            renew(&mut children, found);
            any
        }
        Err(e) => {
            t.unreadable(report, &item.key, e);
            return None;
        }
    };
    if !offered.is_empty() && !consolidated {
        let of = texts::Consolidating::Candidate;
        let pinned = match method_texts::texts(t, &c.tag, of.job(), None) {
            Ok(p) => p,
            Err(e) => {
                let what = format!("the QA items of {}", c.tag);
                method_texts::unreadable(t, log, report, &item.key, &c.tag, &what, &e);
                return None;
            }
        };
        let (_, body) = texts::consolidation_item(&c.tag, &title_of, of, &pinned);
        let assignee = coordinator(t);
        let parent = qa_parent(t, log, report, item, c, &mut milestone)?;
        // Served once every angle report is merged: blocked by the angles'
        // implementation where the tracker keeps such blocks, held in
        // Backlog by the bridge otherwise (BR-ASDLC-020).
        let status = if t.tracker.implementation_blocks() {
            Status::Ready
        } else {
            Status::Backlog
        };
        if let Some(created) = create_item(
            t, log, report, &parent, &title, &body, label, assignee, status,
        ) {
            children.push(as_release_child(created, item));
        }
    }
    let blocked = gate_consolidation(t, log, report, &children, &c.tag, offered);
    Some((children, blocked))
}

/// The release item's children as the method reads them (BR-ASDLC-038): a
/// candidate's QA milestone is the tracker's grouping, not an item of the
/// method's, so the items in it read as the release item's own children
/// and the milestone itself is left out.
pub fn release_children(t: &Tick, item: &Item) -> crate::tracker::TResult<Vec<Item>> {
    let mut out = Vec::new();
    for ch in t.tracker.children(&item.key)? {
        if !is_milestone(&ch) {
            out.push(ch);
            continue;
        }
        for inner in t.tracker.children(&ch.key)? {
            out.push(as_release_child(inner, item));
        }
    }
    Ok(out)
}

/// A candidate's QA milestone: its marker, and no status of the method's
/// (the tracker derives it), so that a ticket carrying a pasted marker is
/// never taken for one.
fn is_milestone(item: &Item) -> bool {
    item.state.status.is_none() && texts::milestone_scope(&item.description).is_some()
}

/// An item of a candidate's milestone, read as the release item's child.
fn as_release_child(mut inner: Item, release: &Item) -> Item {
    inner.parent = Some(release.key.clone());
    inner
}

/// The key the candidate's QA items are created under (BR-ASDLC-038): on a
/// tracker that groups them, the candidate's milestone under the release
/// item, named after the candidate, found by its marker or created once;
/// the release item itself otherwise. `None` when the milestone could not
/// be read or created: nothing is created under the release item instead,
/// and the next tick retries.
fn qa_parent(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    item: &Item,
    c: &Candidate,
    known: &mut Option<String>,
) -> Option<String> {
    if let Some(k) = known {
        return Some(k.clone());
    }
    if !t.tracker.candidate_milestones() {
        *known = Some(item.key.clone());
        return known.clone();
    }
    let children = match t.tracker.children(&item.key) {
        Ok(ch) => ch,
        Err(e) => {
            t.unreadable(report, &item.key, format!("children: {e}"));
            return None;
        }
    };
    let found = children.into_iter().find(|ch| {
        is_milestone(ch)
            && texts::milestone_scope(&ch.description).as_deref() == Some(c.tag.as_str())
    });
    let key = match found {
        Some(m) => m.key,
        None => {
            let description = for_tracker(t, &texts::milestone_description(&c.tag));
            t.act(
                log,
                report,
                &item.key,
                &format!("create milestone \"{}\"", c.tag),
                json!({"candidate": c.tag}),
                || {
                    t.tracker
                        .create_milestone(&item.key, &c.tag, &description)
                        .map_err(|e| e.to_string())
                },
            )?
            .key
        }
    };
    *known = Some(key.clone());
    Some(key)
}

/// The children as `found` reads them (a marker written back).
fn renew(children: &mut [Item], found: Vec<Item>) {
    for f in found {
        if let Some(ch) = children.iter_mut().find(|ch| ch.key == f.key) {
            *ch = f;
        }
    }
}

/// The candidate's consolidation item, while open, is served only once
/// every angle report of the candidate is merged (BR-ASDLC-020): on a
/// tracker that keeps blocks freed at the blocker's merge, it is blocked by
/// every angle sub-item through its implementation; otherwise the bridge
/// holds it in Backlog until every offered angle's sub-item is To Deploy or
/// later, then moves it to Ready (and back, from Ready only, should that
/// stop holding). A block of the release kind an older bridge laid from an
/// angle would hold it until the release: it is removed. A relation or a
/// status already right is not written again; one that could not be read
/// or written is reported and retried next tick, and `false` is returned so
/// that the brief waits for it.
fn gate_consolidation(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    children: &[Item],
    candidate: &str,
    offered: &[String],
) -> bool {
    let want = Some(BridgeItem::Consolidation {
        scope: candidate.to_string(),
    });
    let Some(cons) = children.iter().find(|ch| {
        !ch.state.is_resolved() && texts::bridge_item(&ch.description, ch.parent.as_deref()) == want
    }) else {
        return true;
    };
    let angles: Vec<&Item> = children
        .iter()
        .filter(|ch| angle_of(ch, candidate).is_some())
        .collect();
    let relations = match t.tracker.relations(&cons.key) {
        Ok(r) if r.cut => {
            t.unreadable(
                report,
                &cons.key,
                "the relation list is cut: whether its angles block it cannot be told",
            );
            return false;
        }
        Ok(r) => r,
        Err(e) => {
            t.unreadable(report, &cons.key, format!("relations: {e}"));
            return false;
        }
    };
    let mut all = true;
    for r in &relations.blocked_by {
        if !angles.iter().any(|a| a.key.eq_ignore_ascii_case(&r.other)) {
            continue;
        }
        // A tracker that lists an implementation block among `blocked_by`
        // too (Livela keeps it as a `blocked_by` entry with a kind): that
        // block is the one wanted, never removed, or the next pass would lay
        // it again and the consolidation item be served in between
        // (ASDLC-QFSB).
        if relations
            .blocked_by_implementation
            .iter()
            .any(|i| i.other.eq_ignore_ascii_case(&r.other))
        {
            continue;
        }
        all &= t
            .act(
                log,
                report,
                &cons.key,
                &format!(
                    "remove the release block of {} (an angle frees it at its merge)",
                    r.other
                ),
                json!({"angle": r.other}),
                || t.tracker.unrelate(&r.id).map_err(|e| e.to_string()),
            )
            .is_some();
    }
    let to = if t.tracker.implementation_blocks() {
        for a in &angles {
            if relations
                .blocked_by_implementation
                .iter()
                .any(|r| r.other.eq_ignore_ascii_case(&a.key))
            {
                continue;
            }
            all &= t
                .act(
                    log,
                    report,
                    &a.key,
                    &format!("relate BlocksImplementation {}", cons.key),
                    json!({"consolidation": cons.key}),
                    || {
                        t.tracker
                            .relate(&a.key, &cons.key, RelationKind::BlocksImplementation)
                            .map_err(|e| e.to_string())
                    },
                )
                .is_some();
        }
        // Held in Backlog before the tracker kept such blocks: the blocks
        // hold it now.
        (cons.state.status == Some(Status::Backlog)).then_some(Status::Ready)
    } else {
        let every_merged = offered.iter().all(|k| {
            angles
                .iter()
                .any(|a| angle_of(a, candidate).as_deref() == Some(k.as_str()) && angle_merged(a))
        });
        match cons.state.status {
            Some(Status::Backlog) if every_merged => Some(Status::Ready),
            Some(Status::Ready) if !every_merged => Some(Status::Backlog),
            _ => None,
        }
    };
    if let Some(to) = to {
        let why = if to == Status::Ready {
            "every angle report is merged"
        } else {
            "until every angle report is merged"
        };
        all &= t
            .act(
                log,
                report,
                &cons.key,
                &format!("status {to} ({why})"),
                json!({"candidate": candidate}),
                || {
                    t.tracker
                        .set_status(&cons.key, to, None)
                        .map_err(|e| e.to_string())
                },
            )
            .is_some();
    }
    all
}

/// The QA rounds of the release item ended by a later fact: once a newer
/// candidate is tagged, it redoes every review (BR-ASDLC-038), so every QA
/// item of an older candidate still open (its angle items, its
/// consolidation item) goes Canceled, in the pass that first sees the newer
/// candidate, but an angle item whose report is merged (To Deploy), which
/// goes Done; with no `current`, the release is published and every
/// consolidation item, and every angle item whose report is merged, goes
/// Done (BR-ASDLC-020). The cycle's closure still
/// closes what is left (spec 05 §11).
fn end_consolidations(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    children: &[Item],
    current: Option<&str>,
) {
    for ch in children {
        let (scope, angle) = match texts::bridge_item(&ch.description, ch.parent.as_deref()) {
            Some(BridgeItem::Consolidation { scope }) => (scope, false),
            Some(BridgeItem::Angle { scope, .. }) => (scope, true),
            _ => continue,
        };
        if ch.state.is_resolved() || Some(scope.as_str()) == current {
            continue;
        }
        // An angle whose report is merged did its work: Done. One that is
        // not is canceled by a newer candidate, and left to the closure at
        // the publication.
        let (status, why) = match current {
            _ if angle && angle_merged(ch) => (
                Status::Done,
                "status Done (its report is merged)".to_string(),
            ),
            None if angle => continue,
            Some(newer) => (
                Status::Canceled,
                format!("status Canceled (newer candidate {newer} redoes every review)"),
            ),
            None => (Status::Done, "status Done (release published)".to_string()),
        };
        t.act(
            log,
            report,
            &ch.key,
            &why,
            json!({"candidate": scope}),
            || {
                t.tracker
                    .set_status(&ch.key, status, None)
                    .map_err(|e| e.to_string())
            },
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn create_child(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    parent: &str,
    title: &str,
    body: &str,
    label: &str,
    assignee: Option<crate::tracker::User>,
) -> Option<Item> {
    create_item(
        t,
        log,
        report,
        parent,
        title,
        body,
        label,
        assignee,
        Status::Ready,
    )
}

#[allow(clippy::too_many_arguments)]
fn create_item(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    parent: &str,
    title: &str,
    body: &str,
    label: &str,
    assignee: Option<crate::tracker::User>,
    status: Status,
) -> Option<Item> {
    identity::create(
        t,
        log,
        report,
        parent,
        &format!("create \"{title}\""),
        json!({}),
        &NewItem {
            title: title.to_string(),
            description: for_tracker(t, body),
            labels: vec![label.to_string()],
            status,
            priority: Some(Priority::Urgent),
            assignee,
            parent: Some(parent.to_string()),
            kind: ItemKind::Ticket,
        },
    )
}

/// The angle of a release sub-item of this candidate.
pub fn angle_of(item: &Item, candidate: &str) -> Option<String> {
    match texts::bridge_item(&item.description, item.parent.as_deref())? {
        BridgeItem::Angle { scope, angle } if scope == candidate => Some(angle),
        _ => None,
    }
}

/// The facts of the brief (spec 05, "The brief"), read with the candidate's
/// green gate run. A range, PR, ticket or evidence pack that cannot be read
/// makes the brief wait: it is never guessed.
fn brief_facts(
    t: &Tick,
    shape: &Shape,
    c: &Candidate,
    previous: Option<&str>,
    run: &crate::github::WorkflowRun,
) -> Result<texts::Brief, String> {
    // A first release (no earlier one) is measured from the repository's
    // root: every path at the candidate.
    // A file list GitHub cut at 300 is read all the same (rule G7CK): the
    // brief says so, and the candidate is briefed `sensible` (rule X4PV).
    let (last, cmp) = release_range(t, shape, c)?;
    let paths_since_release = cmp.files;
    let paths_since_release_cut = cmp.files_cut;
    let (paths_since_candidate, paths_since_candidate_cut) = match previous {
        Some(p) => {
            let cmp = t
                .github
                .compare(&t.repo, p, &c.sha)
                .map_err(|e| e.to_string())?;
            (Some(cmp.files), cmp.files_cut)
        }
        None => (None, false),
    };
    let embarked = embarked_items(t, shape, c)?;
    let untracked = embarked
        .untracked
        .iter()
        .map(|u| format!("**PR #{}** — {}", u.number, texts::inert(&u.title)))
        .collect();
    let mut tickets = Vec::new();
    for (key, prs) in embarked.tickets {
        let item = t.tracker.get(&key).map_err(|e| e.to_string())?;
        tickets.push(texts::BriefTicket {
            id: key,
            title: item.title,
            state: item.state.name,
            prs,
            description: Some(item.description),
        });
    }
    let rule_files_touched: Vec<String> = paths_since_release
        .iter()
        .filter(|p| crate::method::business_rules::is_rule_file(p))
        .cloned()
        .collect();
    let evidence_url = evidence_url(t, c, run)?;
    Ok(texts::Brief {
        candidate: c.tag.clone(),
        sha: c.sha.clone(),
        class: c.class.clone(),
        last_release: last,
        previous_candidate: previous.map(str::to_string),
        paths_since_release,
        paths_since_release_cut,
        paths_since_candidate,
        paths_since_candidate_cut,
        gate_url: Some(run.url.clone()),
        evidence_url: Some(evidence_url),
        tickets,
        untracked,
        rule_files_touched,
        no_qa_angle: false,
    })
}

/// The link to the evidence pack of the green gate run (REQ-REL-044): the
/// run's artifacts, where `release-gate-evidence-<sha>` stands unexpired.
fn evidence_url(
    t: &Tick,
    c: &Candidate,
    run: &crate::github::WorkflowRun,
) -> Result<String, String> {
    let name = gate::evidence_artifact(&c.sha);
    let artifacts = t
        .github
        .run_artifacts(&t.repo, run.id)
        .map_err(|e| format!("the artifacts of {}: {e}", run.url))?;
    match artifacts.iter().find(|a| a.name == name) {
        Some(a) if !a.expired => Ok(format!("{}#artifacts", run.url)),
        Some(_) => Err(format!("the evidence pack `{name}` of {} expired", run.url)),
        None => Err(format!("{} carries no evidence pack `{name}`", run.url)),
    }
}

/// The brief, once per candidate, after its round opened (REQ-REL-046).
fn post_brief(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    item: &Item,
    c: &Candidate,
    b: &texts::Brief,
) {
    let body = for_tracker(t, &texts::brief(b));
    t.act(
        log,
        report,
        &item.key,
        "comment brief",
        json!({"candidate": c.tag}),
        || {
            t.tracker
                .comment(&item.key, &body)
                .map(|_| ())
                .map_err(|e| e.to_string())
        },
    );
}

fn gate_run(t: &Tick, c: &Candidate) -> Result<Option<crate::github::WorkflowRun>, String> {
    let runs = t
        .github
        .workflow_runs(&t.repo, &section(t).gate_workflow, None, Some(&c.sha), None)
        .map_err(|e| e.to_string())?;
    Ok(gate::gate_run(&runs, &c.sha, &c.tag).cloned())
}

/// The gate-fix work of the candidate (spec 05 §3, BR-ASDLC-016). A red
/// gate whose automatic rerun settled, or for which no rerun showed up
/// within `gate_rerun_wait_minutes` of its completion, gets one gate-fix
/// ticket: a child of the release item, unassigned, in Ready, urgent, with
/// a complete contract, found again by its marker whatever its state (one
/// per candidate, ever). A green gate closes the gate-fix tickets it made
/// moot. A pending gate, or a red one whose rerun may still come, writes
/// nothing.
#[allow(clippy::too_many_arguments)]
fn gate_fix(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    shape: &Shape,
    item: &Item,
    c: &Candidate,
    branch: &str,
    label: &str,
    gate: Option<&crate::github::WorkflowRun>,
) {
    let Some(first) = gate else {
        return;
    };
    let release = section(t);
    if gate::is_green(Some(first)) {
        close_gate_fixes(t, log, report, shape, item, c, first);
        return;
    }
    let wait = gate::RerunWait {
        now: t.now,
        bound: time::Duration::minutes(i64::from(release.gate_rerun_wait_minutes)),
    };
    if gate::red_gate(first, &[], &wait) == gate::RedGate::NotRed {
        return;
    }
    let children = match release_children(t, item) {
        Ok(ch) => ch,
        Err(e) => {
            t.unreadable(report, &item.key, format!("children: {e}"));
            return;
        }
    };
    let marker = texts::gate_fix_marker(&c.tag);
    let opened = match identity::find(t, log, report, &children, &marker, |ch| {
        texts::has_marker(&ch.description, &marker)
    }) {
        Ok(o) => o,
        Err(e) => {
            t.unreadable(report, &item.key, e);
            return;
        }
    };
    match opened.len() {
        0 => {}
        1 => return,
        _ => {
            let keys: Vec<&str> = opened.iter().map(|i| i.key.as_str()).collect();
            t.unreadable(
                report,
                &item.key,
                format!(
                    "several gate-fix tickets for {}: {}",
                    c.tag,
                    keys.join(", ")
                ),
            );
            return;
        }
    }
    // The rerun workflow first, the gate again after it: a rerun its
    // completed run made is then seen on the gate run. Not filtered by the
    // candidate's commit: GitHub runs a `workflow_run` workflow on the
    // default branch's head, so `red_gate` matches its runs to the gate run
    // by when they were created (SID-1159).
    let rerun_workflow = &release.gate_rerun_workflow;
    let reruns = match t.github.workflow_runs(
        &t.repo,
        rerun_workflow,
        None,
        None,
        Some("workflow_run"),
    ) {
        Ok(r) => r,
        // An absent rerun workflow makes no rerun: said, and the bounded
        // wait decides as for one that never runs.
        Err(e) if e.is_not_found() => {
            t.unreadable(
                report,
                &item.key,
                format!(
                    "the automatic rerun of the gate of {}: {e}; the gate-fix ticket opens {} minutes after the gate run completed",
                    c.tag, release.gate_rerun_wait_minutes
                ),
            );
            Vec::new()
        }
        Err(e) => {
            t.unreadable(
                report,
                &item.key,
                format!("the automatic rerun of the gate of {}: {e}", c.tag),
            );
            return;
        }
    };
    let run = match gate_run(t, c) {
        Ok(Some(r)) => r,
        Ok(None) => return,
        Err(e) => {
            t.unreadable(report, &item.key, format!("the gate of {}: {e}", c.tag));
            return;
        }
    };
    let red = gate::red_gate(&run, &reruns, &wait);
    if !red.is_final() {
        return;
    }
    let unseen = match &red {
        gate::RedGate::Unseen(why) => Some(why.as_str()),
        _ => None,
    };
    let suites = failed_suites_of(t, c, &run);
    let failed_jobs = match &suites {
        Ok(named) if !named.is_empty() => Vec::new(),
        _ => failed_jobs_of(t, &run),
    };
    let facts = texts::RedGateFacts {
        release: &item.key,
        candidate: &c.tag,
        sha: &c.sha,
        branch,
        conclusion: run.conclusion.as_deref().unwrap_or_default(),
        run_url: &run.url,
        suites,
        failed_jobs,
        rerun_unseen: unseen,
    };
    let (title, body) = texts::gate_fix_ticket(&facts);
    let body = for_tracker(t, &body);
    let assignee = fixer(t);
    identity::create(
        t,
        log,
        report,
        &item.key,
        "create gate-fix ticket",
        json!({"candidate": c.tag, "run": run.url, "attempt": run.attempt, "priority": "urgent", "rerun_seen": unseen.is_none(), "assignee": assignee.as_ref().map(|u| u.name.as_str())}),
        &NewItem {
            title,
            description: body,
            labels: vec![label.to_string()],
            status: Status::Ready,
            priority: Some(Priority::Urgent),
            assignee,
            parent: Some(item.key.clone()),
            kind: ItemKind::Ticket,
        },
    );
}

/// The participant `release.fix_assignee` names, who fixes release
/// problems (a red gate, a blocking QA finding), when the tracker knows it;
/// otherwise the ticket is unassigned, taken by role (`doctor` reports an
/// unknown one).
pub(crate) fn fixer(t: &Tick) -> Option<crate::tracker::User> {
    let name = section(t).fix_assignee.as_ref()?;
    t.tracker.user(name).ok()
}

/// The jobs of a red gate run that failed or timed out, for the title of
/// its gate-fix ticket when the evidence pack names no failed suite: none
/// when they cannot be read (the title then keeps its plain wording).
fn failed_jobs_of(t: &Tick, run: &crate::github::WorkflowRun) -> Vec<String> {
    t.github
        .run_jobs(&t.repo, run.id)
        .map(|jobs| {
            jobs.into_iter()
                .filter(|j| matches!(j.conclusion.as_deref(), Some("failure" | "timed_out")))
                .map(|j| j.name)
                .collect()
        })
        .unwrap_or_default()
}

/// The failed suites of a red gate run, read from `results/suites.json` of
/// its evidence pack, or why they could not be (the ticket then says so).
fn failed_suites_of(
    t: &Tick,
    c: &Candidate,
    run: &crate::github::WorkflowRun,
) -> Result<Vec<String>, String> {
    let name = gate::evidence_artifact(&c.sha);
    let artifacts = t
        .github
        .run_artifacts(&t.repo, run.id)
        .map_err(|e| format!("the artifacts of {}: {e}", run.url))?;
    let pack = match artifacts.iter().find(|a| a.name == name) {
        Some(a) if !a.expired => a,
        Some(_) => return Err(format!("the evidence pack `{name}` expired")),
        None => return Err(format!("the run carries no evidence pack `{name}`")),
    };
    let text = t
        .github
        .artifact_file(&t.repo, pack.id, gate::SUITES_FILE)
        .map_err(|e| {
            if e.is_not_found() {
                format!("`{name}` holds no `{}`", gate::SUITES_FILE)
            } else {
                e.to_string()
            }
        })?;
    gate::failed_suites(&text)
}

/// The gate of the newest candidate is green: every gate-fix ticket of this
/// release opened for it or an earlier candidate that never reached a
/// merge, still waiting for an implementer or in their hands (a rerun made
/// the gate green, say), is closed Done with a comment naming the green
/// candidate. Any other is left alone: in review (a PR of it is being
/// judged), past the merge (it follows the flow of any ticket, To Deploy at
/// its fix's merge, Done at the closure) or resolved, and so is one whose
/// state or candidate cannot be read as such (fail-closed).
fn close_gate_fixes(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    shape: &Shape,
    item: &Item,
    c: &Candidate,
    run: &crate::github::WorkflowRun,
) {
    let children = match release_children(t, item) {
        Ok(ch) => ch,
        Err(e) => {
            t.unreadable(report, &item.key, format!("children: {e}"));
            return;
        }
    };
    for ch in &children {
        let Some(cand) = texts::gate_fix_candidate(&ch.description) else {
            continue;
        };
        let open = matches!(
            ch.state.status,
            Some(Status::Triage | Status::Backlog | Status::Ready | Status::InProgress)
        );
        let earlier = shape
            .parse_candidate(&cand)
            .is_some_and(|(v, rc)| v == c.version && rc <= c.rc);
        if ch.archived || !open || !earlier {
            continue;
        }
        let marker = texts::marker(
            "gate-fix-closed",
            &[("candidate", &cand), ("green", &c.tag)],
        );
        match tracker_said(t, &ch.key, &marker) {
            Ok(true) => {}
            Ok(false) => {
                let body = for_tracker(t, &texts::gate_fix_closed_comment(&cand, &c.tag, &run.url));
                let said = t.act(
                    log,
                    report,
                    &ch.key,
                    "comment gate green",
                    json!({"candidate": cand, "green": c.tag}),
                    || {
                        t.tracker
                            .comment(&ch.key, &body)
                            .map(|_| ())
                            .map_err(|e| e.to_string())
                    },
                );
                if said.is_none() {
                    continue;
                }
            }
            Err(e) => {
                t.unreadable(report, &ch.key, format!("comments: {e}"));
                continue;
            }
        }
        t.act(
            log,
            report,
            &ch.key,
            "status Done (gate green)",
            json!({"candidate": cand, "green": c.tag}),
            || {
                t.tracker
                    .set_status(&ch.key, Status::Done, None)
                    .map_err(|e| e.to_string())
            },
        );
    }
}

/// One angle PR of the candidate: its state for the GO, merged into the
/// release branch when every condition holds (spec 05 §4-5).
#[allow(clippy::too_many_arguments)]
fn angle_pr(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    pr: &PullRequest,
    c: &Candidate,
    branch: &str,
    angle: &str,
    children: &[Item],
    merged: &mut bool,
) -> AngleState {
    let object = format!("pr#{}", pr.number);
    let Some(sub) = children
        .iter()
        .find(|ch| angle_of(ch, &c.tag).as_deref() == Some(angle))
    else {
        return AngleState::InReview;
    };
    let release = section(t);
    let required = match t.github.required_checks(&t.repo, branch) {
        Ok(r) => r,
        Err(e) => {
            t.unreadable(report, &object, e);
            return AngleState::InReview;
        }
    };
    let checks = checks::pr_state(pr, &required.contexts);
    let green = checks == CheckState::Green;
    let path = report::report_path(&release.record, &c.tag, angle);
    let valid = match t.github.file(&t.repo, &path, &pr.head_sha) {
        Ok(text) => report::validate(&text, &c.tag, angle).map_err(|e| e.join("; ")),
        Err(e) if e.is_not_found() => Err(format!("no report at `{path}`")),
        Err(e) => {
            t.unreadable(report, &object, e);
            return AngleState::InReview;
        }
    };
    // An angle PR carries its report and nothing else (BR-ASDLC-022): an
    // angle fixes nothing, so a PR with any other file is not merged. The
    // PR's own changes are those since it left the release branch; read
    // only for a valid report of a PR that names its sub-item.
    let others: Vec<String> = if valid.is_ok() && !*merged && prs::names_sub_item(pr, &sub.key) {
        match t.github.compare(&t.repo, branch, &pr.head_sha) {
            Ok(cmp) => cmp.files.into_iter().filter(|f| *f != path).collect(),
            Err(e) => {
                t.unreadable(
                    report,
                    &object,
                    format!("the changes of the angle PR against {branch}: {e}"),
                );
                return AngleState::InReview;
            }
        }
    } else {
        Vec::new()
    };
    // What only the angle can mend sends its item back to work, In Review
    // included, where its agent puts it at the PR's opening (ASDLC-WMTX).
    let fault = checks == CheckState::Red || valid.is_err() || !others.is_empty();
    follow_angle_pr(t, log, report, pr, sub, angle, green, fault);
    if let Err(why) = &valid {
        let marker = texts::marker(
            "report-invalid",
            &[("pr", &pr.url), ("head", texts::short(&pr.head_sha))],
        );
        if let Ok(false) = tracker_said(t, &sub.key, &marker) {
            let body = for_tracker(
                t,
                &format!(
                    "The angle PR {} at `{}` has no valid report, so it is not merged: {why}\n\n{marker}",
                    pr.url,
                    texts::short(&pr.head_sha)
                ),
            );
            t.act(
                log,
                report,
                &sub.key,
                "comment report invalid",
                json!({"pr": pr.url}),
                || {
                    t.tracker
                        .comment(&sub.key, &body)
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                },
            );
        }
        return AngleState::FailedReview;
    }
    if *merged || !prs::names_sub_item(pr, &sub.key) {
        return AngleState::InReview;
    }
    if !others.is_empty() {
        let listed: Vec<String> = others
            .iter()
            .take(10)
            .map(|f| format!("`{}`", texts::inert(f).replace('`', "'")))
            .collect();
        super::merge::say_once(
            t,
            log,
            report,
            pr,
            "angle-not-report-only",
            &[("pr", &pr.url), ("head", texts::short(&pr.head_sha))],
            &format!(
                "This angle PR changes {} file(s) besides its report `{path}`, so the bridge does not \
                 merge it: an angle documents its findings and fixes nothing (`docs/agents/review-release.md`). \
                 Remove the other changes from the branch; a blocking finding is fixed from its own ticket. \
                 Files: {}{}.",
                others.len(),
                listed.join(", "),
                if others.len() > listed.len() {
                    ", …"
                } else {
                    ""
                }
            ),
        );
        return AngleState::InReview;
    }
    // No approval is asked (BR-ASDLC-020): a valid report-only PR merges on
    // the bridge's own checks. A coordinator's request for changes at its
    // head still holds it (its sub-item goes back to its angle), as does a
    // person's standing one; an App's other request for changes is never
    // dismissed without an approval to supersede it, so it holds it too.
    if review::changes_requested_at_head(pr, &release.coordinators).is_some() || !green {
        return AngleState::InReview;
    }
    match super::merge::superseded(pr, &release.coordinators) {
        Ok(d) if d.is_empty() => {}
        Ok(d) => {
            let who: Vec<&str> = d.iter().map(|x| x.login.as_str()).collect();
            t.wait(
                report,
                pr,
                format!("changes requested by {} stand", who.join(", ")),
            );
            return AngleState::InReview;
        }
        Err(why) => {
            t.wait(report, pr, why);
            return AngleState::InReview;
        }
    }
    match review::branch_state(pr) {
        BranchState::Conflicting => return AngleState::InReview,
        BranchState::Behind => {
            // From the release branch only, never from main.
            t.act(
                log,
                report,
                &object,
                "update branch",
                json!({"base": branch}),
                || {
                    t.github
                        .update_branch(&t.repo, pr.number, &pr.head_sha)
                        .map_err(|e| e.to_string())
                },
            );
            return AngleState::InReview;
        }
        BranchState::Current => {}
    }
    let done = t.act(
        log,
        report,
        &object,
        "merge (merge commit) into the release branch",
        json!({"head": pr.head_sha, "angle": angle}),
        || {
            t.github
                .merge_pull(
                    &t.repo,
                    pr.number,
                    &pr.head_sha,
                    crate::github::MergeMethod::Merge,
                )
                .map_err(|e| e.to_string())
        },
    );
    if done.is_none() {
        return AngleState::InReview;
    }
    *merged = true;
    // Its report is merged: To Deploy, as a ticket at its merge; Done when
    // the release is published or a newer candidate is tagged.
    t.act(
        log,
        report,
        &sub.key,
        "status To Deploy (angle merged)",
        json!({"pr": pr.url}),
        || {
            t.tracker
                .set_status(&sub.key, Status::ToDeploy, None)
                .map_err(|e| e.to_string())
        },
    );
    AngleState::Merged
}

/// A release angle sub-item follows its open angle PR as a ticket follows
/// its PR (BR-ASDLC-020): In Progress when the PR is opened, In Review once
/// its agent moves it there or its required checks are green (a draft is
/// out of the flow and never gets here), back In Progress when a
/// configured coordinator requests changes at its head or a fault at its
/// head is the angle's to mend (ASDLC-WMTX); To Deploy comes
/// from the merge. In Review keeps the assignee: it is the bridge's step,
/// served to nobody; sent back, the item goes to its angle's `assignee`, or
/// to nobody (taken by role), in the status's own write. Only a PR
/// that names its sub-item drives it (the link the merge reads too), and
/// one that does not is told once per head which key to name; a status
/// already right is not written again.
#[allow(clippy::too_many_arguments)]
fn follow_angle_pr(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    pr: &PullRequest,
    sub: &Item,
    angle: &str,
    green: bool,
    fault: bool,
) {
    if !prs::names_sub_item(pr, &sub.key) {
        // Neither driven nor merged until it does: said once per head.
        super::merge::say_once(
            t,
            log,
            report,
            pr,
            "angle-unnamed",
            &[("pr", &pr.url), ("head", texts::short(&pr.head_sha))],
            &format!(
                "This angle PR names no sub-item, so the bridge neither follows nor merges it: name \
                 `{}` in its title or description (`docs/agents/review-release.md`, step 5).",
                sub.key
            ),
        );
        return;
    }
    let facts = prs::AnglePrFacts {
        green,
        sent_back: review::changes_requested_at_head(pr, &section(t).coordinators).is_some(),
        fault,
    };
    let Some(to) = prs::angle_item_status(sub.state.status, facts) else {
        return;
    };
    // Who is served, in the same write as the status: its angle again when
    // a coordinator sent it back (nobody named: taken by role); untouched
    // otherwise. In Review is the bridge's own step, served to nobody: the
    // item keeps its assignee while the bridge checks and merges the PR.
    let assignee: Option<Option<crate::tracker::User>> = match to {
        Status::InProgress if facts.sent_back => Some(angle_assignee(t, angle)),
        _ => None,
    };
    let who = match &assignee {
        None => String::new(),
        Some(None) => ", unassigned".to_string(),
        Some(Some(u)) => format!(", assigned to {}", u.name),
    };
    t.act(
        log,
        report,
        &sub.key,
        &format!("status {to}{who} (angle PR)"),
        json!({"pr": pr.url, "head": pr.head_sha}),
        || {
            t.tracker
                .set_status(&sub.key, to, assignee.as_ref().map(Option::as_ref))
                .map_err(|e| e.to_string())
        },
    );
}

/// The participant `release.coordinator_assignee` names, when the tracker
/// knows it (`doctor` reports one it does not).
fn coordinator(t: &Tick) -> Option<crate::tracker::User> {
    let name = section(t).coordinator_assignee.as_ref()?;
    t.tracker.user(name).ok()
}

/// The configured assignee of a release angle, when the tracker knows it.
fn angle_assignee(t: &Tick, angle: &str) -> Option<crate::tracker::User> {
    let name = section(t)
        .angles
        .iter()
        .find(|a| a.key == angle)?
        .assignee
        .as_ref()?;
    t.tracker.user(name).ok()
}

/// A merged angle report of the candidate, read from the merge.
pub struct MergedReport {
    pub angle: String,
    pub pr_url: String,
    pub path: String,
    pub report: AngleReport,
}

/// Every angle PR of the candidate merged into the release branch: its
/// angle marked merged, its sub-item To Deploy when it is not yet (spec 05
/// §5), its report read. Nothing is filed from a report: the coordinator's
/// verdict says which findings become tickets. `None` when a merged report
/// could not be read: the verdict then waits.
#[allow(clippy::too_many_arguments)]
fn merged_angles(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    item: &Item,
    c: &Candidate,
    children: &[Item],
    states: &mut [(String, AngleState)],
    merged: &[PullRequest],
) -> Option<Vec<MergedReport>> {
    let release = section(t);
    let mut out = Vec::new();
    let mut whole = true;
    for pr in merged {
        // A report becomes part of the release's evidence only when this
        // bridge merged it. A writer may merge a lookalike branch by hand,
        // but that must neither close its angle nor pre-empt the reviewer.
        if pr.cross_repository
            || !pr.merged_by.as_ref().is_some_and(|merged_by| {
                merged_by.is_bot && same_login(&t.github_login, &merged_by.login)
            })
        {
            continue;
        }
        let Some((cand, angle)) = prs::parse_angle_branch(&pr.head_ref) else {
            continue;
        };
        if cand != c.tag || angle == verdict::KEY {
            continue;
        }
        if let Some(s) = states.iter_mut().find(|(a, _)| *a == angle) {
            s.1 = AngleState::Merged;
        }
        if let Some(sub) = children
            .iter()
            .find(|ch| angle_of(ch, &c.tag).as_deref() == Some(angle.as_str()))
        {
            if !angle_merged(sub) {
                t.act(
                    log,
                    report,
                    &sub.key,
                    "status To Deploy (angle merged)",
                    json!({"pr": pr.url}),
                    || {
                        t.tracker
                            .set_status(&sub.key, Status::ToDeploy, None)
                            .map_err(|e| e.to_string())
                    },
                );
            }
        }
        let path = report::report_path(&release.record, &c.tag, &angle);
        let rev = pr
            .merge_commit_sha
            .clone()
            .unwrap_or_else(|| pr.head_sha.clone());
        let parsed = t
            .github
            .file(&t.repo, &path, &rev)
            .map_err(|e| e.to_string())
            .and_then(|text| report::read_merged(&text, &c.tag, &angle).map_err(|e| e.join("; ")));
        match parsed {
            Ok(r) => out.push(MergedReport {
                angle,
                pr_url: pr.url.clone(),
                path,
                report: r,
            }),
            Err(e) => {
                whole = false;
                t.unreadable(
                    report,
                    &item.key,
                    format!("the merged report of {}: {e}", pr.url),
                );
            }
        }
    }
    whole.then_some(out)
}

/// An angle sub-item whose report is merged, or that is closed: To Deploy
/// or later.
fn angle_merged(sub: &Item) -> bool {
    sub.state.is_resolved()
        || matches!(
            sub.state.status,
            Some(Status::ToDeploy | Status::PostDeploy)
        )
}

/// The verdict branch of the candidate: `qa/<candidate>/consolidation`.
fn is_verdict_pr(pr: &PullRequest, c: &Candidate) -> bool {
    prs::parse_angle_branch(&pr.head_ref) == Some((c.tag.clone(), verdict::KEY.to_string()))
}

/// The candidate's consolidation item back In Progress from In Review,
/// where its agent puts it at its verdict PR's opening (ASDLC-WMTX), when
/// what keeps the verdict from the merge is its coordinator's to mend: the
/// item is served again. Any other status is left as it is.
fn consolidation_back(t: &Tick, log: &mut ActionLog, report: &mut Report, cons: &Item, why: &str) {
    if cons.state.status != Some(Status::InReview) {
        return;
    }
    t.act(
        log,
        report,
        &cons.key,
        &format!("status In Progress ({why})"),
        json!({}),
        || {
            t.tracker
                .set_status(&cons.key, Status::InProgress, None)
                .map_err(|e| e.to_string())
        },
    );
}

/// Whether a merged verdict PR is the coordinator's (BR-ASDLC-020): a
/// release coordinator authored it or approved it at its merged head, or
/// the bridge's App merged it (it merges only such a verdict). A verdict
/// merged around the bridge by anyone else is not the coordinator's call.
fn trusted_verdict(t: &Tick, pr: &PullRequest) -> bool {
    let coordinators = &section(t).coordinators;
    pr.author
        .as_ref()
        .is_some_and(|a| coordinators.iter().any(|x| same_login(x, &a.login)))
        || matches!(
            review::pr_approval(pr, coordinators),
            Approval::Qualified { .. }
        )
        || pr
            .merged_by
            .as_ref()
            .is_some_and(|m| m.is_bot && same_login(&t.github_login, &m.login))
}

/// The candidate's merged verdict PR the GO trusts ([`trusted_verdict`]),
/// and whether a verdict PR of the candidate was merged at all.
fn merged_verdict_pr<'a>(
    t: &Tick,
    c: &Candidate,
    merged_pulls: &'a [PullRequest],
) -> (Option<&'a PullRequest>, bool) {
    let mut verdicts = merged_pulls.iter().filter(|p| is_verdict_pr(p, c));
    let trusted = verdicts.clone().find(|p| trusted_verdict(t, p));
    (trusted, verdicts.next().is_some())
}

/// The candidate's coordination item, open or not.
fn consolidation_of<'a>(children: &'a [Item], c: &Candidate) -> Option<&'a Item> {
    let want = Some(BridgeItem::Consolidation {
        scope: c.tag.clone(),
    });
    children
        .iter()
        .find(|ch| texts::bridge_item(&ch.description, ch.parent.as_deref()) == want)
}

/// The coordinator's verdict on the candidate (BR-ASDLC-020): once merged,
/// its accepted findings are filed (each once) and the coordination item
/// goes Done; while its PR is open, the PR is merged when it may be. Where
/// the verdict stands, for the GO.
#[allow(clippy::too_many_arguments)]
fn candidate_verdict(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    item: &Item,
    c: &Candidate,
    branch: &str,
    label: &str,
    children: &[Item],
    states: &[(String, AngleState)],
    reports: Option<&[MergedReport]>,
    open: &[PullRequest],
    merged_pulls: &[PullRequest],
    merged: &mut bool,
) -> VerdictState {
    let release = section(t);
    let path = verdict::path(&release.record, &c.tag);
    let consolidation = consolidation_of(children, c);
    let Some(reports) = reports else {
        return VerdictState::Missing;
    };
    let (trusted, any_merged) = merged_verdict_pr(t, c, merged_pulls);
    if let Some(pr) = trusted {
        let v = match read_merged_verdict(t, c, pr) {
            Ok(v) => v,
            Err(e) => {
                t.unreadable(
                    report,
                    &item.key,
                    format!("the merged verdict of {}: {e}", pr.url),
                );
                return VerdictState::Missing;
            }
        };
        file_verdict(t, log, report, item, c, label, &v, reports);
        if let Some(cons) = consolidation.filter(|i| !i.state.is_resolved()) {
            t.act(
                log,
                report,
                &cons.key,
                "status Done (verdict merged)",
                json!({"pr": pr.url, "verdict": v.call.name()}),
                || {
                    t.tracker
                        .set_status(&cons.key, Status::Done, None)
                        .map_err(|e| e.to_string())
                },
            );
        }
        return match v.call {
            verdict::Call::Go => VerdictState::Go,
            verdict::Call::Hold => VerdictState::Hold,
        };
    }
    if let Some(cons) = consolidation {
        // A verdict given in the tracker, committed by the bridge (rule 2W8R).
        let signed = super::tracker_verdict::pass(
            t, log, report, item, c, branch, cons, states, reports, open, &path,
        );
        match (
            open.iter().find(|p| is_verdict_pr(p, c)),
            signed == super::tracker_verdict::Outcome::Wrote,
        ) {
            (Some(pr), false) => verdict_pr(
                t, log, report, pr, c, branch, cons, states, reports, &path, &signed, merged,
            ),
            // Moved In Review by its agent at its PR's opening, it has no
            // PR left for the bridge to check (ASDLC-WMTX).
            (None, false) => consolidation_back(t, log, report, cons, "no verdict PR open"),
            _ => {}
        }
    }
    if any_merged {
        VerdictState::Untrusted
    } else {
        VerdictState::Missing
    }
}

/// The verdict a merged verdict PR carries, read at its merge.
fn read_merged_verdict(
    t: &Tick,
    c: &Candidate,
    pr: &PullRequest,
) -> Result<verdict::Verdict, String> {
    let path = verdict::path(&section(t).record, &c.tag);
    let rev = pr
        .merge_commit_sha
        .clone()
        .unwrap_or_else(|| pr.head_sha.clone());
    t.github
        .file(&t.repo, &path, &rev)
        .map_err(|e| e.to_string())
        .and_then(|text| verdict::parse(&text, &c.tag).map_err(|e| e.join("; ")))
}

/// Where the candidate's merged verdict stands, read-only (`release
/// status`): `Missing` until its PR is merged into the release branch,
/// `Untrusted` while no merged one is the coordinator's.
pub fn merged_verdict_state(
    t: &Tick,
    c: &Candidate,
    merged_pulls: &[PullRequest],
) -> Result<VerdictState, String> {
    let pr = match merged_verdict_pr(t, c, merged_pulls) {
        (Some(pr), _) => pr,
        (None, true) => return Ok(VerdictState::Untrusted),
        (None, false) => return Ok(VerdictState::Missing),
    };
    let v = read_merged_verdict(t, c, pr)
        .map_err(|e| format!("the merged verdict of {}: {e}", pr.url))?;
    Ok(match v.call {
        verdict::Call::Go => VerdictState::Go,
        verdict::Call::Hold => VerdictState::Hold,
    })
}

/// The accepted findings of a merged verdict, filed once each with the
/// blocking the coordinator gave them (spec 05 §6); a rejected one is never
/// filed.
#[allow(clippy::too_many_arguments)]
fn file_verdict(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    item: &Item,
    c: &Candidate,
    label: &str,
    v: &verdict::Verdict,
    reports: &[MergedReport],
) {
    for r in reports {
        let accepted: Vec<Finding> = v
            .accepted()
            .filter(|j| j.angle == r.angle)
            .filter_map(|j| {
                r.report
                    .findings
                    .iter()
                    .find(|f| f.id == j.id)
                    .map(|f| Finding {
                        blocking: j.blocking,
                        ..f.clone()
                    })
            })
            .collect();
        if accepted.is_empty() {
            continue;
        }
        file_findings(
            t, log, report, item, c, &r.angle, &r.pr_url, &r.path, label, &accepted,
        );
    }
}

/// The open verdict PR of the candidate (`qa/<candidate>/consolidation`),
/// merged into the release branch when it names the coordination item,
/// carries only a valid verdict, every angle PR of the candidate is merged,
/// its author is a coordinator or a coordinator approved it at its head,
/// its required checks are green and it is up to date. What keeps it from
/// the merge, beside the checks and the branch, is said once per head.
#[allow(clippy::too_many_arguments)]
fn verdict_pr(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    pr: &PullRequest,
    c: &Candidate,
    branch: &str,
    cons: &Item,
    states: &[(String, AngleState)],
    reports: &[MergedReport],
    path: &str,
    signed: &super::tracker_verdict::Outcome,
    merged: &mut bool,
) {
    let release = section(t);
    let object = format!("pr#{}", pr.number);
    let head = texts::short(&pr.head_sha);
    let fields = [("pr", pr.url.as_str()), ("head", head)];
    let say = |log: &mut ActionLog, report: &mut Report, kind: &str, text: String| {
        super::merge::say_once(t, log, report, pr, kind, &fields, &text);
    };
    if *merged {
        return;
    }
    if !prs::names_sub_item(pr, &cons.key) {
        consolidation_back(t, log, report, cons, "its verdict PR names no item");
        say(
            log,
            report,
            "verdict-unnamed",
            format!(
                "This verdict PR names no coordination item, so the bridge does not merge it: name `{}` in its title or description (`docs/agents/consolidate.md`).",
                cons.key
            ),
        );
        return;
    }
    let open: Vec<&str> = states
        .iter()
        .filter(|(_, s)| *s != AngleState::Merged)
        .map(|(a, _)| a.as_str())
        .collect();
    if !open.is_empty() {
        say(
            log,
            report,
            "verdict-early",
            format!(
                "The bridge merges the verdict once every angle PR of `{}` is merged; still open: {}.",
                c.tag,
                open.join(", ")
            ),
        );
        return;
    }
    match t.github.compare(&t.repo, branch, &pr.head_sha) {
        Ok(cmp) => {
            let others = cmp.files.iter().filter(|f| *f != path).count();
            if others > 0 {
                consolidation_back(t, log, report, cons, "its verdict PR changes other files");
                say(
                    log,
                    report,
                    "verdict-not-only",
                    format!(
                        "This verdict PR changes {others} file(s) besides the verdict `{path}`, so the bridge does not merge it: the verdict judges, it fixes nothing."
                    ),
                );
                return;
            }
        }
        Err(e) => {
            t.unreadable(
                report,
                &object,
                format!("the changes of the verdict PR against {branch}: {e}"),
            );
            return;
        }
    }
    let findings: Vec<(String, Finding)> = reports
        .iter()
        .flat_map(|r| {
            r.report
                .findings
                .iter()
                .map(|f| (r.angle.clone(), f.clone()))
        })
        .collect();
    let at_head = t.github.file(&t.repo, path, &pr.head_sha);
    let valid = match &at_head {
        Ok(text) => verdict::validate(text, &c.tag, &findings).and_then(|v| {
            if v.item.eq_ignore_ascii_case(&cons.key) {
                Ok(v)
            } else {
                Err(vec![format!(
                    "`item` must be \"{}\", the coordination item",
                    cons.key
                )])
            }
        }),
        Err(e) if e.is_not_found() => Err(vec![format!("no verdict at `{path}`")]),
        Err(e) => {
            t.unreadable(report, &object, e);
            return;
        }
    };
    if let Err(errors) = valid {
        consolidation_back(t, log, report, cons, "its verdict is not valid");
        say(
            log,
            report,
            "verdict-invalid",
            format!(
                "This verdict is not valid, so the bridge does not merge it (`docs/reference/06-qa-report.md`, \"The verdict\"):\n\n{}",
                errors
                    .iter()
                    .map(|e| format!("- {e}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
        );
        return;
    }
    let by_author = pr
        .author
        .as_ref()
        .is_some_and(|a| release.coordinators.iter().any(|x| same_login(x, &a.login)))
        // The bridge's own PR, carrying the verdict a person signed in the
        // tracker (rule 2W8R).
        || at_head
            .as_ref()
            .is_ok_and(|text| super::tracker_verdict::vouches(t, pr, text, signed));
    let approved = match review::pr_approval(pr, &release.coordinators) {
        Approval::Qualified { by } => Some(by),
        _ => None,
    };
    if !by_author && approved.is_none() {
        say(
            log,
            report,
            "verdict-unapproved",
            "The verdict is the coordinator's: the bridge merges it when a release coordinator authored this PR or approved it at its head.".to_string(),
        );
        return;
    }
    if review::changes_requested_at_head(pr, &release.coordinators).is_some() {
        return;
    }
    let required = match t.github.required_checks(&t.repo, branch) {
        Ok(r) => r,
        Err(e) => {
            t.unreadable(report, &object, e);
            return;
        }
    };
    match checks::pr_state(pr, &required.contexts) {
        CheckState::Green => {}
        CheckState::Red => {
            consolidation_back(t, log, report, cons, "its verdict PR's checks are red");
            return;
        }
        _ => return,
    }
    match review::branch_state(pr) {
        BranchState::Conflicting => return,
        BranchState::Behind => {
            t.act(
                log,
                report,
                &object,
                "update branch",
                json!({"base": branch}),
                || {
                    t.github
                        .update_branch(&t.repo, pr.number, &pr.head_sha)
                        .map_err(|e| e.to_string())
                },
            );
            return;
        }
        BranchState::Current => {}
    }
    let dismissals = match super::merge::superseded(pr, &release.coordinators) {
        Ok(d) => d,
        Err(why) => {
            t.wait(report, pr, why);
            return;
        }
    };
    match &approved {
        Some(by) => {
            if !super::merge::dismiss(t, log, report, &object, pr, by, &dismissals) {
                return;
            }
        }
        None if !dismissals.is_empty() => {
            let who: Vec<&str> = dismissals.iter().map(|x| x.login.as_str()).collect();
            t.wait(
                report,
                pr,
                format!("changes requested by {} stand", who.join(", ")),
            );
            return;
        }
        None => {}
    }
    let done = t.act(
        log,
        report,
        &object,
        "merge (merge commit) the verdict into the release branch",
        json!({"head": pr.head_sha, "approved_by": approved}),
        || {
            t.github
                .merge_pull(
                    &t.repo,
                    pr.number,
                    &pr.head_sha,
                    crate::github::MergeMethod::Merge,
                )
                .map_err(|e| e.to_string())
        },
    );
    if done.is_some() {
        *merged = true;
    }
}

#[allow(clippy::too_many_arguments)]
fn file_findings(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    item: &Item,
    c: &Candidate,
    angle: &str,
    pr_url: &str,
    path: &str,
    label: &str,
    to_file: &[Finding],
) {
    let labelled = match t.tracker.list_by_label(label) {
        Ok(l) => l,
        Err(e) => {
            t.unreadable(report, &item.key, e);
            return;
        }
    };
    let relations = match t.tracker.relations(&item.key) {
        Ok(r) => r,
        Err(e) => {
            t.unreadable(report, &item.key, e);
            return;
        }
    };
    let blocked_by: Vec<String> = relations
        .blocked_by
        .iter()
        .map(|b| b.other.clone())
        .collect();
    // A finding ticket whose description lost its marker is looked for by
    // its identity comment among the items related to the release item
    // only, never among every ticket the release embarks.
    let pool: Vec<Item> = labelled
        .iter()
        .filter(|i| {
            findings::parse_finding_marker(&i.description).is_some()
                || blocked_by.contains(&i.key)
                || relations.related.iter().any(|r| r.other == i.key)
        })
        .cloned()
        .collect();
    let release_id = item.key.as_str();
    // Read once, and only when a blocking finding is filed.
    let mut fixer_of: Option<Option<crate::tracker::User>> = None;
    for f in to_file {
        // Filed once, whatever became of it: a finding canceled as a false
        // alarm or requalified stays so (the merged report does not
        // change). A new candidate's report is another key.
        let marker = findings::finding_marker(release_id, &c.tag, angle, &f.id);
        let filed = match identity::find(t, log, report, &pool, &marker, |i| {
            findings::parse_finding_marker(&i.description).is_some_and(|(rel, cand, a, id)| {
                rel == release_id && cand == c.tag && a == angle && id == f.id
            })
        }) {
            Ok(found) => found.into_iter().next(),
            Err(e) => {
                t.unreadable(report, &item.key, e);
                continue;
            }
        };
        if let Some(ticket) = filed {
            // A blocking finding whose relation failed after its creation:
            // the relation is laid again (REQ-REL-058).
            let marked =
                findings::parse_blocker_marker(&ticket.description).as_deref() == Some(release_id);
            if marked && !ticket.state.is_resolved() && !blocked_by.contains(&ticket.key) {
                relate(t, log, report, &ticket.key, &item.key, RelationKind::Blocks);
            }
            continue;
        }
        // A blocking finding: an urgent fix ticket for the release fixer,
        // which blocks the release issue. A non-blocking one: a ticket
        // logged for later, related to it (BR-ASDLC-022).
        let (title, body) = findings::finding_ticket(release_id, &c.tag, angle, f, pr_url, path);
        let mut labels = vec![label.to_string()];
        if !f.blocking {
            labels.push(vocabulary::QA_FINDING_LABEL.to_string());
        }
        let new = NewItem {
            title,
            description: for_tracker(t, &body),
            labels,
            status: if f.blocking {
                Status::Ready
            } else {
                Status::Triage
            },
            priority: Some(if f.blocking {
                Priority::Urgent
            } else {
                Priority::Normal
            }),
            assignee: if f.blocking {
                fixer_of.get_or_insert_with(|| fixer(t)).clone()
            } else {
                None
            },
            parent: None,
            kind: ItemKind::Ticket,
        };
        let created = identity::create(
            t,
            log,
            report,
            &item.key,
            &format!("create finding {}", f.id),
            json!({
                "blocking": f.blocking,
                "angle": angle,
                "priority": new.priority.map(Priority::name),
                "assignee": new.assignee.as_ref().map(|u| u.name.as_str()),
            }),
            &new,
        );
        if let Some(ticket) = created {
            let kind = if f.blocking {
                RelationKind::Blocks
            } else {
                RelationKind::Related
            };
            relate(t, log, report, &ticket.key, &item.key, kind);
        }
    }
}

fn relate(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    a: &str,
    b: &str,
    kind: RelationKind,
) {
    if kind == RelationKind::Blocks {
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
    }
    t.act(
        log,
        report,
        a,
        &format!("relate {kind:?} {b}"),
        json!({}),
        || t.tracker.relate(a, b, kind).map_err(|e| e.to_string()),
    );
}

/// The GO and its facts.
#[allow(clippy::too_many_arguments)]
pub fn decide(
    t: &Tick,
    report: &mut Report,
    item: &Item,
    c: &Candidate,
    branch: &str,
    states: &[(String, AngleState)],
    no_qa_angle: bool,
    verdict: VerdictState,
) -> Option<go::GoDecision> {
    let cfg = t.config;
    let release = section(t);
    let gate_green = match gate_run(t, c) {
        Ok(run) => Some(gate::is_green(run.as_ref())),
        Err(e) => {
            t.unreadable(report, &item.key, format!("the gate of {}: {e}", c.tag));
            None
        }
    };
    let relations = match t.tracker.relations(&item.key) {
        Ok(r) => r,
        Err(e) => {
            t.unreadable(report, &item.key, e);
            return None;
        }
    };
    let label = vocabulary::release_label(&cfg.product.name, &c.version.to_string());
    let mut blockers = Vec::new();
    let mut fix_gaps = Vec::new();
    for b in &relations.blocked_by {
        match t.tracker.get(&b.other) {
            Ok(i) if i.state.is_resolved() => {}
            Ok(i) => match blocker_fix(t, report, item, &label, c, &i) {
                Fix::InCandidate => {}
                Fix::Gap(gap) => fix_gaps.push(gap),
                Fix::NotMerged => blockers.push(b.other.clone()),
            },
            Err(e) => {
                t.unreadable(report, &b.other, e);
                return None;
            }
        }
    }
    let unlinked_blockers: Vec<String> = match t.tracker.list_by_label(&label) {
        Ok(items) => items
            .into_iter()
            .filter(|i| !i.state.is_resolved())
            .filter(|i| {
                findings::parse_blocker_marker(&i.description).as_deref() == Some(item.key.as_str())
            })
            .filter(|i| !relations.blocked_by.iter().any(|b| b.other == i.key))
            .filter(|i| !matches!(blocker_fix(t, report, item, &label, c, i), Fix::InCandidate))
            .map(|i| i.key)
            .collect(),
        Err(e) => {
            t.unreadable(report, &item.key, e);
            return None;
        }
    };
    // The branch moved past the candidate with more than bookkeeping: fixes
    // no gate and no QA has seen (REQ-REL-070). A report of the candidate's
    // own angles is never a gap: its angle PR is merged after the candidate
    // by construction, and the last allowed pass cuts no candidate after
    // it. An earlier candidate's report is a gap.
    let tip_gaps = match t.github.compare(&t.repo, &c.sha, branch) {
        Ok(cmp) => {
            let cut = cmp.files_cut;
            let moved: Vec<String> = cmp
                .files
                .into_iter()
                .filter(|f| !bookkeeping(release, c, f))
                .collect();
            // Who cuts the next candidate (rule C8WN): the bridge with
            // `auto_candidate`, a person or the coordinator otherwise.
            let next = if release.auto_candidate {
                "the bridge cuts the next candidate once the release's fixes are merged"
            } else {
                "a cut of the next candidate is due"
            };
            // A list GitHub cut is more than bookkeeping: its other files
            // are not known (rule X4PV).
            if moved.is_empty() && !cut {
                Vec::new()
            } else {
                vec![format!(
                    "{branch} moved past {} ({}): {next}",
                    c.tag,
                    texts::file_count(&moved, cut)
                )]
            }
        }
        Err(e) => vec![format!(
            "{branch} could not be compared with {}: {e}",
            c.tag
        )],
    };
    // A candidate whose release was retracted (its publication failed before
    // deploying) gets no second GO: the next candidate takes the version.
    let release_tag = Shape::of(&cfg.product.name, release).release_tag(c.version);
    // The commit the GO would tag; one it cannot tag is a tip gap, said.
    let mut tip_gaps = tip_gaps;
    let target = match go_target(t, c, branch) {
        Ok(sha) => sha,
        Err(e) => {
            tip_gaps.push(e);
            c.sha.clone()
        }
    };
    let retracted_at = |sha: &str| super::retract::retracted(t, &item.key, &release_tag, sha);
    let retracted = match retracted_at(&c.sha)
        .and_then(|on_candidate| Ok(on_candidate || (target != c.sha && retracted_at(&target)?)))
    {
        Ok(true) => Some(release_tag),
        Ok(false) => None,
        Err(e) => {
            t.unreadable(report, &item.key, e);
            return None;
        }
    };
    let policy = match release.go_policy {
        crate::config::GoPolicy::Computed => Policy::Computed,
        // Whose approval counts is the owner's to settle (`.spl-asdlc/open-points.md` #3): never given.
        crate::config::GoPolicy::Human => Policy::Human { approved: None },
    };
    // A person holds the GO with a label on the release item: merges and
    // ticket moves go on, the release is neither tagged nor published.
    let held = item
        .labels
        .iter()
        .find(|l| l.eq_ignore_ascii_case(&release.hold_label))
        .cloned();
    let facts = GoFacts {
        release: item.key.clone(),
        gate_green,
        angles: states.to_vec(),
        no_qa_angle,
        verdict,
        blockers,
        unlinked_blockers,
        fix_gaps,
        tip_gaps,
        retracted,
        held,
        policy,
    };
    Some(go::evaluate(&facts))
}

/// Where an open blocker of the release stands against the candidate
/// (spec 05 §8, ASDLC-SEJA).
#[derive(Debug, PartialEq, Eq)]
enum Fix {
    /// Not a blocking finding of this release whose fix is merged: it is an
    /// open blocking issue.
    NotMerged,
    /// A blocking finding whose fix is merged and in the candidate: it no
    /// longer holds the GO, though it reaches Done only at the closure.
    InCandidate,
    /// A blocking finding whose fix is merged, but not in the candidate, or
    /// not known to be: a fix gap, which a newer candidate that contains it
    /// frees.
    Gap(String),
}

/// Whether the open blocker `i` of the release `item` is freed by the
/// candidate: a blocking finding of this release (the release label and the
/// blocker marker naming the release item) at To Deploy or later, every PR
/// the bridge recorded as merged on it merged by a commit the candidate
/// contains (REQ-REL-059). Closing it is the closure's, after the publication,
/// which needs the GO: counting it open until Done would never let the
/// release go. Fail closed: a PR or an ancestry that cannot be read is a
/// gap, never a freed blocker.
fn blocker_fix(
    t: &Tick,
    report: &mut Report,
    item: &Item,
    label: &str,
    c: &Candidate,
    i: &Item,
) -> Fix {
    let past_merge = matches!(i.state.status, Some(Status::ToDeploy | Status::PostDeploy));
    let finding = i.labels.iter().any(|l| l.eq_ignore_ascii_case(label))
        && findings::parse_blocker_marker(&i.description).as_deref() == Some(item.key.as_str());
    if !past_merge || !finding {
        return Fix::NotMerged;
    }
    let unread = |report: &mut Report, e: String| {
        t.unreadable(
            report,
            &i.key,
            format!("the fix of this blocking finding: {e}"),
        );
        Fix::Gap(format!("{}: its fix could not be read", i.key))
    };
    let numbers = match merged_pr_numbers(t, &i.key) {
        Ok(n) => n,
        Err(e) => return unread(report, e.to_string()),
    };
    if numbers.is_empty() {
        return Fix::Gap(format!("{}: no merged PR of it is recorded", i.key));
    }
    // Every recorded PR: a fix that took two PRs is in when both are.
    for n in numbers {
        let pr = match t.github.pull(&t.repo, n) {
            Ok(pr) => pr,
            Err(e) => return unread(report, format!("PR #{n}: {e}")),
        };
        let sha = pr.merge_commit_sha.as_deref().filter(|s| !s.is_empty());
        let Some(sha) = sha.filter(|_| pr.state == crate::github::PrState::Merged) else {
            return Fix::Gap(format!("{}: PR #{n} is not merged", i.key));
        };
        match t.github.compare_status(&t.repo, sha, &c.sha) {
            Ok(CompareStatus::Ahead | CompareStatus::Identical) => {}
            Ok(CompareStatus::Behind | CompareStatus::Diverged) => {
                return Fix::Gap(format!("{}: PR #{n} is not in {}", i.key, c.tag));
            }
            Err(e) => return unread(report, format!("PR #{n} against {}: {e}", c.tag)),
        }
    }
    Fix::InCandidate
}

/// The PRs the bridge recorded as merged on a ticket (its `merged_pr: <url>`
/// lines), oldest first, once each.
fn merged_pr_numbers(t: &Tick, key: &str) -> crate::tracker::TResult<Vec<u64>> {
    let mut out = Vec::new();
    for n in t
        .tracker
        .comments(key)?
        .iter()
        .filter(|c| c.by_bridge)
        .flat_map(|c| c.body.lines())
        .filter_map(|l| l.trim().strip_prefix("merged_pr: "))
        .filter_map(|url| url.trim().rsplit('/').next()?.parse::<u64>().ok())
    {
        if !out.contains(&n) {
            out.push(n);
        }
    }
    Ok(out)
}

/// Whether `path` may differ between the candidate and the release branch
/// tip (REQ-REL-070): a configured bookkeeping path (exact), the report of
/// one of the candidate's own angles or its coordinator's verdict
/// (`release.record` rendered for `candidate`), and with a release note
/// writer the version's note files, where the unified note is written
/// after the cut.
pub(super) fn bookkeeping(release: &Release, c: &Candidate, path: &str) -> bool {
    release.bookkeeping_paths.iter().any(|b| b == path)
        || verdict::path(&release.record, &c.tag) == path
        || release
            .angles
            .iter()
            .any(|a| report::report_path(&release.record, &c.tag, &a.key) == path)
        || (release.note_writer.is_some()
            && super::release_note::note_paths(release, &c.version.to_string())
                .iter()
                .any(|p| p == path))
}

/// On GO (spec 05 §9): the release tag on the candidate's commit, then the
/// GitHub release of that tag, its notes the CHANGELOG section the cut
/// prepared on the release branch. Publishing the release starts the
/// publication workflow. Each step is read before it is written, so a tag
/// or a release a person made by hand (spec 09) is taken as done, never
/// made again; a tag on another commit stops the GO, fail-closed.
/// Write the candidate's release note into the release item (spec 05 §12):
/// at its brief, and again at the GO before the release is published, so a
/// GO a person makes in GitHub finds it there too. The item is read again,
/// since this tick may have written its candidate line (a dry run, which
/// wrote nothing and may only have pictured the item, uses it as read).
/// Whether the item carries the note now.
fn note_into_item(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    shape: &Shape,
    item: &Item,
    c: &Candidate,
) -> bool {
    let fresh = match t.tracker.get(&item.key) {
        Ok(i) => i,
        Err(_) if t.dry_run => item.clone(),
        Err(e) => {
            t.unreadable(report, &item.key, e);
            return false;
        }
    };
    match closure::note_block(t, shape, c) {
        Ok(block) => closure::describe_note(t, log, report, &fresh, &block),
        Err(e) => {
            t.unreadable(report, &item.key, format!("the release note: {e}"));
            false
        }
    }
}

fn on_go(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    shape: &Shape,
    item: &Item,
    c: &Candidate,
    branch: &str,
) {
    let tag = shape.release_tag(c.version);
    // The commit the release tags: the candidate's, or the release branch's
    // tip when a unified release note was written there (spec 05 §12).
    let target = match go_target(t, c, branch) {
        Ok(sha) => sha,
        Err(e) => {
            t.unreadable(report, &item.key, e);
            return;
        }
    };
    let at = target.as_str();
    let notes = match t.github.file(&t.repo, crate::prepare::CHANGELOG, at) {
        Ok(text) => match prepare::section_of(&text, c.version) {
            Some(n) => n,
            None => {
                t.unreadable(
                    report,
                    &item.key,
                    format!(
                        "{} at {} has no section for {}: the cut did not prepare it, so the release has no notes",
                        crate::prepare::CHANGELOG,
                        c.tag,
                        c.version
                    ),
                );
                return;
            }
        },
        Err(e) => {
            t.unreadable(
                report,
                &item.key,
                format!("{} at {}: {e}", crate::prepare::CHANGELOG, c.tag),
            );
            return;
        }
    };
    // The release item carries its release note before the release is
    // published (spec 05 §12): no placeholder is left at publication. A note
    // that cannot be built or written holds the GO; the next tick retries.
    if !note_into_item(t, log, report, shape, item, c) {
        return;
    }
    let existing = match release_tag_commit(t, &tag) {
        Ok(e) => e,
        Err(e) => {
            t.unreadable(report, &item.key, format!("the tag {tag}: {e}"));
            return;
        }
    };
    // A tag a retraction left on the retracted candidate (its deletion
    // failed after the release's): the retraction is finished first.
    let existing = match existing {
        Some(sha) if sha != target => match super::retract::retracted(t, &item.key, &tag, &sha) {
            Ok(true) => {
                if !super::retract::delete_tag(t, log, report, item, &tag, &sha) {
                    return;
                }
                None
            }
            Ok(false) => Some(sha),
            Err(e) => {
                t.unreadable(report, &item.key, e);
                return;
            }
        },
        other => other,
    };
    match existing {
        Some(sha) if sha == target => {}
        Some(sha) => {
            t.unreadable(
                report,
                &item.key,
                format!(
                    "{tag} exists on {}, not on the candidate {} ({}): the bridge publishes no release of it",
                    texts::short(&sha),
                    c.tag,
                    texts::short(&target)
                ),
            );
            return;
        }
        None => {
            // A patch's release tag says so, as its first candidate did: the
            // publication carries it into the open cycle, never the default
            // branch (ASDLC-MG26).
            let patches = match cycle_patches(t, shape, c) {
                Ok(p) => p,
                Err(e) => {
                    t.unreadable(
                        report,
                        &item.key,
                        format!("the cycle's first candidate: {e}"),
                    );
                    return;
                }
            };
            let mut message = format!("Release {tag}\n\nCandidate: {}", c.tag);
            if let Some(from) = patches {
                message.push_str(&format!("\nRelease-Patch-From: {from}"));
            }
            let made = t.act(
                log,
                report,
                &item.key,
                &format!("tag {tag} (GO)"),
                json!({"candidate": c.tag, "sha": target}),
                || {
                    t.github
                        .create_tag(&t.repo, &tag, &target, &message)
                        .map_err(|e| e.to_string())
                },
            );
            if made.is_none() {
                return;
            }
        }
    }
    match t.github.release(&t.repo, &tag) {
        Ok(r) if r.draft => {
            t.unreadable(
                report,
                &item.key,
                format!(
                    "a draft release of {tag} exists: publish it or delete it; the bridge publishes no second one"
                ),
            );
            return;
        }
        Ok(_) => return,
        Err(e) if e.is_not_found() => {}
        Err(e) => {
            t.unreadable(report, &item.key, format!("the release {tag}: {e}"));
            return;
        }
    }
    let name = format!("{} {}", t.config.product.name, c.version);
    let published = t.act(
        log,
        report,
        &item.key,
        &format!("publish release {tag} (GO)"),
        json!({"candidate": c.tag}),
        || {
            t.github
                .create_release(&t.repo, &tag, &name, &notes)
                .map_err(|e| e.to_string())
        },
    );
    if let Some(r) = published {
        let body = format!(
            "GO computed on `{}`: the bridge tagged `{tag}` on its commit `{}` and published the release {}, with the notes the cut prepared. The publication workflow takes over.",
            c.tag,
            texts::short(&c.sha),
            r.url
        );
        t.act(log, report, &item.key, "comment GO", json!({}), || {
            t.tracker
                .comment(&item.key, &body)
                .map(|_| ())
                .map_err(|e| e.to_string())
        });
    }
}

/// The commit the GO tags (spec 05 §9, §12): the candidate's own, or, with a
/// release note writer and the version's note files changed on the release
/// branch since the candidate, the branch's tip, so that the tag carries the
/// unified note to GitHub and to the apps. The GO already holds unless the
/// tip differs from the candidate by bookkeeping only (§8).
fn go_target(t: &Tick, c: &Candidate, branch: &str) -> Result<String, String> {
    let release = section(t);
    if release.note_writer.is_none() {
        return Ok(c.sha.clone());
    }
    let notes = super::release_note::note_paths(release, &c.version.to_string());
    let cmp = t
        .github
        .compare(&t.repo, &c.sha, branch)
        .map_err(|e| format!("{branch} could not be compared with {}: {e}", c.tag))?;
    // A list GitHub cut may hide the note files and others: the tip is
    // never tagged on it (rule X4PV).
    if cmp.files_cut {
        return Err(format!(
            "{branch} differs from {} by {}: the release cannot be tagged on its tip; tag the next candidate",
            c.tag,
            texts::file_count(&cmp.files, true)
        ));
    }
    if !cmp.files.iter().any(|f| notes.contains(f)) {
        return Ok(c.sha.clone());
    }
    // The tip is tagged only when it differs from the candidate by what the
    // publication's guard lets follow it: the note files, this candidate's
    // own angle reports, and its coordinator's verdict. The commit tagged
    // is the one this comparison read, never a tip read again later.
    let others: Vec<&String> = cmp
        .files
        .iter()
        .filter(|f| !notes.contains(f))
        .filter(|f| verdict::path(&release.record, &c.tag) != **f)
        .filter(|f| {
            !release
                .angles
                .iter()
                .any(|a| report::report_path(&release.record, &c.tag, &a.key) == **f)
        })
        .collect();
    if !others.is_empty() {
        return Err(format!(
            "{branch} carries the unified release note and {} other file(s) since {}: the release cannot be tagged on its tip; tag the next candidate",
            others.len(),
            c.tag
        ));
    }
    if cmp.status != CompareStatus::Ahead {
        return Err(format!(
            "{branch} is not ahead of {}: the unified release note cannot be tagged",
            c.tag
        ));
    }
    cmp.commits
        .last()
        .map(|commit| commit.sha.clone())
        .ok_or_else(|| format!("{branch} lists no commit since {}", c.tag))
}

/// The commit the release tag names, or `None` when it does not exist.
pub(crate) fn release_tag_commit(t: &Tick, tag: &str) -> Result<Option<String>, String> {
    let refs = t.github.tags(&t.repo, tag).map_err(|e| e.to_string())?;
    let Some(r) = refs.iter().find(|r| r.name == tag) else {
        return Ok(None);
    };
    match r.object_type {
        ObjectType::Tag => t
            .github
            .tag_object(&t.repo, &r.sha)
            .map(|o| Some(o.target_sha))
            .map_err(|e| e.to_string()),
        _ => Ok(Some(r.sha.clone())),
    }
}

/// The HOLD, once per candidate and set of reasons, only when every angle
/// answered and something the QA found or a tip gap holds the release
/// (REQ-REL-069).
fn hold(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    item: &Item,
    c: &Candidate,
    d: &go::GoDecision,
) {
    if !d.every_angle_answered()
        || (d.blockers.is_empty() && d.fix_gaps.is_empty() && d.tip_gaps.is_empty())
    {
        return;
    }
    let key = texts::hold_key(&d.blockers, &d.fix_gaps, &d.tip_gaps);
    let marker = texts::marker("hold", &[("candidate", &c.tag), ("key", &key)]);
    if let Ok(false) = tracker_said(t, &item.key, &marker) {
        let body = for_tracker(
            t,
            &texts::hold_comment(&c.tag, &d.reasons, &d.blockers_carried(), &key),
        );
        t.act(
            log,
            report,
            &item.key,
            "comment HOLD",
            json!({"reasons": d.reasons}),
            || {
                t.tracker
                    .comment(&item.key, &body)
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            },
        );
    }
}

/// When the last allowed pass is spent and something still holds: the
/// release goes to the owner, once (spec 05 §10). The hand-off is
/// idempotent as a whole: the owner assigned before it is explained, the
/// hold label (created first when the tracker lacks it) written whenever it
/// is missing while the escalation holds, the comment carrying the marker
/// posted once. A failed label write costs the label, retried at the next
/// tick, never the comment.
pub(super) fn escalate(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    item: &Item,
    c: &Candidate,
    d: &go::GoDecision,
    _branch: &str,
) {
    let release = section(t);
    let max = release.max_rc_passes;
    let moved = !d.tip_gaps.is_empty();
    if c.pass() < u64::from(max) || !d.every_angle_answered() || (d.blockers.is_empty() && !moved) {
        return;
    }
    let marker = texts::marker("qa-escalation", &[("candidate", &c.tag)]);
    let Ok(said) = tracker_said(t, &item.key, &marker) else {
        return;
    };
    // Not yet explained: the owner first, so that the comment never names
    // a hand-off that did not happen; a failed assignment is retried whole.
    if !said {
        let owner = match t.tracker.user(&t.config.owner.tracker_user) {
            Ok(o) => o,
            Err(e) => {
                t.unreadable(
                    report,
                    &item.key,
                    format!("the owner {}: {e}", t.config.owner.tracker_user),
                );
                return;
            }
        };
        if item.assignee.as_ref().map(|a| &a.id) != Some(&owner.id)
            && t.act(
                log,
                report,
                &item.key,
                "assign the owner",
                json!({}),
                || {
                    t.tracker
                        .assign(&item.key, Some(&owner))
                        .map_err(|e| e.to_string())
                },
            )
            .is_none()
        {
            return;
        }
    }
    let hold_label = &release.escalation_label;
    if !item.labels.iter().any(|l| l == hold_label)
        && t.act(
            log,
            report,
            &item.key,
            &format!("ensure label {hold_label}"),
            json!({}),
            || {
                t.tracker
                    .ensure_label(hold_label)
                    .map_err(|e| e.to_string())
            },
        )
        .is_some()
    {
        t.act(
            log,
            report,
            &item.key,
            &format!("label {hold_label}"),
            json!({}),
            || {
                t.tracker
                    .add_label(&item.key, hold_label)
                    .map_err(|e| e.to_string())
            },
        );
    }
    if said {
        return;
    }
    let text = texts::escalation_comment(&c.tag, c.pass(), max, &d.blockers, moved);
    // With no channel for it, the comment alone is the notice.
    let sent = t.notify(log, report, &item.key, "escalation", json!({}), &text);
    if sent.is_some() {
        let body = for_tracker(t, &text);
        t.act(
            log,
            report,
            &item.key,
            "comment escalation",
            json!({}),
            || {
                t.tracker
                    .comment(&item.key, &body)
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            },
        );
    }
}

/// A candidate waiting longer than `stagnation_hours` is said once.
fn stagnation(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    item: &Item,
    c: &Candidate,
    d: &go::GoDecision,
) {
    let release = section(t);
    let hours = release.stagnation_hours;
    let Some(tagged) = c.tagged_at else {
        return;
    };
    if t.now.0 - tagged.0 < time::Duration::hours(i64::from(hours)) {
        return;
    }
    let marker = texts::marker("stagnation", &[("candidate", &c.tag)]);
    match tracker_said(t, &item.key, &marker) {
        Ok(false) => {}
        _ => return,
    }
    let gate_red = (!d.gate_green).then(|| "the gate is not green".to_string());
    let lists: Vec<(&str, Vec<String>)> = vec![
        ("Open angles", d.open_angles.clone()),
        ("Angles not offered", d.missing_angles.clone()),
        ("Blockers", d.blockers.clone()),
    ];
    let text = texts::stagnation_message(
        &c.tag,
        hours,
        gate_red.as_deref(),
        &lists,
        &d.reasons,
        &item.key,
        &item.url,
    );
    // With no channel for it, the comment alone is the notice.
    let sent = t.notify(log, report, &item.key, "stagnation", json!({}), &text);
    if sent.is_some() {
        let body = for_tracker(t, &format!("{text}\n\n{marker}"));
        t.act(
            log,
            report,
            &item.key,
            "comment stagnation",
            json!({}),
            || {
                t.tracker
                    .comment(&item.key, &body)
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            },
        );
    }
}
