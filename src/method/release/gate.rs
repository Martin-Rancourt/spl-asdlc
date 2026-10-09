//! Reading the gate and the class of a candidate (REQ-REL-010, 011, 034,
//! 035): the gate is the run of the gate workflow on the candidate's exact
//! commit, from its own tag push; the class travels in the tag annotation.

use crate::github::WorkflowRun;
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
}

/// Whether the gate run's red is final (the owner's decision, 2026-10-08):
/// the bridge cannot read the rerun decision, so it waits for what it can
/// see, fail-closed. A rerun attempt that completed red is final (one rerun
/// at most). A first attempt is final only once the rerun workflow ran for
/// it and every such run completed: `reruns` are that workflow's
/// `workflow_run` runs on the candidate's commit; until one exists, the
/// rerun may still come. The caller reads them before it reads the gate run
/// again, so that a rerun they made is seen.
pub fn red_gate(run: &WorkflowRun, reruns: &[WorkflowRun]) -> RedGate {
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
        return RedGate::Awaiting(format!(
            "the automatic rerun of {} has not run yet",
            run.url
        ));
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
/// the sensitive-path list could not be read, or a sensitive path is touched.
pub fn classify(changed: Option<&[String]>, sensitive: Result<&[String], &str>) -> (Class, String) {
    let changed = match changed {
        Some(c) if !c.is_empty() => c,
        _ => {
            return (
                Class::Sensible,
                "the diff since the last release could not be read".into(),
            );
        }
    };
    let patterns = match sensitive {
        Ok(p) if !p.is_empty() => p,
        Ok(_) => {
            return (
                Class::Sensible,
                "the review table could not be read: it names no path".into(),
            );
        }
        Err(why) => {
            return (
                Class::Sensible,
                format!("the review table could not be read: {why}"),
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
        }
    }

    fn red(attempt: u64) -> WorkflowRun {
        WorkflowRun {
            attempt,
            created_at: crate::github::Timestamp::parse("2026-10-05T11:00:00Z"),
            ..run("s", TAG, "push", Some("failure"))
        }
    }

    #[test]
    fn a_red_first_attempt_waits_for_its_automatic_rerun_to_complete() {
        assert!(matches!(red_gate(&red(1), &[]), RedGate::Awaiting(_)));
        let running = [rerun("s", "in_progress", "2026-10-05T11:20:00Z")];
        assert!(matches!(red_gate(&red(1), &running), RedGate::Awaiting(_)));
        let done = [rerun("s", "completed", "2026-10-05T11:20:00Z")];
        assert_eq!(red_gate(&red(1), &done), RedGate::Settled);
        // Another commit's, or an older run's, is not this gate run's rerun.
        let other = [
            rerun("t", "completed", "2026-10-05T11:20:00Z"),
            rerun("s", "completed", "2026-10-05T10:00:00Z"),
        ];
        assert!(matches!(red_gate(&red(1), &other), RedGate::Awaiting(_)));
    }

    #[test]
    fn a_red_rerun_attempt_is_final_and_a_pending_or_green_gate_is_not_red() {
        assert_eq!(red_gate(&red(2), &[]), RedGate::Settled);
        let pending = WorkflowRun {
            status: "in_progress".into(),
            conclusion: None,
            ..red(2)
        };
        assert_eq!(red_gate(&pending, &[]), RedGate::NotRed);
        let green = run("s", TAG, "push", Some("success"));
        assert_eq!(red_gate(&green, &[]), RedGate::NotRed);
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
        let sensitive = vec!["src/auth/**".to_string()];
        let docs = vec!["docs/a.md".to_string()];
        assert_eq!(classify(Some(&docs), Ok(&sensitive)).0, Class::Routine);
        let auth = vec!["src/auth/x.rs".to_string()];
        let (class, reason) = classify(Some(&auth), Ok(&sensitive));
        assert_eq!(class, Class::Sensible);
        assert_eq!(reason, "touches the reviewed path src/auth/x.rs");
        assert_eq!(classify(None, Ok(&sensitive)).0, Class::Sensible);
        assert_eq!(
            classify(Some(&[]), Ok(&sensitive)).0,
            Class::Sensible,
            "an empty diff is nobody telling"
        );
        assert_eq!(classify(Some(&docs), Err("absent")).0, Class::Sensible);
        assert_eq!(classify(Some(&docs), Ok(&[])).0, Class::Sensible);
    }
}
