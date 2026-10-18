//! What the publication run of a published release says of it
//! (`docs/reference/05-release-cycle.md` §9): whether anything was deployed.
//! A release whose publication failed before deploying is retracted (its
//! GitHub release and its tag deleted), so that the next candidate takes its
//! version; one whose deploy or merge back ran is never retracted.
//!
//! The run is the project's caller of `asdlc-release-publish.yml`, called
//! twice: `verify`, the project's own publication and deployment jobs (which
//! `needs` the verify call), then `close`. GitHub names a job of a called
//! workflow `<caller job> / <called job>`; the called jobs of
//! `asdlc-release-publish.yml` are `verify`, `close` and `closing-guard`.
//! Every other job is the project's. Nothing was deployed when a verify job
//! concluded red and every job of the project, and every merge back, was
//! skipped; `closing-guard` deploys nothing, whatever it concluded.

use crate::github::{Job, WorkflowRun};

/// What a publication run says of its release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Publication {
    /// The run, or one of its jobs, has not concluded: wait.
    Running,
    /// Nothing was deployed: the release may be retracted.
    Undeployed { why: String },
    /// Delivered, or something ran past the verification, or nothing tells
    /// that nothing was deployed: never retracted.
    Kept { why: String },
}

/// The called job of `asdlc-release-publish.yml` a job is, by the last
/// segment of its name (`<caller job> / <called job>`); `None` for a job of
/// the project's own.
fn stage_job(name: &str) -> Option<&str> {
    let (_, called) = name.rsplit_once(" / ")?;
    ["verify", "close", "closing-guard"]
        .into_iter()
        .find(|j| *j == called)
}

/// Judge a publication run from its jobs (those of its latest attempt).
pub fn judge(run: &WorkflowRun, jobs: &[Job]) -> Publication {
    if run.status != "completed" {
        return Publication::Running;
    }
    let conclusion = run.conclusion.as_deref().unwrap_or("unknown");
    if conclusion == "success" {
        return Publication::Kept {
            why: "the publication succeeded".to_string(),
        };
    }
    if jobs.is_empty() {
        // A workflow GitHub could not start (a wiring error) ran nothing.
        return if matches!(conclusion, "failure" | "startup_failure") {
            Publication::Undeployed {
                why: format!(
                    "the publication run concluded `{conclusion}` without starting any job"
                ),
            }
        } else {
            Publication::Kept {
                why: format!(
                    "the publication run concluded `{conclusion}` with no job: nothing tells that nothing was deployed"
                ),
            }
        };
    }
    if jobs.iter().any(|j| j.conclusion.is_none()) {
        return Publication::Running;
    }
    let mut red_verify: Option<(&str, &str)> = None;
    for j in jobs {
        let c = j.conclusion.as_deref().unwrap_or_default();
        match stage_job(&j.name) {
            Some("closing-guard") => {}
            Some("verify") => match c {
                "skipped" => {}
                "failure" | "cancelled" | "timed_out" => red_verify = Some((&j.name, c)),
                _ => {
                    return Publication::Kept {
                        why: format!("its verification `{}` concluded `{c}`", j.name),
                    };
                }
            },
            Some(_) if c == "skipped" => {}
            Some(_) => {
                return Publication::Kept {
                    why: format!("its merge back `{}` ran (`{c}`)", j.name),
                };
            }
            None if c == "skipped" => {}
            None => {
                return Publication::Kept {
                    why: format!(
                        "the job `{}` ran (`{c}`): something may have been deployed",
                        j.name
                    ),
                };
            }
        }
    }
    match red_verify {
        Some((name, c)) => Publication::Undeployed {
            why: format!(
                "its verification `{name}` concluded `{c}`, so no publication or deployment job ran"
            ),
        },
        None => Publication::Kept {
            why: "no verification job of `asdlc-release-publish.yml` concluded red: nothing tells that nothing was deployed".to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(status: &str, conclusion: Option<&str>) -> WorkflowRun {
        WorkflowRun {
            id: 50,
            head_sha: "c1".into(),
            head_branch: "acme-v1.2.0".into(),
            event: "release".into(),
            status: status.into(),
            conclusion: conclusion.map(str::to_string),
            attempt: 1,
            url: "https://github.com/example-org/acme/actions/runs/50".into(),
            created_at: None,
            updated_at: None,
        }
    }

    fn job(name: &str, conclusion: Option<&str>) -> Job {
        Job {
            name: name.into(),
            conclusion: conclusion.map(str::to_string),
        }
    }

    /// The jobs of a run whose verify stage refused: as GitHub lists them.
    fn refused(publish: &str) -> Vec<Job> {
        vec![
            job("verify / verify", Some("failure")),
            job("verify / close", Some("skipped")),
            job("verify / closing-guard", Some("success")),
            job("publish", Some(publish)),
            job("close / verify", Some("skipped")),
            job("close / close", Some("skipped")),
            job("close / closing-guard", Some("failure")),
        ]
    }

    #[test]
    fn a_refused_verification_deployed_nothing() {
        let p = judge(&run("completed", Some("failure")), &refused("skipped"));
        assert!(
            matches!(&p, Publication::Undeployed { why } if why.contains("`verify / verify` concluded `failure`")),
            "{p:?}"
        );
    }

    #[test]
    fn a_run_that_started_no_job_deployed_nothing() {
        for c in ["failure", "startup_failure"] {
            assert!(matches!(
                judge(&run("completed", Some(c)), &[]),
                Publication::Undeployed { .. }
            ));
        }
        assert!(matches!(
            judge(&run("completed", Some("cancelled")), &[]),
            Publication::Kept { .. }
        ));
    }

    #[test]
    fn a_publication_that_ran_a_deploy_job_is_kept() {
        for c in ["success", "failure", "cancelled"] {
            let mut jobs = refused(c);
            jobs[0] = job("verify / verify", Some("success"));
            assert!(
                matches!(
                    judge(&run("completed", Some("failure")), &jobs),
                    Publication::Kept { .. }
                ),
                "{c}"
            );
        }
        // A project's job that ran beside a red verification.
        let p = judge(&run("completed", Some("failure")), &refused("failure"));
        assert!(
            matches!(&p, Publication::Kept { why } if why.contains("`publish` ran")),
            "{p:?}"
        );
    }

    #[test]
    fn a_merge_back_that_ran_is_kept() {
        let mut jobs = refused("skipped");
        jobs[5] = job("close / close", Some("failure"));
        assert!(matches!(
            judge(&run("completed", Some("failure")), &jobs),
            Publication::Kept { .. }
        ));
    }

    #[test]
    fn a_running_or_successful_publication_is_never_retracted() {
        assert_eq!(
            judge(&run("in_progress", None), &refused("skipped")),
            Publication::Running
        );
        let mut jobs = refused("skipped");
        jobs[3] = job("publish", None);
        assert_eq!(
            judge(&run("completed", Some("failure")), &jobs),
            Publication::Running
        );
        assert!(matches!(
            judge(&run("completed", Some("success")), &[]),
            Publication::Kept { .. }
        ));
    }

    #[test]
    fn a_run_with_no_red_verification_is_kept() {
        let jobs = vec![
            job("publish", Some("skipped")),
            job("verify", Some("failure")),
        ];
        assert!(
            matches!(
                judge(&run("completed", Some("failure")), &jobs),
                Publication::Kept { .. }
            ),
            "a bare `verify` is the project's own job, not the stage's"
        );
    }
}
