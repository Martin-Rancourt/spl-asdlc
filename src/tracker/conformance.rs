//! The conformance suite every `Tracker` adapter must pass
//! (`docs/livela-contract.md`, Conformance): create a release item with two
//! angle children, link a PR URL and find the item back by it, move a child
//! through In Review → To Deploy → Post Deploy → Done, lay a `blocks`
//! relation and read it back from both ends (and only there), comment twice
//! as the service identity and read the thread oldest first, edit a comment
//! where the tracker can, describe, label, unlabel, assign and unassign with
//! a read-back after each write, repeat every write and check that the
//! repetition leaves no trace, list by label, then cancel everything it
//! created, on success and on failure alike.
//!
//! It writes; run it on a test team or project only.

use super::*;

/// What one run did, step by step.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Report {
    pub passed: Vec<String>,
    pub created: Vec<String>,
    /// Optional capabilities the tracker says it does not offer.
    pub not_offered: Vec<String>,
}

/// Counts the changes the tracker recorded (facts, history entries), to
/// prove that a write of an unchanged value records none. `None` when the
/// tracker cannot tell: the repeated writes are then checked by read-back
/// only.
pub type ChangeCounter<'a> = Option<&'a dyn Fn() -> u64>;

fn check(cond: bool, what: &str) -> Result<(), String> {
    if cond {
        Ok(())
    } else {
        Err(format!("conformance failed: {what}"))
    }
}

fn step<T>(report: &mut Report, name: &str, r: TResult<T>) -> Result<T, String> {
    match r {
        Ok(v) => {
            report.passed.push(name.to_string());
            Ok(v)
        }
        Err(e) => Err(format!("conformance failed at `{name}`: {e}")),
    }
}

/// Run the suite. `run_id` makes the labels and URLs of this run unique.
pub fn run(tracker: &dyn Tracker, statuses: &StatusTable, run_id: &str) -> Result<Report, String> {
    run_with(tracker, statuses, run_id, None)
}

/// Run the suite with a change counter. On failure, the items created so
/// far are canceled and named in the error.
pub fn run_with(
    tracker: &dyn Tracker,
    statuses: &StatusTable,
    run_id: &str,
    changes: ChangeCounter<'_>,
) -> Result<Report, String> {
    let mut r = Report::default();
    match suite(tracker, statuses, run_id, changes, &mut r) {
        Ok(()) => {
            for key in r.created.clone() {
                step(
                    &mut r,
                    &format!("cancel {key}"),
                    tracker.set_status(&key, Status::Canceled, None),
                )?;
            }
            Ok(r)
        }
        Err(e) => {
            let mut left = Vec::new();
            for key in &r.created {
                if tracker.set_status(key, Status::Canceled, None).is_err() {
                    left.push(key.clone());
                }
            }
            let created = r.created.join(", ");
            Err(if left.is_empty() {
                format!("{e} (created and canceled: {created})")
            } else {
                format!(
                    "{e} (created: {created}; could not cancel: {})",
                    left.join(", ")
                )
            })
        }
    }
}

