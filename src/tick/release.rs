//! The release pass (`docs/spec/05-release-cycle.md`, spec 07 step 2), for
//! the newest candidate of the product:
//!
//! 1. the release item, found by its label and marker or created;
//! 2. the candidate's record (comment and `## Candidates` line), the
//!    embarked tickets labelled; once the candidate's gate is green, and
//!    never before, the angle sub-items and the consolidation item of the
//!    candidate, the consolidation item blocked by every angle sub-item,
//!    then the brief (BR-ASDLC-004, BR-ASDLC-020); a candidate with no QA
//!    angle (`release.angles = []`) gets the brief alone; the consolidation item of
//!    an older candidate Done; a red gate whose automatic rerun settled gets
//!    its urgent gate-fix ticket, which a later green gate closes
//!    (BR-ASDLC-016);
//! 3. the angle PRs: each sub-item follows its PR (In Progress, In Review,
//!    back In Progress on changes requested); merged into the release
//!    branch when approved, green and carrying a valid report, the sub-item
//!    Done; the findings of a merged report filed;
//! 4. the GO, and on GO the release tag on the candidate and its GitHub
//!    release published, with the notes the cut prepared;
//! 5. otherwise the HOLD said once, the escalation when the passes are
//!    spent, the stagnation notice;
//! 6. once the release is published: the consolidation items Done, then the
//!    closure (`closure.rs`).
//!
//! Points the specification leaves open are not settled here; each waits,
//! as `docs/open-points.md` lists them (the human GO, a raised `max_rc_passes`).

use serde_json::json;

use crate::action_log::ActionLog;
use crate::config::{LaterPasses, Release};
use crate::github::{CompareStatus, Comparison, GitRef, ObjectType, PullRequest};
use crate::method::checks::{self, CheckState};
use crate::method::release::findings;
use crate::method::release::gate;
use crate::method::release::go::{self, AngleState, GoFacts, Policy};
use crate::method::release::prepare;
use crate::method::release::prs;
use crate::method::release::report::{self, Report as AngleReport};
use crate::method::release::shape::{Shape, Version};
use crate::method::review::{self, Approval, BranchState};
use crate::method::texts::{self, AngleLine, AngleScope, BridgeItem};
use crate::method::vocabulary::{self, Status};
use crate::tracker::{Item, NewItem, Priority, RelationKind};

use super::hands_off;
use super::merge::{for_tracker, tracker_said};
use super::{Report, Tick, closure};

/// The candidate in flight, as the tags say.
pub struct Candidate {
    pub tag: String,
    pub version: Version,
    pub rc: u64,
    pub sha: String,
    pub class: String,
    pub tagged_at: Option<crate::github::Timestamp>,
}

/// Why the release cycle has nothing to act on.
pub(crate) const NO_RELEASE_CYCLE: &str =
    "the configuration has no [release] section: the project has no release cycle";

/// The `[release]` section, or [`NO_RELEASE_CYCLE`]. Every reader handles
/// its absence, never by a panic (a host's daemon runs the bridge): the
/// pass does nothing, the verbs refuse, a step reached without it writes
/// nothing.
pub(crate) fn section<'a>(t: &Tick<'a>) -> Result<&'a Release, String> {
    t.config
        .release
        .as_ref()
        .ok_or_else(|| NO_RELEASE_CYCLE.to_string())
}

