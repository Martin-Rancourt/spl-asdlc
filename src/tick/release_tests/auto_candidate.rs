//! The next candidate cut by the bridge with `release.auto_candidate`
//! (rule C8WN, `docs/reference/05-release-cycle.md` §7): once per release
//! branch head, when the branch moved past the latest candidate and every
//! fix of the release is merged.

use super::*;
use crate::method::release::findings;

/// The cut the bridge dispatches on the release branch.
const DISPATCH: &str =
    "dispatch example-org/acme release-cut.yml on release/2026-10-05-14-03-utc action=candidate";

/// The example with `[release]` edited by `edits` (`from` → `to`).
fn edited(edits: &[(&str, &str)]) -> Config {
    let mut text = crate::config::spec_example::repository().to_string();
    for (from, to) in edits {
        text = text.replace(from, to);
    }
    crate::config::spec_example::parse(&text)
}

const AUTO: (&str, &str) = ("auto_candidate = false", "auto_candidate = true");

impl World {
    /// The example as edited, the cycle opened.
    fn opened(edits: &[(&str, &str)]) -> World {
        let mut w = World::new();
        w.config = edited(edits);
        w.tick();
        w
    }

    /// A fix merged to the release branch: the branch at `head`, past the
    /// candidate by a file that is not bookkeeping.
    fn fix_merged(&self, head: &str) {
        self.moved_past("c1", &["src/fix.rs"]);
        self.github
            .state
            .borrow_mut()
            .branches
            .insert(repo(), vec![tag(BRANCH, head)]);
    }

    fn dispatches(&self) -> Vec<String> {
        self.github
            .writes()
            .into_iter()
            .filter(|l| l.contains("dispatch "))
            .collect()
    }

    /// The bridge's comments on the release item that record a cut.
    fn cut_comments(&self) -> usize {
        let release = self.release_item();
        self.tracker
            .comments(&release.key)
            .unwrap()
            .iter()
            .filter(|c| c.by_bridge && c.body.contains("spl-asdlc:auto-candidate"))
            .count()
    }

    /// A gate-fix ticket of the release, in `status`.
    fn gate_fix_in(&self, key: &str, status: Status) {
        let release = self.release_item();
        let body = format!(
            "## Goal\nThe gate green.\n\n{}\n",
            texts::gate_fix_marker(CAND)
        );
        self.tracker.seed(key, "Gate fix", &body, status, &[LABEL]);
        self.tracker
            .state
            .borrow_mut()
            .set_parent(key, &release.key);
    }

    /// A blocking finding's ticket of the release, in `status`.
    fn blocking_finding_in(&self, key: &str, status: Status) {
        let release = self.release_item();
        let body = format!(
            "## Goal\nFix the finding.\n\n{}\n",
            findings::blocker_marker(&release.key)
        );
        self.tracker.seed(key, "Finding", &body, status, &[LABEL]);
    }
}

#[test]
fn with_auto_candidate_the_bridge_cuts_the_next_candidate_once_per_branch_head() {
    let w = World::opened(&[AUTO]);
    assert!(w.dispatches().is_empty(), "nothing moved yet");
    w.fix_merged("f1");
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(w.dispatches(), vec![DISPATCH.to_string()]);
    assert_eq!(w.cut_comments(), 1);
    // The same head: never dispatched again.
    w.tick();
    assert_eq!(w.dispatches().len(), 1);
    assert_eq!(w.cut_comments(), 1);
    // A newer fix moves the head: one more cut.
    w.fix_merged("f2");
    w.tick();
    assert_eq!(w.dispatches().len(), 2);
    assert_eq!(w.cut_comments(), 2);
    assert_eq!(
        w.tip_gaps(),
        vec![format!(
            "{BRANCH} moved past {CAND} (1 file(s)): the bridge cuts the next candidate once the release's fixes are merged"
        )]
    );
}

#[test]
fn bookkeeping_after_the_candidate_cuts_nothing() {
    let w = World::opened(&[AUTO]);
    w.moved_past("c1", &[&format!(".spl-asdlc/qa/{CAND}/security.json")]);
    w.github
        .state
        .borrow_mut()
        .branches
        .insert(repo(), vec![tag(BRANCH, "q1")]);
    w.tick();
    assert!(w.dispatches().is_empty(), "{:?}", w.dispatches());
}

