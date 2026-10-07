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