fn suite(
    tracker: &dyn Tracker,
    statuses: &StatusTable,
    run_id: &str,
    changes: ChangeCounter<'_>,
    r: &mut Report,
) -> Result<(), String> {
    let me = step(r, "identity", tracker.identity())?;
    let names = step(r, "status names", tracker.status_names())?;
    let missing = statuses.missing(&names);
    check(
        missing.is_empty(),
        &format!("the team lacks the statuses {missing:?}"),
    )?;
    let team = step(r, "team key", tracker.team_key())?;
    check(!team.is_empty(), "the team key is empty")?;
    let found_me = step(r, "user by name", tracker.user(&me.name))?;
    check(
        found_me.id == me.id,
        "the service identity is not found by its name",
    )?;
    let found_me = step(r, "user by id", tracker.user(&me.id))?;
    check(
        found_me.id == me.id,
        "the service identity is not found by its id",
    )?;

    let unknown = format!("{team}-99999998");
    match tracker.get(&unknown) {
        Err(e) if e.is_not_found() => r.passed.push("get an unknown key".into()),
        other => {
            return Err(format!(
                "conformance failed: `get` of an unknown key must be NotFound, got {other:?}"
            ));
        }
    }

    let label = format!("release/conformance-v0.0.{run_id}");
    let extra = format!("conformance-extra-{run_id}");
    step(r, "ensure label", tracker.ensure_label(&label))?;
    step(
        r,
        "ensure label again (idempotent)",
        tracker.ensure_label(&label),
    )?;
    step(r, "ensure a second label", tracker.ensure_label(&extra))?;

    let release = step(
        r,
        "create release item",
        tracker.create_item(&NewItem {
            title: format!("Release conformance v0.0.{run_id}"),
            description: "Conformance run of the spl-asdlc tracker adapter.".into(),
            labels: vec![label.clone()],
            status: Status::Ready,
            priority: Some(1),
            assignee: None,
            parent: None,
        }),
    )?;
    r.created.push(release.key.clone());
    check(
        release.state.status == Some(Status::Ready),
        "the release item is not Ready",
    )?;
    check(
        release.labels.contains(&label),
        "the release item lacks its label",
    )?;
    let back = step(r, "read the release item", tracker.get(&release.key))?;
    check(back == release, "the release item reads back differently")?;

    let mut children = Vec::new();
    for angle in ["security", "product"] {
        let child = step(
            r,
            &format!("create {angle} child"),
            tracker.create_item(&NewItem {
                title: format!("{angle} review — conformance v0.0.{run_id}"),
                description: "- [ ] a box".into(),
                labels: vec![label.clone()],
                status: Status::Ready,
                priority: Some(3),
                assignee: None,
                parent: Some(release.key.clone()),
            }),
        )?;
        r.created.push(child.key.clone());
        check(
            child.parent.as_deref() == Some(release.key.as_str()),
            "a child does not name its parent",
        )?;
        children.push(child);
    }
    let listed = step(r, "children", tracker.children(&release.key))?;
    let mut listed: Vec<String> = listed.into_iter().map(|i| i.key).collect();
    listed.sort();
    let mut expected: Vec<String> = children.iter().map(|c| c.key.clone()).collect();
    expected.sort();
    check(
        listed == expected,
        &format!("children {listed:?}, expected {expected:?}"),
    )?;

    // The PR link, found back, idempotent.
    let pr_url = format!("https://github.com/example-org/conformance/pull/{run_id}");
    let unlinked = format!("https://github.com/example-org/conformance/pull/{run_id}0");
    step(
        r,
        "link PR",
        tracker.link_pr(&children[0].key, &pr_url, "conformance PR"),
    )?;
    let found = step(r, "find by PR URL", tracker.items_linked_to_pr(&pr_url))?;
    check(
        found == vec![children[0].key.clone()],
        &format!("found {found:?} by the PR URL"),
    )?;
    let none = step(
        r,
        "find by an unlinked URL",
        tracker.items_linked_to_pr(&unlinked),
    )?;
    check(
        none.is_empty(),
        &format!("found {none:?} by an unlinked URL"),
    )?;

    // Through the validation statuses to Done.
    for status in [
        Status::InReview,
        Status::ToDeploy,
        Status::PostDeploy,
        Status::Done,
    ] {
        step(
            r,
            &format!("set {status}"),
            tracker.set_status(&children[0].key, status, None),
        )?;
        let back = step(r, &format!("read {status}"), tracker.get(&children[0].key))?;
        check(
            back.state.status == Some(status),
            &format!("set {status}, read {:?}", back.state.status),
        )?;
    }

    // Status and assignee in one call, then the assignee alone.
    step(
        r,
        "set status with an assignee",
        tracker.set_status(&children[1].key, Status::InProgress, Some(Some(&me))),
    )?;
    let back = step(r, "read status and assignee", tracker.get(&children[1].key))?;
    check(
        back.state.status == Some(Status::InProgress)
            && back.assignee.as_ref().map(|u| &u.id) == Some(&me.id),
        "the status and the assignee did not land together",
    )?;
    step(r, "unassign", tracker.assign(&children[1].key, None))?;
    let back = step(r, "read unassigned", tracker.get(&children[1].key))?;
    check(back.assignee.is_none(), "the item is still assigned")?;
    step(r, "assign", tracker.assign(&children[1].key, Some(&me)))?;
    let back = step(r, "read assigned", tracker.get(&children[1].key))?;
    check(
        back.assignee.as_ref().map(|u| &u.id) == Some(&me.id),
        "the assignee did not land",
    )?;

    // A `blocks` relation, seen from both ends and only in its direction.
    step(
        r,
        "relate blocks",
        tracker.relate(&children[1].key, &release.key, RelationKind::Blocks),
    )?;
    let of_release = step(
        r,
        "relations of the blocked",
        tracker.relations(&release.key),
    )?;
    let arc = of_release
        .blocked_by
        .iter()
        .find(|x| x.other == children[1].key)
        .cloned();
    check(arc.is_some(), "the blocked item does not see its blocker")?;
    check(
        of_release.blocked_by.len() == 1,
        "the blocked item sees more than one blocker",
    )?;
    check(
        of_release.blocks.is_empty(),
        "the blocked item says it blocks something",
    )?;
    let of_blocker = step(
        r,
        "relations of the blocker",
        tracker.relations(&children[1].key),
    )?;
    check(
        of_blocker.blocks.iter().any(|x| x.other == release.key),
        "the blocker does not see what it blocks",
    )?;
    check(
        of_blocker.blocked_by.is_empty(),
        "the blocker says it is blocked",
    )?;
    if tracker
        .relate(&children[1].key, &unknown, RelationKind::Blocks)
        .is_ok()
    {
        return Err("conformance failed: a relation to an unknown item was accepted".into());
    }
    r.passed.push("refuse a relation to an unknown item".into());

    // Two comments, oldest first, signed by the service identity.
    let c1 = step(
        r,
        "comment",
        tracker.comment(&release.key, "Conformance comment, first."),
    )?;
    let c2 = step(
        r,
        "second comment",
        tracker.comment(&release.key, "Conformance comment, second."),
    )?;
    let edits = tracker.can_edit_comments();
    if edits {
        step(
            r,
            "edit comment",
            tracker.update_comment(&c1.id, "Conformance comment, first, edited."),
        )?;
    } else {
        r.not_offered.push("comment edit".to_string());
    }
    let thread = step(r, "read comments", tracker.comments(&release.key))?;
    let ids: Vec<&str> = thread.iter().map(|c| c.id.as_str()).collect();
    check(
        ids == [c1.id.as_str(), c2.id.as_str()],
        &format!("the thread reads {ids:?}, expected oldest first"),
    )?;
    check(
        thread.iter().all(|x| x.by_bridge),
        "a comment is not attributed to the service identity",
    )?;
    check(
        thread
            .iter()
            .all(|x| x.author.as_ref().is_some_and(|a| a.id == me.id)),
        "a comment author is not the service identity",
    )?;
    check(
        !edits || thread[0].body.contains("edited"),
        "the comment edit did not land",
    )?;

    // Description and labels, each read back.
    let description = format!("Edited by conformance run {run_id}.");
    step(
        r,
        "describe",
        tracker.update_description(&release.key, &description),
    )?;
    step(r, "add label", tracker.add_label(&release.key, &extra))?;
    let back = step(r, "read description and labels", tracker.get(&release.key))?;
    check(
        back.description == description,
        "the description did not land",
    )?;
    check(
        back.labels.contains(&extra) && back.labels.contains(&label),
        &format!("labels read {:?}", back.labels),
    )?;

    // Every write again, unchanged: nothing recorded, nothing doubled.
    let before = changes.map(|c| c());
    step(
        r,
        "repeat: status",
        tracker.set_status(&children[0].key, Status::Done, None),
    )?;
    step(
        r,
        "repeat: status and assignee",
        tracker.set_status(&children[1].key, Status::InProgress, Some(Some(&me))),
    )?;
    step(
        r,
        "repeat: assign",
        tracker.assign(&children[1].key, Some(&me)),
    )?;
    step(
        r,
        "repeat: describe",
        tracker.update_description(&release.key, &description),
    )?;
    step(
        r,
        "repeat: add label",
        tracker.add_label(&release.key, &extra),
    )?;
    step(
        r,
        "repeat: relate",
        tracker.relate(&children[1].key, &release.key, RelationKind::Blocks),
    )?;
    step(
        r,
        "repeat: link PR",
        tracker.link_pr(&children[0].key, &pr_url, "conformance PR"),
    )?;
    if let (Some(count), Some(before)) = (changes, before) {
        let after = count();
        check(
            after == before,
            &format!(
                "writing unchanged values recorded {} change(s)",
                after - before
            ),
        )?;
        r.passed.push("unchanged writes record nothing".into());
    }
    let again = step(
        r,
        "relations after the repeat",
        tracker.relations(&release.key),
    )?;
    check(
        again.blocked_by.len() == 1,
        "the repeated relation was doubled",
    )?;
    let found = step(
        r,
        "find by PR URL after the repeat",
        tracker.items_linked_to_pr(&pr_url),
    )?;
    check(
        found == vec![children[0].key.clone()],
        &format!("found {found:?} after linking twice"),
    )?;

    step(
        r,
        "remove label",
        tracker.remove_label(&release.key, &extra),
    )?;
    let back = step(r, "read labels after removal", tracker.get(&release.key))?;
    check(
        !back.labels.contains(&extra) && back.labels.contains(&label),
        &format!("labels read {:?} after the removal", back.labels),
    )?;

    let by_label = step(r, "list by label", tracker.list_by_label(&label))?;
    check(
        by_label.len() == 3,
        &format!("{} items carry the label, 3 expected", by_label.len()),
    )?;

    if let Some(arc) = arc {
        step(r, "unrelate", tracker.unrelate(&arc.id))?;
        let after = step(
            r,
            "relations after unrelate",
            tracker.relations(&release.key),
        )?;
        check(
            after.blocked_by.iter().all(|x| x.other != children[1].key),
            "the relation is still there",
        )?;
        let blocker = step(
            r,
            "blocker after unrelate",
            tracker.relations(&children[1].key),
        )?;
        check(
            blocker.blocks.is_empty(),
            "the blocker still sees the relation",
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tracker::fake::FakeTracker;

    #[test]
    fn the_fake_tracker_passes_the_conformance_suite() {
        let t = FakeTracker::new("ABC");
        let count = || t.writes().len() as u64;
        let report = run_with(&t, &StatusTable::identity(), "1", Some(&count)).unwrap();
        assert_eq!(report.created.len(), 3);
        assert!(
            report
                .passed
                .contains(&"unchanged writes record nothing".to_string())
        );
        assert!(report.passed.contains(&"find by PR URL".to_string()));
    }

    #[test]
    fn a_tracker_that_refuses_a_write_fails_the_suite_by_name() {
        let t = FakeTracker::new("ABC");
        t.fail_writes_starting_with("relate");
        let err = run(&t, &StatusTable::identity(), "2").unwrap_err();
        assert!(err.contains("`relate blocks`"), "{err}");
        assert!(err.contains("created and canceled"), "{err}");
        let canceled = t
            .writes()
            .iter()
            .filter(|w| w.starts_with("status") && w.contains("Canceled"))
            .count();
        assert_eq!(canceled, 3, "a failed run cancels what it created");
    }
}
