//! The release pass (`docs/spec/05-release-cycle.md`, spec 07 step 2), for
//! the newest candidate of the product:
//!
//! 1. the release item, found by its label and marker or created;
//! 2. the candidate's record (comment and `## Candidates` line), the
//!    embarked tickets labelled; once the candidate's gate is green, and
//!    never before, the angle sub-items and the consolidation item of the
//!    candidate, then the brief (BR-ASDLC-004); a candidate with no QA
//!    angle (`release.angles = []`) gets the brief alone; a red gate whose automatic
//!    rerun settled gets its urgent gate-fix ticket, which a later green
//!    gate closes (BR-ASDLC-016);
//! 3. the angle PRs: merged into the release branch when approved, green and
//!    carrying a valid report; the findings of a merged report filed;
//! 4. the GO, and on GO the release-please PR merged;
//! 5. otherwise the HOLD said once, the escalation when the passes are
//!    spent, the stagnation notice;
//! 6. once the release is published: the closure (`closure.rs`).
//!
//! Points the specification leaves open are not settled here; each waits,
//! as `docs/open-points.md` lists them (the human GO, a raised `max_rc_passes`).

use serde_json::json;

use crate::action_log::ActionLog;
use crate::config::{LaterPasses, NitsGrouping, Release};
use crate::github::{CompareStatus, GitRef, ObjectType, PullRequest};
use crate::method::checks::{self, CheckState};
use crate::method::release::findings;
use crate::method::release::gate;
use crate::method::release::go::{self, AngleState, GoFacts, Policy};
use crate::method::release::notes;
use crate::method::release::prs;
use crate::method::release::report::{self, Report as AngleReport};
use crate::method::release::shape::{Shape, Version};
use crate::method::review::{self, Approval, BranchState};
use crate::method::texts::{self, AngleLine, AngleScope, BridgeItem};
use crate::method::vocabulary::{self, Severity, Status};
use crate::tracker::{Item, NewItem, Priority, RelationKind};

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
        return false;
    }
    if let Some(published) = published {
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
            .unwrap_or_default(),
        Round::Brief(b) => {
            // The round first, the brief last (REQ-REL-046).
            let opened = open_round(t, log, report, &item, &candidate, &branch, &label, &offered);
            if opened.is_some() {
                post_brief(t, log, report, &item, &candidate, &b);
            }
            opened.unwrap_or_default()
        }
    };

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
        if cand != candidate.tag {
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
    let decision = decide(
        t,
        report,
        &shape,
        &item,
        &candidate,
        &branch,
        &states,
        &angle_prs,
        without_angles,
    );
    let Some((decision, rp)) = decision else {
        return merged;
    };
    if decision.go {
        return on_go(
            t, log, report, &shape, &item, &candidate, &branch, &decision, &rp, &states,
        );
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
                Ok(r) if c.rc > 1 && r.later_passes == LaterPasses::GateAndCoordinator => {
                    AngleScope::LighterPass
                }
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
    let description = texts::append_candidate(&item.description, &line);
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

/// The angles offered on this candidate (spec 05 §7, `later_passes`).
pub fn offered(t: &Tick, c: &Candidate) -> Vec<String> {
    let Ok(release) = section(t) else {
        return Vec::new();
    };
    if c.rc > u64::from(release.max_rc_passes)
        || (c.rc > 1 && release.later_passes == LaterPasses::GateAndCoordinator)
    {
        return Vec::new();
    }
    release.angles.iter().map(|a| a.key.clone()).collect()
}

/// The commits of the range, each to its merged PR, each PR to its ticket;
/// the tickets of the team that are past the merge get the release label
/// (spec 05 §2, the brief: "found by commit → PR → linked ticket").
pub fn embarked(t: &Tick, shape: &Shape, c: &Candidate) -> Result<Vec<(String, Vec<u64>)>, String> {
    let tags = t
        .github
        .tags(&t.repo, shape.release_tag_prefix())
        .map_err(|e| e.to_string())?;
    let base = shape.previous_release(tags.iter().map(|r| r.name.as_str()), Some(c.version));
    let commits = match &base {
        Some((tag, _)) => {
            t.github
                .compare(&t.repo, tag, &c.sha)
                .map_err(|e| e.to_string())?
                .commits
        }
        None => return Ok(Vec::new()),
    };
    let team = t.tracker.team_key().map_err(|e| e.to_string())?;
    let mut keys = t.config.tracker.team_keys.clone();
    if keys.is_empty() {
        keys.push(team);
    }
    let mut out: Vec<(String, Vec<u64>)> = Vec::new();
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
        if pr.head_ref.starts_with(review::RELEASE_PLEASE_PREFIX)
            || pr.head_ref.starts_with(&shape.branch_prefix)
        {
            continue;
        }
        for key in t
            .tracker
            .items_linked_to_pr(&pr.url)
            .map_err(|e| e.to_string())?
        {
            if !keys.iter().any(|k| key.starts_with(&format!("{k}-"))) {
                continue;
            }
            match out.iter_mut().find(|(k, _)| *k == key) {
                Some((_, prs)) => prs.push(pr.number),
                None => out.push((key, vec![pr.number])),
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
/// created when missing (spec 05 §3, §4). Returns the release item's
/// children after the creations, or `None` when they could not be read.
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
) -> Option<Vec<Item>> {
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
        if children
            .iter()
            .any(|ch| angle_of(ch, &c.tag).as_deref() == Some(angle.key.as_str()))
        {
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
        let assignee = angle.assignee.as_ref().and_then(|a| t.tracker.user(a).ok());
        if let Some(created) =
            create_child(t, log, report, &item.key, &title, &body, label, assignee)
        {
            children.push(created);
        }
    }
    let consolidated = children.iter().any(|ch| {
        texts::bridge_item(&ch.description, ch.parent.as_deref())
            == Some(BridgeItem::Consolidation {
                scope: c.tag.clone(),
            })
    });
    if !offered.is_empty() && !consolidated {
        let (title, body) = texts::consolidation_item(
            &c.tag,
            &format!("{product} {v} (rc.{})", c.rc),
            texts::Consolidating::Candidate,
        );
        let assignee = release
            .coordinator_assignee
            .as_ref()
            .and_then(|a| t.tracker.user(a).ok());
        if let Some(created) =
            create_child(t, log, report, &item.key, &title, &body, label, assignee)
        {
            children.push(created);
        }
    }
    Some(children)
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
    let release_tags = t
        .github
        .tags(&t.repo, shape.release_tag_prefix())
        .map_err(|e| e.to_string())?;
    let last = shape
        .previous_release(
            release_tags.iter().map(|r| r.name.as_str()),
            Some(c.version),
        )
        .map(|(tag, _)| tag);
    let paths_since_release = match &last {
        Some(tag) => {
            t.github
                .compare(&t.repo, tag, &c.sha)
                .map_err(|e| e.to_string())?
                .files
        }
        None => Vec::new(),
    };
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
    t.act(
        log,
        report,
        &item.key,
        "create gate-fix ticket",
        json!({"candidate": c.tag, "run": run.url, "attempt": run.attempt, "priority": "urgent", "rerun_seen": unseen.is_none()}),
        || {
            t.tracker
                .create_item(&NewItem {
                    title: title.clone(),
                    description: body.clone(),
                    labels: vec![label.to_string()],
                    status: Status::Ready,
                    priority: Some(Priority::Urgent),
                    assignee: None,
                    parent: Some(item.key.clone()),
                })
                .map_err(|e| e.to_string())
        },
    );
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
    let Approval::Qualified { by } = review::pr_approval(pr, &release.coordinators) else {
        return AngleState::InReview;
    };
    let required = match t.github.required_checks(&t.repo, branch) {
        Ok(r) => r,
        Err(e) => {
            t.unreadable(report, &object, e);
            return AngleState::InReview;
        }
    };
    if checks::pr_state(pr, &required.contexts) != CheckState::Green {
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
            .and_then(|text| report::validate(&text, &c.tag, &angle).map_err(|e| e.join("; ")));
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
    let product = &t.config.product.name;
    let v = c.version.to_string();
    let Ok(release) = section(t) else {
        return;
    };
    let shared = release.nits == NitsGrouping::Shared;
    // The shared nits ticket, once found or created in this call: its key
    // and its description as this call last wrote it. `Err` once it is
    // refused (unreadable, or no contract): no nit of this call goes in.
    let mut nits: Option<Result<(String, String), ()>> = None;
    for f in r.findings.iter().filter(|f| !f.fixed_in_branch) {
        match f.severity {
            Severity::Nit if shared => {
                let title = findings::nits_title(product, &v);
                let key = findings::nit_key(&c.tag, angle, &f.id);
                let line = findings::nit_line(&c.tag, angle, f);
                if nits.is_none() {
                    if let Some(n) = labelled.iter().find(|i| i.title == title) {
                        nits = Some(match t.tracker.get(&n.key) {
                            Ok(i) => Ok((i.key, i.description)),
                            Err(e) => {
                                t.unreadable(report, &n.key, e);
                                Err(())
                            }
                        });
                    }
                }
                match &mut nits {
                    Some(Err(())) => {}
                    Some(Ok((key_of, current))) => {
                        // Fail-closed: a nits ticket without a contract (the
                        // older format) is left alone and said, never rewritten.
                        match findings::append_nit(current, &key, &line) {
                            Err(e) => {
                                t.unreadable(report, key_of, format!("the nits ticket {e}"));
                                nits = Some(Err(()));
                            }
                            Ok(None) => {}
                            Ok(Some(next)) => {
                                let next = for_tracker(t, &next);
                                let done = t.act(
                                    log,
                                    report,
                                    key_of,
                                    "append nit",
                                    json!({"finding": f.id}),
                                    || {
                                        t.tracker
                                            .update_description(key_of, &next)
                                            .map_err(|e| e.to_string())
                                    },
                                );
                                if done.is_some() {
                                    *current = next;
                                }
                            }
                        }
                    }
                    None => {
                        let body =
                            for_tracker(t, &findings::nits_ticket(&item.key, product, &v, &[line]));
                        let created = t.act(
                            log,
                            report,
                            &item.key,
                            "create nits ticket",
                            json!({}),
                            || {
                                t.tracker
                                    .create_item(&NewItem {
                                        title: title.clone(),
                                        description: body.clone(),
                                        labels: vec![label.to_string()],
                                        status: Status::Triage,
                                        priority: None,
                                        assignee: None,
                                        parent: None,
                                    })
                                    .map_err(|e| e.to_string())
                            },
                        );
                        if let Some(n) = created {
                            relate(t, log, report, &n.key, &item.key, RelationKind::Related);
                            nits = Some(Ok((n.key, body)));
                        }
                    }
                }
            }
            // A blocker, a should-fix, or a nit under `release.nits =
            // "per-nit"`: a ticket of its own, keyed by its finding marker.
            _ => {
                let release_id = item.key.as_str();
                // Filed once, whatever became of it: a blocker canceled as a
                // false alarm or requalified stays so (the merged report does
                // not change). A new candidate's report is another key.
                let filed = labelled.iter().find(|i| {
                    findings::parse_finding_marker(&i.description).is_some_and(
                        |(rel, cand, a, id)| {
                            rel == release_id && cand == c.tag && a == angle && id == f.id
                        },
                    )
                });
                if let Some(ticket) = filed {
                    // A blocker whose relation failed after its creation: the
                    // relation is laid again (REQ-REL-058).
                    let marked = findings::parse_blocker_marker(&ticket.description).as_deref()
                        == Some(release_id);
                    if marked && !ticket.state.is_resolved() && !blocked_by.contains(&ticket.key) {
                        relate(t, log, report, &ticket.key, &item.key, RelationKind::Blocks);
                    }
                    continue;
                }
                let (title, body) = if f.severity == Severity::Nit {
                    findings::nit_ticket(release_id, &c.tag, angle, f, pr_url, path)
                } else {
                    findings::finding_ticket(release_id, &c.tag, angle, f, pr_url, path)
                };
                let blocker = f.severity == Severity::Blocker;
                let mut labels = vec![label.to_string()];
                if f.severity == Severity::ShouldFix {
                    labels.push(vocabulary::QA_FINDING_LABEL.to_string());
                }
                let body = for_tracker(t, &body);
                let created = t.act(
                    log,
                    report,
                    &item.key,
                    &format!("create finding {}", f.id),
                    json!({"severity": f.severity.name(), "angle": angle}),
                    || {
                        t.tracker
                            .create_item(&NewItem {
                                title: title.clone(),
                                description: body.clone(),
                                labels: labels.clone(),
                                status: if blocker {
                                    Status::Ready
                                } else {
                                    Status::Triage
                                },
                                priority: blocker.then_some(Priority::Urgent),
                                assignee: None,
                                parent: None,
                            })
                            .map_err(|e| e.to_string())
                    },
                );
                if let Some(ticket) = created {
                    if blocker {
                        relate(t, log, report, &ticket.key, &item.key, RelationKind::Blocks);
                    } else {
                        relate(
                            t,
                            log,
                            report,
                            &ticket.key,
                            &item.key,
                            RelationKind::Related,
                        );
                    }
                }
            }
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

/// A release PR of the cycle, in merge order.
#[derive(Debug, Clone)]
pub struct ReleasePr {
    /// The component, for a multi-component product.
    pub component: Option<String>,
    pub version: Version,
    pub pr: PullRequest,
}

/// The release PRs open against the release branch, in merge order (spec 05
/// §9: the version component last), with the gaps of their notes and of
/// their staged version.
fn release_prs(
    t: &Tick,
    shape: &Shape,
    c: &Candidate,
    branch: &str,
    open: &[PullRequest],
) -> (Vec<String>, Vec<String>, Vec<ReleasePr>) {
    let cfg = t.config;
    let Ok(release) = section(t) else {
        return (vec![NO_RELEASE_CYCLE.to_string()], Vec::new(), Vec::new());
    };
    let rp_path = release.release_please_config.as_str();
    let rp_config = |pr: &PullRequest| t.github.file(&t.repo, rp_path, &pr.head_sha);
    // The range, read once: unreadable is a gap, never an empty range.
    let range: Result<Vec<crate::github::CommitSummary>, String> = t
        .github
        .tags(&t.repo, shape.release_tag_prefix())
        .map_err(|e| e.to_string())
        .and_then(|tags| {
            match shape.previous_release(tags.iter().map(|r| r.name.as_str()), Some(c.version)) {
                Some((tag, _)) => t
                    .github
                    .compare(&t.repo, &tag, &c.sha)
                    .map(|cmp| cmp.commits)
                    .map_err(|e| e.to_string()),
                // The first release: no previous tag bounds it.
                None => Ok(Vec::new()),
            }
        });
    let commits = match &range {
        Ok(list) => Ok(list.as_slice()),
        Err(_) => Err("the range could not be read"),
    };
    let coverage = |pr: &PullRequest, version: Version, changelog_path: &str| {
        let changelog = t.github.file(&t.repo, changelog_path, &pr.head_sha).ok();
        notes::coverage(
            version,
            changelog.as_deref(),
            rp_config(pr).ok().as_deref(),
            commits,
        )
    };
    if !shape.multi_component() {
        return match prs::release_please_pr(
            open,
            branch,
            &release.release_please_authors,
            &release.release_pr_label,
        ) {
            Err(e) => (vec![e.to_string()], Vec::new(), Vec::new()),
            Ok(lookup) => match lookup.pr {
                None => (
                    vec!["no release-please PR is open".into()],
                    Vec::new(),
                    Vec::new(),
                ),
                Some(pr) => {
                    let note = coverage(pr, c.version, "CHANGELOG.md");
                    let staged = notes::staged_version(&pr.body, &cfg.product.name)
                        .or_else(|| notes::staged_version(&pr.title, &cfg.product.name));
                    let version_gaps = match staged {
                        Some(v) if v == c.version => Vec::new(),
                        Some(v) => vec![format!(
                            "the release PR stages {v}, the QA judged {}",
                            c.version
                        )],
                        None => {
                            vec!["the staged version could not be read from the release PR".into()]
                        }
                    };
                    let rp = staged
                        .map(|v| ReleasePr {
                            component: None,
                            version: v,
                            pr: pr.clone(),
                        })
                        .into_iter()
                        .collect();
                    (note.gaps(), version_gaps, rp)
                }
            },
        };
    }
    let found = match prs::component_prs(
        open,
        shape,
        branch,
        &release.release_please_authors,
        &release.release_pr_label,
    ) {
        Ok(f) => f,
        Err(e) => return (vec![e], Vec::new(), Vec::new()),
    };
    if found.is_empty() {
        return (
            vec!["no release-please PR is open".into()],
            Vec::new(),
            Vec::new(),
        );
    }
    let version_component = shape.version_component.clone().unwrap_or_default();
    let version_gaps = match found.iter().find(|f| f.component == version_component) {
        Some(f) if f.version == c.version => Vec::new(),
        Some(f) => vec![format!(
            "the release PR of {version_component} stages {}, the QA judged {}",
            f.version, c.version
        )],
        None => vec![format!(
            "no release PR of the version component {version_component} is open"
        )],
    };
    // A commit of the range is covered when one component's note names it.
    let mut missing: Option<Vec<crate::github::CommitSummary>> = None;
    let mut unreadable = None;
    for f in &found {
        // Where each component's note lives is read from release-please's
        // configuration: unreadable is a gap, never the root changelog.
        let config = match rp_config(f.pr) {
            Ok(c) => c,
            Err(e) => {
                unreadable.get_or_insert(format!("{}: {rp_path}: {e}", f.component));
                continue;
            }
        };
        let path = changelog_path(Some(config.as_str()), &f.component);
        let cov = coverage(f.pr, f.version, &path);
        if let Some(why) = cov.unreadable {
            unreadable.get_or_insert(format!("{}: {why}", f.component));
            continue;
        }
        missing = Some(match missing {
            None => cov.missing,
            Some(prev) => prev
                .into_iter()
                .filter(|m| cov.missing.iter().any(|x| x.sha == m.sha))
                .collect(),
        });
    }
    let note = notes::NoteCoverage {
        missing: missing.unwrap_or_default(),
        unreadable,
    };
    let rp = found
        .iter()
        .map(|f| ReleasePr {
            component: Some(f.component.clone()),
            version: f.version,
            pr: f.pr.clone(),
        })
        .collect();
    (note.gaps(), version_gaps, rp)
}

/// Where release-please writes a component's changelog: its package's path
/// in `release-please-config.json`, else the root.
fn changelog_path(config: Option<&str>, component: &str) -> String {
    let packages = config
        .and_then(|c| serde_json::from_str::<serde_json::Value>(c).ok())
        .and_then(|v| v.get("packages").cloned())
        .and_then(|p| p.as_object().cloned())
        .unwrap_or_default();
    for (path, pkg) in &packages {
        let named = pkg.get("component").and_then(|v| v.as_str()) == Some(component)
            || path.rsplit('/').next() == Some(component);
        if named {
            return if path == "." {
                "CHANGELOG.md".into()
            } else {
                format!("{}/CHANGELOG.md", path.trim_end_matches('/'))
            };
        }
    }
    "CHANGELOG.md".into()
}

/// The GO and its facts, with the release-please PR when one was found.
/// `open` is the list of PRs open against the release branch, where
/// release-please proposes the release.
#[allow(clippy::too_many_arguments)]
pub fn decide(
    t: &Tick,
    report: &mut Report,
    shape: &Shape,
    item: &Item,
    c: &Candidate,
    branch: &str,
    states: &[(String, AngleState)],
    open: &[PullRequest],
    no_qa_angle: bool,
) -> Option<(go::GoDecision, Vec<ReleasePr>)> {
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
    // no gate and no QA has seen (REQ-REL-070).
    // The gate record is the bridge's own write on the branch, after the
    // candidate: it is never a gap.
    let record = shape.gate_record_path(c.version);
    let tip_gaps = match t.github.compare(&t.repo, &c.sha, branch) {
        Ok(cmp) => {
            let moved: Vec<String> = cmp
                .files
                .into_iter()
                .filter(|f| !release.bookkeeping_paths.iter().any(|b| b == f))
                .filter(|f| record.as_deref() != Some(f.as_str()))
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
    let (note_gaps, version_gaps, rp) = release_prs(t, shape, c, branch, open);
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
        note_gaps,
        fix_gaps: Vec::new(),
        tip_gaps,
        version_gaps,
        policy,
    };
    Some((go::evaluate(&facts), rp))
}

/// On GO (spec 05 §9): a single-component product's release PR is merged;
/// a multi-component product's gate record lands first, through the
/// bridge's own PR, then the component release PRs in order, one per tick.
#[allow(clippy::too_many_arguments)]
fn on_go(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    shape: &Shape,
    item: &Item,
    c: &Candidate,
    branch: &str,
    decision: &go::GoDecision,
    rp: &[ReleasePr],
    states: &[(String, AngleState)],
) -> bool {
    let Some(first) = rp.first() else {
        return false;
    };
    if let Some(path) = shape.gate_record_path(c.version) {
        match record_on_branch(t, &path, branch, c) {
            Ok(true) => {}
            Ok(false) => {
                land_record(t, log, report, item, c, branch, &path, decision, rp, states);
                return false;
            }
            Err(e) => {
                t.unreadable(report, &item.key, format!("the gate record {path}: {e}"));
                return false;
            }
        }
    }
    merge_release_pr(t, log, report, item, c, &first.pr)
}

/// Whether the release branch carries the record of this candidate.
fn record_on_branch(t: &Tick, path: &str, branch: &str, c: &Candidate) -> Result<bool, String> {
    match t.github.file(&t.repo, path, branch) {
        Ok(text) => Ok(texts::record_names(&text, &c.tag)),
        Err(e) if e.is_not_found() => Ok(false),
        Err(e) => Err(e.to_string()),
    }
}

/// Land the gate record (REQ-REL-078): merge the bridge's record PR when it
/// is green, current and exactly the record; open it otherwise.
#[allow(clippy::too_many_arguments)]
fn land_record(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    item: &Item,
    c: &Candidate,
    branch: &str,
    path: &str,
    decision: &go::GoDecision,
    rp: &[ReleasePr],
    states: &[(String, AngleState)],
) {
    let head = format!("{}{}", review::RELEASE_RECORD_PREFIX, c.tag);
    let open = match t.github.open_pulls(&t.repo, Some(branch)) {
        Ok(o) => o,
        Err(e) => {
            t.unreadable(report, &item.key, e);
            return;
        }
    };
    if let Some(pr) = open.iter().find(|p| p.head_ref == head) {
        let ours = pr
            .author
            .as_ref()
            .is_some_and(|a| a.login.eq_ignore_ascii_case(&t.github_login));
        if !ours {
            t.unreadable(
                report,
                &item.key,
                format!("{} is not the bridge's own record PR", pr.url),
            );
            return;
        }
        let required = match t.github.required_checks(&t.repo, branch) {
            Ok(r) => r,
            Err(e) => {
                t.unreadable(report, &item.key, e);
                return;
            }
        };
        if checks::pr_state(pr, &required.contexts) != CheckState::Green {
            return;
        }
        if review::branch_state(pr) == BranchState::Behind {
            t.act(
                log,
                report,
                &item.key,
                &format!("update record PR #{}", pr.number),
                json!({}),
                || {
                    t.github
                        .update_branch(&t.repo, pr.number, &pr.head_sha)
                        .map_err(|e| e.to_string())
                },
            );
            return;
        }
        if review::branch_state(pr) != BranchState::Current {
            return;
        }
        // Exactly the record, as it stands at the pinned head.
        let files = t.github.pull_files(&t.repo, pr.number);
        let text = t.github.file(&t.repo, path, &pr.head_sha);
        let exact = matches!(&files, Ok(f) if f.len() == 1 && f[0] == path)
            && matches!(&text, Ok(x) if texts::record_names(x, &c.tag));
        if !exact {
            t.unreadable(
                report,
                &item.key,
                format!("{} is not exactly the gate record", pr.url),
            );
            return;
        }
        t.act(
            log,
            report,
            &item.key,
            &format!("merge record PR #{}", pr.number),
            json!({"head": pr.head_sha}),
            || {
                t.github
                    .merge_pull(
                        &t.repo,
                        pr.number,
                        &pr.head_sha,
                        crate::github::MergeMethod::Squash,
                    )
                    .map_err(|e| e.to_string())
            },
        );
        return;
    }
    let angles: Vec<(String, String)> = states
        .iter()
        .map(|(a, s)| (a.clone(), s.name().to_string()))
        .collect();
    let prs: Vec<(String, String, u64)> = rp
        .iter()
        .map(|r| {
            (
                r.component.clone().unwrap_or_default(),
                r.version.to_string(),
                r.pr.number,
            )
        })
        .collect();
    let computed_at = format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        t.now.0.year(),
        u8::from(t.now.0.month()),
        t.now.0.day(),
        t.now.0.hour(),
        t.now.0.minute()
    );
    let Some(content) = texts::gate_record(
        &c.version.to_string(),
        &c.tag,
        &c.sha,
        branch,
        &item.key,
        &item.url,
        &computed_at,
        decision,
        &angles,
        &prs,
    ) else {
        return;
    };
    let tip = match t.github.commit(&t.repo, branch) {
        Ok(cm) => cm.sha,
        Err(e) => {
            t.unreadable(report, &item.key, e);
            return;
        }
    };
    // A head left by a closed PR or a tick that died is nobody's to vouch
    // for: recreated from the release branch.
    let _ = t.github.delete_branch(&t.repo, &head);
    let made = t.act(
        log,
        report,
        &item.key,
        &format!("create branch {head}"),
        json!({"from": tip}),
        || {
            t.github
                .create_branch(&t.repo, &head, &tip)
                .map_err(|e| e.to_string())
        },
    );
    if made.is_none() {
        return;
    }
    let message = format!("docs(release): gate record {} ({})", c.version, item.key);
    let put = t.act(
        log,
        report,
        &item.key,
        &format!("write {path}"),
        json!({}),
        || {
            t.github
                .put_file(&t.repo, &head, path, &content, &message)
                .map_err(|e| e.to_string())
        },
    );
    if put.is_none() {
        return;
    }
    let body = format!(
        "The gate record of `{}` (`{}`): {}. Merged by the release pass once green; the release PRs of the cycle follow.",
        c.tag,
        decision.blockers_carried(),
        item.url
    );
    t.act(log, report, &item.key, "open record PR", json!({}), || {
        t.github
            .create_pull(&t.repo, &head, branch, &message, &body)
            .map(|_| ())
            .map_err(|e| e.to_string())
    });
}

fn merge_release_pr(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    item: &Item,
    c: &Candidate,
    pr: &PullRequest,
) -> bool {
    let done = t.act(
        log,
        report,
        &item.key,
        &format!("merge release PR #{} (GO)", pr.number),
        json!({"candidate": c.tag, "head": pr.head_sha}),
        || {
            t.github
                .merge_pull(
                    &t.repo,
                    pr.number,
                    &pr.head_sha,
                    crate::github::MergeMethod::Squash,
                )
                .map_err(|e| e.to_string())
        },
    );
    if done.is_some() {
        let body = format!(
            "GO computed on `{}`: the bridge merged the release PR {} (squash, at `{}`). The publication workflow takes over.",
            c.tag,
            pr.url,
            texts::short(&pr.head_sha)
        );
        t.act(log, report, &item.key, "comment GO", json!({}), || {
            t.tracker
                .comment(&item.key, &body)
                .map(|_| ())
                .map_err(|e| e.to_string())
        });
    }
    done.is_some()
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
/// release goes to the owner, once (spec 05 §10).
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
    match tracker_said(t, &item.key, &marker) {
        Ok(false) => {}
        _ => return,
    }
    // Every effect first; the comment that carries the marker last, and only
    // when they all went through, so that a failed one is retried.
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
    let hold_label = &release.escalation_label;
    if !item.labels.iter().any(|l| l == hold_label)
        && t.act(
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
        )
        .is_none()
    {
        return;
    }
    let text = texts::escalation_comment(&c.tag, c.rc, max, &d.blockers, moved);
    let sent = t.act(
        log,
        report,
        &item.key,
        "notify escalation",
        json!({}),
        || t.signal.signal("escalation", &text).map(|_| ()),
    );
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
        ("Release note gaps", d.note_gaps.clone()),
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
    let sent = t.act(
        log,
        report,
        &item.key,
        "notify stagnation",
        json!({}),
        || t.signal.signal("stagnation", &text).map(|_| ()),
    );
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
