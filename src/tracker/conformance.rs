//! The conformance suite every `Tracker` adapter must pass
//! (`docs/livela-contract.md`, Conformance): create a release item with two
//! angle children, link a PR URL and find the item back by it, move a child
//! through In Review → To Deploy → Done, lay a `blocks` relation and read it
//! back from both ends, comment as the service identity and edit the
//! comment, list by label, then cancel everything it created.
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
    let mut r = Report::default();
    let me = step(&mut r, "identity", tracker.identity())?;
    let names = step(&mut r, "status names", tracker.status_names())?;
    let missing = statuses.missing(&names);
    check(
        missing.is_empty(),
        &format!("the team lacks the statuses {missing:?}"),
    )?;
    let team = step(&mut r, "team key", tracker.team_key())?;
    check(!team.is_empty(), "the team key is empty")?;

    let label = format!("release/conformance-v0.0.{run_id}");
    step(&mut r, "ensure label", tracker.ensure_label(&label))?;
    step(
        &mut r,
        "ensure label again (idempotent)",
        tracker.ensure_label(&label),
    )?;

    let release = step(
        &mut r,
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

    let mut children = Vec::new();
    for angle in ["security", "product"] {
        let child = step(
            &mut r,
            &format!("create {angle} child"),
            tracker.create_item(&NewItem {
                title: format!("{angle} review — conformance v0.0.{run_id}"),
                description: "- [ ] a box".into(),
                labels: vec![label.clone()],
                status: Status::Ready,
                priority: Some(1),
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
    let listed = step(&mut r, "children", tracker.children(&release.key))?;
    check(
        listed.len() == 2,
        &format!("{} children listed, 2 expected", listed.len()),
    )?;

    let pr_url = format!("https://github.com/example-org/conformance/pull/{run_id}");
    step(
        &mut r,
        "link PR",
        tracker.link_pr(&children[0].key, &pr_url, "conformance PR"),
    )?;
    let found = step(
        &mut r,
        "find by PR URL",
        tracker.items_linked_to_pr(&pr_url),
    )?;
    check(
        found == vec![children[0].key.clone()],
        &format!("found {found:?} by the PR URL"),
    )?;

    for status in [Status::InReview, Status::ToDeploy, Status::Done] {
        step(
            &mut r,
            &format!("set {status}"),
            tracker.set_status(&children[0].key, status, None),
        )?;
        let back = step(
            &mut r,
            &format!("read {status}"),
            tracker.get(&children[0].key),
        )?;
        check(
            back.state.status == Some(status),
            &format!("set {status}, read {:?}", back.state.status),
        )?;
    }

    step(
        &mut r,
        "relate blocks",
        tracker.relate(&children[1].key, &release.key, RelationKind::Blocks),
    )?;
    let of_release = step(
        &mut r,
        "relations of the blocked",
        tracker.relations(&release.key),
    )?;
    let arc = of_release
        .blocked_by
        .iter()
        .find(|x| x.other == children[1].key)
        .cloned();
    check(arc.is_some(), "the blocked item does not see its blocker")?;
    let of_blocker = step(
        &mut r,
        "relations of the blocker",
        tracker.relations(&children[1].key),
    )?;
    check(
        of_blocker.blocks.iter().any(|x| x.other == release.key),
        "the blocker does not see what it blocks",
    )?;

    let c = step(
        &mut r,
        "comment",
        tracker.comment(&release.key, "Conformance comment."),
    )?;
    let edits = tracker.can_edit_comments();
    if edits {
        step(
            &mut r,
            "edit comment",
            tracker.update_comment(&c.id, "Conformance comment, edited."),
        )?;
    } else {
        r.not_offered.push("comment edit".to_string());
    }
    let thread = step(&mut r, "read comments", tracker.comments(&release.key))?;
    let mine = thread.iter().find(|x| x.id == c.id);
    check(
        mine.is_some_and(|x| x.by_bridge),
        "the comment is not attributed to the service identity",
    )?;
    check(
        !edits || mine.is_some_and(|x| x.body.contains("edited")),
        "the comment edit did not land",
    )?;
    check(
        mine.and_then(|x| x.author.as_ref())
            .is_some_and(|a| a.id == me.id),
        "the comment author is not the service identity",
    )?;

    let by_label = step(&mut r, "list by label", tracker.list_by_label(&label))?;
    check(
        by_label.len() == 3,
        &format!("{} items carry the label, 3 expected", by_label.len()),
    )?;

    step(
        &mut r,
        "describe",
        tracker.update_description(&release.key, "Edited by the conformance run."),
    )?;
    step(&mut r, "add label", tracker.add_label(&release.key, &label))?;
    if let Some(arc) = arc {
        step(&mut r, "unrelate", tracker.unrelate(&arc.id))?;
        let after = step(
            &mut r,
            "relations after unrelate",
            tracker.relations(&release.key),
        )?;
        check(
            after.blocked_by.iter().all(|x| x.other != children[1].key),
            "the relation is still there",
        )?;
    }
    for key in r.created.clone() {
        step(
            &mut r,
            &format!("cancel {key}"),
            tracker.set_status(&key, Status::Canceled, None),
        )?;
    }
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tracker::fake::FakeTracker;

    #[test]
    fn the_fake_tracker_passes_the_conformance_suite() {
        let t = FakeTracker::new("ABC");
        let report = run(&t, &StatusTable::identity(), "1").unwrap();
        assert_eq!(report.created.len(), 3);
        assert!(report.passed.contains(&"find by PR URL".to_string()));
    }

    #[test]
    fn a_tracker_that_refuses_a_write_fails_the_suite_by_name() {
        let t = FakeTracker::new("ABC");
        t.fail_writes_starting_with("relate");
        let err = run(&t, &StatusTable::identity(), "2").unwrap_err();
        assert!(err.contains("`relate blocks`"), "{err}");
    }
}
