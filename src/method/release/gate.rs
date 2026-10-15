//! Reading the gate and the class of a candidate (REQ-REL-010, 011, 034,
//! 035): the gate is the run of the gate workflow on the candidate's exact
//! commit, from its own tag push; the class travels in the tag annotation.

use crate::github::{Timestamp, WorkflowRun};
use crate::method::glob;

/// The newest run that is the tag's own verdict: same commit, the tag as
/// its ref, started by the tag push. A run on `main` sharing the commit, or
/// a dispatch, is not the candidate's certification. Runs come newest first.
pub fn gate_run<'a>(
    runs: &'a [WorkflowRun],
    sha: &str,
    candidate: &str,
) -> Option<&'a WorkflowRun> {
    runs.iter()
        .find(|r| r.head_sha == sha && r.head_branch == candidate && r.event == "push")
}

/// The name of the evidence pack the gate run uploads for the commit `sha`
/// (REQ-REL-037).
pub fn evidence_artifact(sha: &str) -> String {
    format!("release-gate-evidence-{sha}")
}

/// Green only for a run that concluded `success`.
pub fn is_green(run: Option<&WorkflowRun>) -> bool {
    run.and_then(|r| r.conclusion.as_deref()) == Some("success")
}

/// What the gate's state says, for messages: "never ran", "concluded …".
pub fn detail(run: Option<&WorkflowRun>, workflow: &str, candidate: &str) -> String {
    match run {
        None => format!("`{workflow}` never ran on `{candidate}`"),
        Some(r) => format!(
            "`{workflow}` concluded `{}`",
            r.conclusion.as_deref().unwrap_or("nothing yet")
        ),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Colour {
    Green,
    Red,
    /// Not decided yet, unreadable, or a conclusion nobody listed.
    Orange,
}

/// The allowlist of `release status` (REQ-REL-035): only `success` is green.
pub fn colour(conclusion: Option<&str>) -> Colour {
    match conclusion {
        Some("success") => Colour::Green,
        Some("failure" | "timed_out" | "startup_failure" | "cancelled" | "action_required") => {
            Colour::Red
        }
        _ => Colour::Orange,
    }
}

/// Where a red gate stands with its one automatic rerun (REQ-REL-037: a red
/// first attempt that is a runner abort or a known flake is rerun once, by
/// the rerun workflow on the gate run's completion).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RedGate {
    /// Not a red gate: green, pending, or a conclusion nobody listed.
    NotRed,
    /// Red, but a rerun may still come: the work of a red gate waits.
    Awaiting(String),
    /// Red, and no automatic rerun is left to come.
    Settled,
    /// Red, and no run of the rerun workflow showed up within the bounded
    /// wait: the rerun workflow may be absent or broken, so the red is
    /// taken as final (the reason says so).
    Unseen(String),
}

impl RedGate {
    /// The red is final: its gate-fix ticket is due.
    pub fn is_final(&self) -> bool {
        matches!(self, RedGate::Settled | RedGate::Unseen(_))
    }
}

/// How long a red first attempt waits for a run of the rerun workflow to
/// show up (`release.gate_rerun_wait_minutes`), and the tick's instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RerunWait {
    pub now: Timestamp,
    pub bound: time::Duration,
}