/// Rule X4PV: a list GitHub cut at 300 files holds more than bookkeeping,
/// even when every file it lists is: the next candidate is cut.
#[test]
fn a_cut_file_list_after_the_candidate_is_never_bookkeeping_only() {
    let w = World::opened(&[AUTO]);
    let report = format!(".spl-asdlc/qa/{CAND}/security.json");
    let listed: Vec<&str> = (0..300).map(|_| report.as_str()).collect();
    w.moved_past("c1", &listed);
    w.github
        .state
        .borrow_mut()
        .branches
        .insert(repo(), vec![tag(BRANCH, "q1")]);
    w.tick();
    assert_eq!(w.dispatches(), vec![DISPATCH.to_string()]);
}

#[test]
fn no_cut_while_a_fix_ticket_of_the_release_is_before_to_deploy() {
    let w = World::opened(&[AUTO]);
    w.gate_fix_in("ABC-90", Status::InProgress);
    w.blocking_finding_in("ABC-91", Status::InReview);
    w.fix_merged("f1");
    w.tick();
    assert!(w.dispatches().is_empty(), "{:?}", w.dispatches());
    // The gate fix merged; the finding's fix is still in review.
    w.tracker
        .set_status("ABC-90", Status::ToDeploy, None)
        .unwrap();
    w.tick();
    assert!(w.dispatches().is_empty(), "{:?}", w.dispatches());
    // Every fix of the release merged: the next candidate is cut.
    w.tracker
        .set_status("ABC-91", Status::ToDeploy, None)
        .unwrap();
    w.tick();
    assert_eq!(w.dispatches(), vec![DISPATCH.to_string()]);
}

#[test]
fn without_auto_candidate_nothing_is_dispatched_and_status_says_a_cut_is_due() {
    let w = World::opened(&[]);
    w.fix_merged("f1");
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert!(w.dispatches().is_empty(), "{:?}", w.dispatches());
    assert_eq!(w.cut_comments(), 0);
    assert_eq!(
        w.tip_gaps(),
        vec![format!(
            "{BRANCH} moved past {CAND} (1 file(s)): a cut of the next candidate is due"
        )]
    );
}

#[test]
fn a_failed_dispatch_writes_no_marker_and_is_retried_at_the_next_tick() {
    let w = World::opened(&[AUTO]);
    w.fix_merged("f1");
    w.github
        .fail_writes_starting_with("dispatch ", Some(422), "workflow not found");
    let r = w.tick();
    assert!(
        r.failures
            .iter()
            .any(|f| f.contains("dispatch release-cut.yml")),
        "{r:?}"
    );
    assert_eq!(w.cut_comments(), 0, "no marker for a failed dispatch");
    w.github.state.borrow_mut().failing_writes.clear();
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(
        w.dispatches().last().map(String::as_str),
        Some(DISPATCH),
        "{:?}",
        w.dispatches()
    );
    assert_eq!(w.cut_comments(), 1);
}

#[test]
fn no_cut_past_the_last_allowed_pass() {
    let w = World::opened(&[AUTO, ("max_rc_passes = 2", "max_rc_passes = 1")]);
    w.fix_merged("f1");
    w.tick();
    assert!(w.dispatches().is_empty(), "{:?}", w.dispatches());
}

#[test]
fn a_dry_run_prints_the_cut_it_would_dispatch() {
    let w = World::opened(&[AUTO]);
    w.fix_merged("f1");
    let intents = crate::dry::Intents::default();
    let gh = crate::dry::DryGitHub {
        inner: &w.github,
        intents: &intents,
    };
    let tr = crate::dry::DryTracker {
        inner: &w.tracker,
        intents: &intents,
    };
    let sig = crate::dry::DrySignal { intents: &intents };
    let t = Tick {
        config: &w.config,
        repo: repo(),
        github: &gh,
        tracker: &tr,
        signal: &sig,
        github_login: bot("spl-asdlc"),
        now: Timestamp::parse("2026-10-05T12:00:00Z").unwrap(),
        deadline: None,
        dry_run: true,
        method_texts: Default::default(),
        low_quota: false,
    };
    let before = (w.github.writes().len(), w.tracker.writes().len());
    let mut log = ActionLog::in_memory();
    run(&t, &mut log);
    assert_eq!(
        (w.github.writes().len(), w.tracker.writes().len()),
        before,
        "a dry run writes nothing"
    );
    let line =
        format!("github example-org/acme: dispatch release-cut.yml on {BRANCH} action=candidate");
    assert!(intents.lines().contains(&line), "{:?}", intents.lines());
}