/// Run the pass. Returns whether it merged a PR (the tick's one merge).
/// Without a `[release]` section the project has no release cycle: nothing
/// is read or written (spec 05, "Without a release cycle").
pub fn pass(t: &Tick, log: &mut ActionLog, report: &mut Report) -> bool {
    let cfg = t.config;
    let Ok(release) = section(t) else {
        return false;
    };
    let shape = Shape::of(&cfg.product.name, release);
    let (candidate, tags) = match newest(t, &shape) {
        Ok(Some(found)) => found,
        Ok(None) => return false,
        Err(e) => {
            t.unreadable(report, "the release candidate", e);
            return false;
        }
    };
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
        match find_release_item(t, &shape, version) {
            Ok(Some(_)) => {}
            Ok(None) => return false,
            Err(e) => {
                t.unreadable(report, &object, e);
                return false;
            }
        }
    }
    let Some(item) = release_item(t, log, report, &shape, version) else {
        return false;
    };
    if item.state.is_resolved() {
        // A closed cycle: what its archiving left is retried (BR-ASDLC-019).
        if let Some(published) = &published {
            closure::archive_closed(t, log, report, &shape, &candidate, &item, published);
        }
        return false;
    }
    if let Some(published) = published {
        // The release published ends every overall review of the cycle
        // (BR-ASDLC-020); the closure stays the fallback.
        match t.tracker.children(&item.key) {
            Ok(children) => end_consolidations(t, log, report, &children, None),
            Err(e) => t.unreadable(report, &item.key, format!("children: {e}")),
        }
        closure::close(t, log, report, &shape, &candidate, &item, &published);
        return false;
    }
    let branch = match release_branch(t, &shape, &candidate) {
        Ok(b) => b,
        Err(e) => {
            t.unreadable(report, &object, e);
            return false;
        }
    };
    let label = vocabulary::release_label(&cfg.product.name, &version.to_string());
    let previous = previous_candidate(&shape, &tags, &candidate);
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
    if recorded || label_embarked(t, log, report, &shape, &candidate, &label) {
        record_candidate(t, log, report, &item, &candidate, previous.as_deref());
    }
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
        &shape,
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
        &shape,
        &item,
        &candidate,
        previous.as_deref(),
        gate,
        without_angles,
    );
    let children = match round {
        Round::Wait => match t.tracker.children(&item.key) {
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
    let angle_prs = match t.github.open_pulls(&t.repo, Some(&branch)) {
        Ok(p) => p,
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
        if cand != candidate.tag || t.by_hand(report, pr) {
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
    file_merged_findings(
        t,
        log,
        report,
        &item,
        &candidate,
        &branch,
        &label,
        &children,
        &mut states,
    );

    if merged {
        return true;
    }
    // A later pass under the lighter policy offers no angle: its GO counts
    // the angle PRs merged in the first pass (spec 05 §7-8).
    let (states, without_angles) = if !without_angles && lighter_pass(release, &candidate) {
        match first_pass_angles(t, &shape, &item, &candidate, &branch, &angle_prs) {
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
    ) else {
        return false;
    };
    if decision.go {
        on_go(t, log, report, &shape, &item, &candidate);
        return false;
    }
    hold(t, log, report, &item, &candidate, &decision);
    escalate(t, log, report, &item, &candidate, &decision, &branch);
    stagnation(t, log, report, &item, &candidate, &decision);
    false
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
    })
}

fn previous_candidate(shape: &Shape, tags: &[GitRef], c: &Candidate) -> Option<String> {
    tags.iter()
        .filter_map(|r| {
            shape
                .parse_candidate(&r.name)
                .map(|(v, rc)| (r.name.clone(), v, rc))
        })
        .filter(|(_, v, rc)| *v == c.version && *rc < c.rc)
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
        let cmp = t
            .github
            .compare(&t.repo, &c.sha, &b.name)
            .map_err(|e| e.to_string())?;
        if matches!(cmp.status, CompareStatus::Ahead | CompareStatus::Identical) {
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
    let found = match t.tracker.list_by_label(&label) {
        Ok(items) => items,
        Err(e) => {
            t.unreadable(report, &object, format!("the items labelled {label}: {e}"));
            return None;
        }
    };
    let mut mine: Vec<Item> = found
        .into_iter()
        .filter(|i| texts::bridge_item(&i.description, i.parent.as_deref()).as_ref() == Some(&want))
        .collect();
    match mine.len() {
        1 => return Some(mine.remove(0)),
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
    let Ok(release) = section(t) else {
        return None;
    };
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
    t.act(
        log,
        report,
        &object,
        "create release item",
        json!({"label": label}),
        || {
            t.tracker
                .create_item(&NewItem {
                    title: texts::release_title(product, &v),
                    description: description.clone(),
                    labels: vec![label.clone()],
                    status: Status::Ready,
                    priority: Some(Priority::Urgent),
                    assignee: None,
                    parent: None,
                })
                .map_err(|e| e.to_string())
        },
    )
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
    let mut mine: Vec<Item> = t
        .tracker
        .list_by_label(&label)
        .map_err(|e| format!("the items labelled {label}: {e}"))?
        .into_iter()
        .filter(|i| texts::bridge_item(&i.description, i.parent.as_deref()).as_ref() == Some(&want))
        // An abandoned cycle's item: the next cycle of the version gets its own.
        .filter(|i| i.state.status != Some(Status::Canceled))
        .collect();
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
            let paths = match previous.map(|p| t.github.compare(&t.repo, p, &c.sha)) {
                None => Vec::new(),
                Some(Ok(cmp)) => cmp.files,
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
            let scope = match section(t) {
                Err(_) => AngleScope::NoQaAngle,
                Ok(r) if r.angles.is_empty() => AngleScope::NoQaAngle,
                Ok(r) if lighter_pass(r, c) => AngleScope::LighterPass,
                Ok(_) => AngleScope::Configured,
            };
            let offered = offered(t, c);
            let body = for_tracker(
                t,
                &texts::candidate_comment(
                    &c.tag, &c.class, previous, None, &paths, &offered, scope,
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
    Ok(section(t)?.angles.is_empty()
        || tracker_said(t, &item.key, &texts::no_qa_angle_marker(&c.tag))?)
}

/// Whether the candidate is a later pass under the lighter policy
/// (`later_passes = "gate-and-coordinator"`): it offers no angle, and its GO
/// counts the angles of the first pass (spec 05 §7-8).
pub fn lighter_pass(release: &Release, c: &Candidate) -> bool {
    c.rc > 1 && release.later_passes == LaterPasses::GateAndCoordinator
}

/// The angles offered on this candidate (spec 05 §7, `later_passes`).
pub fn offered(t: &Tick, c: &Candidate) -> Vec<String> {
    let Ok(release) = section(t) else {
        return Vec::new();
    };
    if c.rc > u64::from(release.max_rc_passes) || lighter_pass(release, c) {
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
    let release = section(t)?;
    let first = shape.candidate_tag(c.version, 1);
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
        let pulls = t
            .github
            .commit_pulls(&t.repo, &commit.sha)
            .map_err(|e| e.to_string())?;
        let Some(pr) = pulls
            .iter()
            .find(|p| p.merge_commit_sha.as_deref() == Some(commit.sha.as_str()))
        else {
            continue;
        };
        if pr.head_ref.starts_with(&shape.branch_prefix) {
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
        for sub in split.sub_items {
            if !out.sub_items.contains(&sub.key) {
                out.sub_items.push(sub.key);
            }
        }
        for ticket in split.tickets {
            // Nothing to deploy: its merge delivered it, no release embarks
            // it (spec 04 §10).
            if super::merge::nothing_to_deploy(t, &ticket) {
                continue;
            }
            match out.tickets.iter_mut().find(|(k, _)| *k == ticket.key) {
                Some((_, prs)) => prs.push(pr.number),
                None => out.tickets.push((ticket.key, vec![pr.number])),
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
    let release = section(t).ok()?;
    let mut children = match t.tracker.children(&item.key) {
        Ok(c) => c,
        Err(e) => {
            t.unreadable(report, &item.key, format!("children: {e}"));
            return None;
        }
    };
    let product = &t.config.product.name;
    let v = c.version.to_string();
    for angle in &release.angles {
        if !offered.contains(&angle.key) {
            continue;
        }
        let line = AngleLine {
            key: angle.key.clone(),
            title: angle.title.clone(),
            assignee: angle.assignee.clone(),
        };
        let path = report::report_path(&release.record, &c.tag, &angle.key);
        let (title, body) = texts::release_angle_item(
            product,
            &v,
            &c.tag,
            c.rc,
            &line,
            branch,
            &path,
            label,
            angle.notes.as_deref(),
        );
        // Found by its marker or by its exact title (REQ-REL-048): a marker
        // an agent edited does not make the item invisible.
        if children.iter().any(|ch| {
            angle_of(ch, &c.tag).as_deref() == Some(angle.key.as_str()) || ch.title == title
        }) {
            continue;
        }
        let assignee = angle.assignee.as_ref().and_then(|a| t.tracker.user(a).ok());
        if let Some(created) =
            create_child(t, log, report, &item.key, &title, &body, label, assignee)
        {
            children.push(created);
        }
    }
    let (title, body) = texts::consolidation_item(
        &c.tag,
        &format!("{product} {v} (rc.{})", c.rc),
        texts::Consolidating::Candidate,
    );
    let consolidated = children.iter().any(|ch| {
        texts::bridge_item(&ch.description, ch.parent.as_deref())
            == Some(BridgeItem::Consolidation {
                scope: c.tag.clone(),
            })
            || ch.title == title
    });
    if !offered.is_empty() && !consolidated {
        let assignee = coordinator(t);
        if let Some(created) =
            create_child(t, log, report, &item.key, &title, &body, label, assignee)
        {
            children.push(created);
        }
    }
    let blocked = block_consolidation(t, log, report, &children, &c.tag);
    Some((children, blocked))
}

/// The candidate's consolidation item, while open, is blocked by every
/// angle sub-item of the candidate (BR-ASDLC-020), through the same
/// relation as a PR round's (BR-ASDLC-018): it is served only once every
/// angle PR is merged. A relation already there is not written again; one
/// that could not be read or written is reported and retried next tick, and
/// `false` is returned so that the brief waits for it.
fn block_consolidation(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    children: &[Item],
    candidate: &str,
) -> bool {
    let want = Some(BridgeItem::Consolidation {
        scope: candidate.to_string(),
    });
    let Some(consolidation) = children.iter().find(|ch| {
        !ch.state.is_resolved() && texts::bridge_item(&ch.description, ch.parent.as_deref()) == want
    }) else {
        return true;
    };
    let angles: Vec<String> = children
        .iter()
        .filter(|ch| angle_of(ch, candidate).is_some())
        .map(|ch| ch.key.clone())
        .collect();
    super::merge::block_consolidation(t, log, report, &consolidation.key, &angles)
}

/// The consolidation items of the release item ended by a later fact
/// (BR-ASDLC-020): every one but the `current` candidate's goes Done, since
/// a newer candidate was tagged (the overall review cut it); with no
/// `current`, the release is published and every one goes Done. The
/// cycle's closure still closes what is left (spec 05 §11).
fn end_consolidations(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    children: &[Item],
    current: Option<&str>,
) {
    for ch in children {
        let Some(BridgeItem::Consolidation { scope }) =
            texts::bridge_item(&ch.description, ch.parent.as_deref())
        else {
            continue;
        };
        if ch.state.is_resolved() || Some(scope.as_str()) == current {
            continue;
        }
        let why = match current {
            Some(newer) => format!("status Done (newer candidate {newer})"),
            None => "status Done (release published)".to_string(),
        };
        t.act(
            log,
            report,
            &ch.key,
            &why,
            json!({"candidate": scope}),
            || {
                t.tracker
                    .set_status(&ch.key, Status::Done, None)
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
    let body = for_tracker(t, body);
    t.act(
        log,
        report,
        parent,
        &format!("create \"{title}\""),
        json!({}),
        || {
            t.tracker
                .create_item(&NewItem {
                    title: title.to_string(),
                    description: body.clone(),
                    labels: vec![label.to_string()],
                    status: Status::Ready,
                    priority: Some(Priority::Urgent),
                    assignee: assignee.clone(),
                    parent: Some(parent.to_string()),
                })
                .map_err(|e| e.to_string())
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
    let (last, cmp) = release_range(t, shape, c)?;
    let paths_since_release = cmp.files;
    let paths_since_candidate = match previous {
        Some(p) => Some(
            t.github
                .compare(&t.repo, p, &c.sha)
                .map_err(|e| e.to_string())?
                .files,
        ),
        None => None,
    };
    let mut tickets = Vec::new();
    for (key, prs) in embarked(t, shape, c)? {
        let item = t.tracker.get(&key).map_err(|e| e.to_string())?;
        tickets.push(texts::BriefTicket {
            id: key,
            title: item.title,
            state: item.state.name,
            prs,
            description: Some(item.description),
        });
    }
    let rule_paths = &section(t)?.business_rule_paths;
    let rule_files_touched = crate::method::glob::hits(&paths_since_release, rule_paths);
    let evidence_url = evidence_url(t, c, run)?;
    Ok(texts::Brief {
        candidate: c.tag.clone(),
        sha: c.sha.clone(),
        class: c.class.clone(),
        last_release: last,
        previous_candidate: previous.map(str::to_string),
        paths_since_release,
        paths_since_candidate,
        gate_url: Some(run.url.clone()),
        evidence_url: Some(evidence_url),
        tickets,
        untracked: Vec::new(),
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
        .workflow_runs(
            &t.repo,
            &section(t)?.gate_workflow,
            None,
            Some(&c.sha),
            None,
        )
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
    let (Some(first), Ok(release)) = (gate, section(t)) else {
        return;
    };
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
    let children = match t.tracker.children(&item.key) {
        Ok(ch) => ch,
        Err(e) => {
            t.unreadable(report, &item.key, format!("children: {e}"));
            return;
        }
    };
    let marker = texts::gate_fix_marker(&c.tag);
    let opened: Vec<&Item> = children
        .iter()
        .filter(|ch| texts::has_marker(&ch.description, &marker))
        .collect();
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
    // completed run made is then seen on the gate run.
    let rerun_workflow = &release.gate_rerun_workflow;
    let reruns = match t.github.workflow_runs(
        &t.repo,
        rerun_workflow,
        None,
        Some(&c.sha),
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
    let facts = texts::RedGateFacts {
        release: &item.key,
        candidate: &c.tag,
        sha: &c.sha,
        branch,
        conclusion: run.conclusion.as_deref().unwrap_or_default(),
        run_url: &run.url,
        suites: failed_suites_of(t, c, &run),
        rerun_unseen: unseen,
    };
    let (title, body) = texts::gate_fix_ticket(&facts);
    let body = for_tracker(t, &body);
    let assignee = fixer(t);
    t.act(
        log,
        report,
        &item.key,
        "create gate-fix ticket",
        json!({"candidate": c.tag, "run": run.url, "attempt": run.attempt, "priority": "urgent", "rerun_seen": unseen.is_none(), "assignee": assignee.as_ref().map(|u| u.name.as_str())}),
        || {
            t.tracker
                .create_item(&NewItem {
                    title: title.clone(),
                    description: body.clone(),
                    labels: vec![label.to_string()],
                    status: Status::Ready,
                    priority: Some(Priority::Urgent),
                    assignee: assignee.clone(),
                    parent: Some(item.key.clone()),
                })
                .map_err(|e| e.to_string())
        },
    );
}

/// The participant `release.fix_assignee` names, who fixes release
/// problems (a red gate, a blocking QA finding), when the tracker knows it;
/// otherwise the ticket is unassigned, taken by role (`doctor` reports an
/// unknown one).
pub(crate) fn fixer(t: &Tick) -> Option<crate::tracker::User> {
    let name = section(t).ok()?.fix_assignee.as_ref()?;
    t.tracker.user(name).ok()
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
    let children = match t.tracker.children(&item.key) {
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
    let Ok(release) = section(t) else {
        return AngleState::InReview;
    };
    let required = match t.github.required_checks(&t.repo, branch) {
        Ok(r) => r,
        Err(e) => {
            t.unreadable(report, &object, e);
            return AngleState::InReview;
        }
    };
    let green = checks::pr_state(pr, &required.contexts) == CheckState::Green;
    follow_angle_pr(t, log, report, pr, sub, angle, green);
    let path = report::report_path(&release.record, &c.tag, angle);
    let valid = match t.github.file(&t.repo, &path, &pr.head_sha) {
        Ok(text) => report::validate(&text, &c.tag, angle).map_err(|e| e.join("; ")),
        Err(e) if e.is_not_found() => Err(format!("no report at `{path}`")),
        Err(e) => {
            t.unreadable(report, &object, e);
            return AngleState::InReview;
        }
    };
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
    if *merged || pr.draft || !prs::names_sub_item(pr, &sub.key) {
        return AngleState::InReview;
    }
    // An angle PR carries its report and nothing else (BR-ASDLC-022): an
    // angle fixes nothing, so a PR with any other file is not merged, and
    // is told once per head. The PR's own changes are those since it left
    // the release branch.
    match t.github.compare(&t.repo, branch, &pr.head_sha) {
        Ok(cmp) => {
            let others: Vec<&String> = cmp.files.iter().filter(|f| **f != path).collect();
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
        }
        Err(e) => {
            t.unreadable(
                report,
                &object,
                format!("the changes of the angle PR against {branch}: {e}"),
            );
            return AngleState::InReview;
        }
    }
    let Approval::Qualified { by } = review::pr_approval(pr, &release.coordinators) else {
        return AngleState::InReview;
    };
    if !green {
        return AngleState::InReview;
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
    // As for any PR (spec 04 §6): a person's standing request for changes
    // holds it; an App's is superseded by the approval at the head and
    // dismissed right before the merge.
    let dismissals = match super::merge::superseded(pr, &release.coordinators) {
        Ok(d) => d,
        Err(why) => {
            t.wait(report, pr, why);
            return AngleState::InReview;
        }
    };
    if !super::merge::dismiss(t, log, report, &object, pr, &by, &dismissals) {
        return AngleState::InReview;
    }
    let done = t.act(
        log,
        report,
        &object,
        "merge (merge commit) into the release branch",
        json!({"head": pr.head_sha, "approved_by": by, "angle": angle}),
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
    t.act(
        log,
        report,
        &sub.key,
        "status Done (angle merged)",
        json!({"pr": pr.url}),
        || {
            t.tracker
                .set_status(&sub.key, Status::Done, None)
                .map_err(|e| e.to_string())
        },
    );
    AngleState::Merged
}

/// A release angle sub-item follows its open angle PR as a ticket follows
/// its PR (BR-ASDLC-020): In Progress when the PR appears, In Review once it
/// is ready with its required checks green, back In Progress when a
/// configured coordinator requests changes at its head; Done comes from the
/// merge. In Review assigns `release.coordinator_assignee`, who is then
/// served the PR's review; sent back, the item goes to its angle's
/// `assignee`, or to nobody (taken by role); both in the status's own write,
/// an unset or unknown coordinator leaving the assignee as it is. Only a PR
/// that names its sub-item drives it (the link the merge reads too), and
/// one that does not is told once per head which key to name; a status
/// already right is not written again.
fn follow_angle_pr(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    pr: &PullRequest,
    sub: &Item,
    angle: &str,
    green: bool,
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
        draft: pr.draft,
        green,
        sent_back: section(t)
            .is_ok_and(|r| review::changes_requested_at_head(pr, &r.coordinators).is_some()),
    };
    let Some(to) = prs::angle_item_status(sub.state.status, facts) else {
        return;
    };
    // Who is served, in the same write as the status: the coordinator to
    // review the PR; its angle again when the coordinator sent it back
    // (nobody named: taken by role); untouched when the work starts.
    let assignee: Option<Option<crate::tracker::User>> = match to {
        Status::InReview => coordinator(t).map(Some),
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
    let name = section(t).ok()?.coordinator_assignee.as_ref()?;
    t.tracker.user(name).ok()
}

/// The configured assignee of a release angle, when the tracker knows it.
fn angle_assignee(t: &Tick, angle: &str) -> Option<crate::tracker::User> {
    let name = section(t)
        .ok()?
        .angles
        .iter()
        .find(|a| a.key == angle)?
        .assignee
        .as_ref()?;
    t.tracker.user(name).ok()
}

/// The findings of every angle PR of the candidate merged into the release
/// branch, filed once each (spec 05 §5-6). Marks the merged angles.
#[allow(clippy::too_many_arguments)]
fn file_merged_findings(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    item: &Item,
    c: &Candidate,
    branch: &str,
    label: &str,
    children: &[Item],
    states: &mut [(String, AngleState)],
) {
    let Ok(release) = section(t) else {
        return;
    };
    let merged = match t.github.merged_pulls(&t.repo, Some(branch), None, 500) {
        Ok(m) => m,
        Err(e) => {
            t.unreadable(
                report,
                &item.key,
                format!("the merged PRs of {branch}: {e}"),
            );
            return;
        }
    };
    for pr in merged {
        if !t.in_time() {
            report.out_of_time = true;
            return;
        }
        let Some((cand, angle)) = prs::parse_angle_branch(&pr.head_ref) else {
            continue;
        };
        if cand != c.tag {
            continue;
        }
        if let Some(s) = states.iter_mut().find(|(a, _)| *a == angle) {
            s.1 = AngleState::Merged;
        }
        if let Some(sub) = children
            .iter()
            .find(|ch| angle_of(ch, &c.tag).as_deref() == Some(angle.as_str()))
        {
            if !sub.state.is_resolved() {
                t.act(
                    log,
                    report,
                    &sub.key,
                    "status Done (angle merged)",
                    json!({"pr": pr.url}),
                    || {
                        t.tracker
                            .set_status(&sub.key, Status::Done, None)
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
            Ok(r) => file_findings(t, log, report, item, c, &angle, &pr.url, &path, label, &r),
            Err(e) => t.unreadable(
                report,
                &item.key,
                format!("the merged report of {}: {e}", pr.url),
            ),
        }
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
    r: &AngleReport,
) {
    let labelled = match t.tracker.list_by_label(label) {
        Ok(l) => l,
        Err(e) => {
            t.unreadable(report, &item.key, e);
            return;
        }
    };
    let blocked_by: Vec<String> = match t.tracker.relations(&item.key) {
        Ok(r) => r.blocked_by.into_iter().map(|b| b.other).collect(),
        Err(e) => {
            t.unreadable(report, &item.key, e);
            return;
        }
    };
    let release_id = item.key.as_str();
    // Read once, and only when a blocking finding is filed.
    let mut fixer_of: Option<Option<crate::tracker::User>> = None;
    for f in &r.findings {
        // Filed once, whatever became of it: a finding canceled as a false
        // alarm or requalified stays so (the merged report does not
        // change). A new candidate's report is another key.
        let filed = labelled.iter().find(|i| {
            findings::parse_finding_marker(&i.description).is_some_and(|(rel, cand, a, id)| {
                rel == release_id && cand == c.tag && a == angle && id == f.id
            })
        });
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
        };
        let created = t.act(
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
            || t.tracker.create_item(&new).map_err(|e| e.to_string()),
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
) -> Option<go::GoDecision> {
    let cfg = t.config;
    let release = match section(t) {
        Ok(r) => r,
        Err(e) => {
            t.unreadable(report, &item.key, e);
            return None;
        }
    };
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
    let mut blockers = Vec::new();
    for b in &relations.blocked_by {
        match t.tracker.get(&b.other) {
            Ok(i) if i.state.is_resolved() => {}
            Ok(_) => blockers.push(b.other.clone()),
            Err(e) => {
                t.unreadable(report, &b.other, e);
                return None;
            }
        }
    }
    let label = vocabulary::release_label(&cfg.product.name, &c.version.to_string());
    let unlinked_blockers: Vec<String> = match t.tracker.list_by_label(&label) {
        Ok(items) => items
            .into_iter()
            .filter(|i| !i.state.is_resolved())
            .filter(|i| {
                findings::parse_blocker_marker(&i.description).as_deref() == Some(item.key.as_str())
            })
            .filter(|i| !relations.blocked_by.iter().any(|b| b.other == i.key))
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
            let moved: Vec<String> = cmp
                .files
                .into_iter()
                .filter(|f| !bookkeeping(release, &c.tag, f))
                .collect();
            if moved.is_empty() {
                Vec::new()
            } else {
                vec![format!(
                    "{branch} moved past {} ({} file(s)): tag the next candidate",
                    c.tag,
                    moved.len()
                )]
            }
        }
        Err(e) => vec![format!(
            "{branch} could not be compared with {}: {e}",
            c.tag
        )],
    };
    let policy = match release.go_policy {
        crate::config::GoPolicy::Computed => Policy::Computed,
        // Whose approval counts is the owner's to settle (`docs/open-points.md` #3): never given.
        crate::config::GoPolicy::Human => Policy::Human { approved: None },
    };
    let facts = GoFacts {
        release: item.key.clone(),
        gate_green,
        angles: states.to_vec(),
        no_qa_angle,
        blockers,
        unlinked_blockers,
        fix_gaps: Vec::new(),
        tip_gaps,
        policy,
    };
    Some(go::evaluate(&facts))
}

/// Whether `path` may differ between the candidate and the release branch
/// tip (REQ-REL-070): a configured bookkeeping path (exact), or the report
/// of one of the candidate's own angles (`release.record` rendered for
/// `candidate`).
fn bookkeeping(release: &Release, candidate: &str, path: &str) -> bool {
    release.bookkeeping_paths.iter().any(|b| b == path)
        || release
            .angles
            .iter()
            .any(|a| report::report_path(&release.record, candidate, &a.key) == path)
}

/// On GO (spec 05 §9): the release tag on the candidate's commit, then the
/// GitHub release of that tag, its notes the CHANGELOG section the cut
/// prepared on the release branch. Publishing the release starts the
/// publication workflow. Each step is read before it is written, so a tag
/// or a release a person made by hand (spec 09) is taken as done, never
/// made again; a tag on another commit stops the GO, fail-closed. The
/// release item under the hands-off label means a person takes the GO: the
/// bridge tags nothing.
fn on_go(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    shape: &Shape,
    item: &Item,
    c: &Candidate,
) {
    if hands_off::carries(&item.labels, &t.config.bridge.hands_off_label) {
        t.hold_by_hand(report, &item.key);
        return;
    }
    let tag = shape.release_tag(c.version);
    let notes = match t.github.file(&t.repo, crate::prepare::CHANGELOG, &c.sha) {
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
    let existing = match release_tag_commit(t, &tag) {
        Ok(e) => e,
        Err(e) => {
            t.unreadable(report, &item.key, format!("the tag {tag}: {e}"));
            return;
        }
    };
    match existing {
        Some(sha) if sha == c.sha => {}
        Some(sha) => {
            t.unreadable(
                report,
                &item.key,
                format!(
                    "{tag} exists on {}, not on the candidate {} ({}): the bridge publishes no release of it",
                    texts::short(&sha),
                    c.tag,
                    texts::short(&c.sha)
                ),
            );
            return;
        }
        None => {
            let message = format!("Release {tag}\n\nCandidate: {}", c.tag);
            let made = t.act(
                log,
                report,
                &item.key,
                &format!("tag {tag} (GO)"),
                json!({"candidate": c.tag, "sha": c.sha}),
                || {
                    t.github
                        .create_tag(&t.repo, &tag, &c.sha, &message)
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

/// The commit the release tag names, or `None` when it does not exist.
fn release_tag_commit(t: &Tick, tag: &str) -> Result<Option<String>, String> {
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
/// answered and something the QA found holds the release (REQ-REL-069).
fn hold(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    item: &Item,
    c: &Candidate,
    d: &go::GoDecision,
) {
    if !d.every_angle_answered() || (d.blockers.is_empty() && d.fix_gaps.is_empty()) {
        return;
    }
    let key = texts::hold_key(&d.blockers, &d.fix_gaps);
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
fn escalate(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    item: &Item,
    c: &Candidate,
    d: &go::GoDecision,
    _branch: &str,
) {
    let Ok(release) = section(t) else {
        return;
    };
    let max = release.max_rc_passes;
    let moved = !d.tip_gaps.is_empty();
    if c.rc < u64::from(max) || !d.every_angle_answered() || (d.blockers.is_empty() && !moved) {
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
    let text = texts::escalation_comment(&c.tag, c.rc, max, &d.blockers, moved);
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
    let Ok(release) = section(t) else {
        return;
    };
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