/// Whether the gate run's red is final (the owner's decisions, 2026-10-08):
/// the bridge cannot read the rerun decision, so it waits for what it can
/// see. A rerun attempt that completed red is final (one rerun at most). A
/// first attempt is final once the rerun workflow ran for it and every such
/// run completed: `reruns` are that workflow's `workflow_run` runs on the
/// candidate's commit. While none exists the rerun may still come, but only
/// for `wait.bound` after the gate run completed (its `updated_at`, else
/// its `created_at`): past it the red is final all the same, the rerun
/// workflow may be absent or broken. A rerun run that showed up and is
/// still running keeps the wait, whatever the bound. The caller reads them
/// before it reads the gate run again, so that a rerun they made is seen.
pub fn red_gate(run: &WorkflowRun, reruns: &[WorkflowRun], wait: &RerunWait) -> RedGate {
    if run.status != "completed" || colour(run.conclusion.as_deref()) != Colour::Red {
        return RedGate::NotRed;
    }
    if run.attempt > 1 {
        return RedGate::Settled;
    }
    let mine: Vec<&WorkflowRun> = reruns
        .iter()
        .filter(|r| r.event == "workflow_run" && r.head_sha == run.head_sha)
        // A rerun workflow run older than the gate run is another run's.
        .filter(|r| match (&r.created_at, &run.created_at) {
            (Some(a), Some(b)) => a.0 >= b.0,
            _ => true,
        })
        .collect();
    if mine.is_empty() {
        let minutes = wait.bound.whole_minutes();
        return match run.updated_at.or(run.created_at) {
            Some(done) if wait.now.0 - done.0 >= wait.bound => RedGate::Unseen(format!(
                "no run of the automatic rerun workflow showed up within {minutes} minutes of the completion of {}",
                run.url
            )),
            Some(done) => RedGate::Awaiting(format!(
                "the automatic rerun of {} has not run yet (the bridge waits for it until {})",
                run.url,
                Timestamp(done.0 + wait.bound)
            )),
            None => RedGate::Awaiting(format!(
                "the automatic rerun of {} has not run yet, and when the run completed cannot be read",
                run.url
            )),
        };
    }
    match mine.iter().find(|r| r.status != "completed") {
        Some(r) => RedGate::Awaiting(format!(
            "the automatic rerun of {} is still running ({})",
            run.url, r.url
        )),
        None => RedGate::Settled,
    }
}

/// The path of the gate's summary inside its evidence pack (REQ-REL-037).
pub const SUITES_FILE: &str = "results/suites.json";

/// The failed suites a `results/suites.json` names (its `failed` list), or
/// why it cannot be read: never an empty list for an unreadable file.
pub fn failed_suites(json: &str) -> Result<Vec<String>, String> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("{SUITES_FILE} is not JSON: {e}"))?;
    let failed = v
        .get("failed")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| format!("{SUITES_FILE} has no `failed` list"))?;
    failed
        .iter()
        .map(|f| {
            f.as_str()
                .map(str::to_string)
                .ok_or_else(|| format!("{SUITES_FILE} names a failed suite that is not a string"))
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Routine,
    Sensible,
    Unclassified,
}

impl Class {
    pub fn name(self) -> &'static str {
        match self {
            Class::Routine => "routine",
            Class::Sensible => "sensible",
            Class::Unclassified => "unclassified",
        }
    }
}

/// The class of a candidate, read from the annotation of a real tag object
/// only: a `Release-Class:` line anchored at its end of line, `routine` or
/// `sensible`; anything else is `unclassified`, never promoted to routine.
pub fn class_of_annotation(message: Option<&str>) -> Class {
    let Some(message) = message else {
        return Class::Unclassified;
    };
    for line in message.lines() {
        if let Some(value) = line.strip_prefix("Release-Class:") {
            return match value.trim() {
                "routine" => Class::Routine,
                "sensible" => Class::Sensible,
                _ => Class::Unclassified,
            };
        }
    }
    Class::Unclassified
}

/// The class a cut computes (REQ-REL-011): `sensible` whenever the diff or
/// the reviewed paths (the union of the review angles' `paths`) could not
/// be read, or a reviewed path is touched; `routine` otherwise, and when no
/// review angle carries paths.
pub fn classify(changed: Option<&[String]>, reviewed: Result<&[String], &str>) -> (Class, String) {
    let changed = match changed {
        Some(c) if !c.is_empty() => c,
        _ => {
            return (
                Class::Sensible,
                "the diff since the last release could not be read".into(),
            );
        }
    };
    let patterns = match reviewed {
        Ok(p) => p,
        Err(why) => {
            return (
                Class::Sensible,
                format!("the review angles' paths could not be read: {why}"),
            );
        }
    };
    match glob::hits(changed, patterns).first() {
        Some(hit) => (Class::Sensible, format!("touches the reviewed path {hit}")),
        None => (Class::Routine, "no reviewed path touched".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(sha: &str, branch: &str, event: &str, conclusion: Option<&str>) -> WorkflowRun {
        WorkflowRun {
            id: 1,
            head_sha: sha.into(),
            head_branch: branch.into(),
            event: event.into(),
            status: "completed".into(),
            conclusion: conclusion.map(str::to_string),
            attempt: 1,
            url: "https://github.com/example-org/acme/actions/runs/1".into(),
            created_at: None,
            updated_at: None,
        }
    }

    const TAG: &str = "acme-v1.2.3-rc.1";

    #[test]
    fn a_run_on_main_sharing_the_commit_is_not_a_certification() {
        let runs = [run("s", "main", "push", Some("success"))];
        assert!(gate_run(&runs, "s", TAG).is_none());
        assert!(!is_green(gate_run(&runs, "s", TAG)));
    }

    #[test]
    fn a_dispatch_rerun_is_not_the_tags_own_verdict() {
        let runs = [run("s", TAG, "workflow_dispatch", Some("success"))];
        assert!(gate_run(&runs, "s", TAG).is_none());
    }

    #[test]
    fn a_newer_red_run_hides_an_older_green() {
        let runs = [
            run("s", TAG, "push", Some("failure")),
            run("s", TAG, "push", Some("success")),
        ];
        assert!(!is_green(gate_run(&runs, "s", TAG)));
    }

    #[test]
    fn a_run_that_never_happened_or_never_concluded_is_not_green() {
        assert_eq!(
            detail(None, "release-gate.yml", TAG),
            "`release-gate.yml` never ran on `acme-v1.2.3-rc.1`"
        );
        let running = [run("s", TAG, "push", None)];
        assert!(!is_green(gate_run(&running, "s", TAG)));
        assert_eq!(
            detail(gate_run(&running, "s", TAG), "g.yml", TAG),
            "`g.yml` concluded `nothing yet`"
        );
    }

    #[test]
    fn the_gate_colour_is_an_allowlist() {
        assert_eq!(colour(Some("success")), Colour::Green);
        for red in [
            "failure",
            "timed_out",
            "startup_failure",
            "cancelled",
            "action_required",
        ] {
            assert_eq!(colour(Some(red)), Colour::Red, "{red}");
        }
        for other in [
            None,
            Some(""),
            Some("neutral"),
            Some("skipped"),
            Some("brand_new"),
        ] {
            assert_eq!(colour(other), Colour::Orange, "{other:?}");
        }
    }

    fn rerun(sha: &str, status: &str, at: &str) -> WorkflowRun {
        WorkflowRun {
            id: 9,
            head_sha: sha.into(),
            head_branch: TAG.into(),
            event: "workflow_run".into(),
            status: status.into(),
            conclusion: (status == "completed").then(|| "success".to_string()),
            attempt: 1,
            url: "https://github.com/example-org/acme/actions/runs/9".into(),
            created_at: crate::github::Timestamp::parse(at),
            updated_at: None,
        }
    }

    fn red(attempt: u64) -> WorkflowRun {
        WorkflowRun {
            attempt,
            created_at: crate::github::Timestamp::parse("2026-10-05T11:00:00Z"),
            ..run("s", TAG, "push", Some("failure"))
        }
    }

    /// Ten minutes after the red run of `red` completed, a 30-minute bound.
    fn soon() -> RerunWait {
        at("2026-10-05T11:10:00Z")
    }

    fn at(now: &str) -> RerunWait {
        RerunWait {
            now: Timestamp::parse(now).unwrap(),
            bound: time::Duration::minutes(30),
        }
    }

    #[test]
    fn a_red_first_attempt_waits_for_its_automatic_rerun_to_complete() {
        assert!(matches!(
            red_gate(&red(1), &[], &soon()),
            RedGate::Awaiting(_)
        ));
        let running = [rerun("s", "in_progress", "2026-10-05T11:20:00Z")];
        assert!(matches!(
            red_gate(&red(1), &running, &soon()),
            RedGate::Awaiting(_)
        ));
        let done = [rerun("s", "completed", "2026-10-05T11:20:00Z")];
        assert_eq!(red_gate(&red(1), &done, &soon()), RedGate::Settled);
        // Another commit's, or an older run's, is not this gate run's rerun.
        let other = [
            rerun("t", "completed", "2026-10-05T11:20:00Z"),
            rerun("s", "completed", "2026-10-05T10:00:00Z"),
        ];
        assert!(matches!(
            red_gate(&red(1), &other, &soon()),
            RedGate::Awaiting(_)
        ));
    }

    #[test]
    fn the_wait_for_a_rerun_that_never_shows_up_is_bounded() {
        // Completed at 11:40 (`updated_at`), created at 11:00.
        let completed = WorkflowRun {
            updated_at: Timestamp::parse("2026-10-05T11:40:00Z"),
            ..red(1)
        };
        // No rerun run before the bound: the rerun may still come.
        let before = red_gate(&completed, &[], &at("2026-10-05T12:09:59Z"));
        assert!(matches!(before, RedGate::Awaiting(_)), "{before:?}");
        assert!(!before.is_final());
        // From the bound on: final all the same, and said why.
        let after = red_gate(&completed, &[], &at("2026-10-05T12:10:00Z"));
        match &after {
            RedGate::Unseen(why) => assert!(why.contains("within 30 minutes"), "{why}"),
            other => panic!("{other:?}"),
        }
        assert!(after.is_final());
        // A rerun run that showed up and still runs keeps the wait past it.
        let running = [rerun("s", "in_progress", "2026-10-05T11:41:00Z")];
        assert!(matches!(
            red_gate(&completed, &running, &at("2026-10-05T14:00:00Z")),
            RedGate::Awaiting(_)
        ));
        // Without `updated_at`, the bound counts from `created_at`.
        assert!(red_gate(&red(1), &[], &at("2026-10-05T11:30:00Z")).is_final());
        // When the run completed cannot be read at all, it waits.
        let unknown = WorkflowRun {
            created_at: None,
            ..red(1)
        };
        assert!(matches!(
            red_gate(&unknown, &[], &at("2030-01-01T00:00:00Z")),
            RedGate::Awaiting(_)
        ));
    }

    #[test]
    fn a_red_rerun_attempt_is_final_and_a_pending_or_green_gate_is_not_red() {
        assert_eq!(red_gate(&red(2), &[], &soon()), RedGate::Settled);
        let pending = WorkflowRun {
            status: "in_progress".into(),
            conclusion: None,
            ..red(2)
        };
        assert_eq!(red_gate(&pending, &[], &soon()), RedGate::NotRed);
        let green = run("s", TAG, "push", Some("success"));
        assert_eq!(red_gate(&green, &[], &soon()), RedGate::NotRed);
    }

    #[test]
    fn the_failed_suites_are_read_from_the_summary_or_refused() {
        assert_eq!(
            failed_suites(r#"{"failed":["tests","lint"],"suites":[]}"#).unwrap(),
            vec!["tests".to_string(), "lint".to_string()]
        );
        assert_eq!(
            failed_suites(r#"{"failed":[]}"#).unwrap(),
            Vec::<String>::new()
        );
        assert!(failed_suites("{").is_err());
        assert!(failed_suites(r#"{"suites":[]}"#).is_err());
        assert!(failed_suites(r#"{"failed":[1]}"#).is_err());
    }

    #[test]
    fn the_class_is_read_from_the_annotation_only() {
        assert_eq!(
            class_of_annotation(Some(
                "Release candidate x\n\nRelease-Class: routine\nRelease-Class-Reason: docs"
            )),
            Class::Routine
        );
        assert_eq!(
            class_of_annotation(Some("Release-Class: sensible")),
            Class::Sensible
        );
        assert_eq!(
            class_of_annotation(Some("Release-Class: routine because")),
            Class::Unclassified,
            "anything appended reads as unclassified"
        );
        assert_eq!(
            class_of_annotation(Some("no class line")),
            Class::Unclassified
        );
        assert_eq!(
            class_of_annotation(None),
            Class::Unclassified,
            "a lightweight tag is unclassified"
        );
    }

    #[test]
    fn classification_fails_towards_sensible() {
        let reviewed = vec!["src/auth/**".to_string()];
        let docs = vec!["docs/a.md".to_string()];
        assert_eq!(classify(Some(&docs), Ok(&reviewed)).0, Class::Routine);
        let auth = vec!["src/auth/x.rs".to_string()];
        let (class, reason) = classify(Some(&auth), Ok(&reviewed));
        assert_eq!(class, Class::Sensible);
        assert_eq!(reason, "touches the reviewed path src/auth/x.rs");
        assert_eq!(classify(None, Ok(&reviewed)).0, Class::Sensible);
        assert_eq!(
            classify(Some(&[]), Ok(&reviewed)).0,
            Class::Sensible,
            "an empty diff is nobody telling"
        );
        assert_eq!(classify(Some(&docs), Err("absent")).0, Class::Sensible);
        assert_eq!(
            classify(Some(&auth), Ok(&[])).0,
            Class::Routine,
            "no review angle carries paths: no path is reviewed apart"
        );
    }
}
