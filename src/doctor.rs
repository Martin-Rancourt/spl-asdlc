//! `spl-asdlc doctor`: each point of the setup procedure (`docs/setup/`)
//! checked through the bridge's own identities, reported `ok`, `missing` or
//! `invalid`. It reads; it never writes.

use std::fmt;

use crate::config::{Config, Source};
use crate::github::{GitHub, RepoSlug};
use crate::method::business_rules;
use crate::method::review;
use crate::tracker::{StatusTable, Tracker};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Ok,
    Missing,
    Invalid,
}

impl fmt::Display for Verdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Verdict::Ok => "ok",
            Verdict::Missing => "missing",
            Verdict::Invalid => "invalid",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub name: String,
    pub verdict: Verdict,
    pub detail: String,
}

impl fmt::Display for Check {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {} — {}", self.name, self.verdict, self.detail)
    }
}

fn check(name: &str, verdict: Verdict, detail: impl Into<String>) -> Check {
    Check {
        name: name.to_string(),
        verdict,
        detail: detail.into(),
    }
}

/// Every check, in the order of the procedure. `instance` is the instance
/// overlay merged into `config`, if any: the repository's file at the
/// default branch is judged merged with it, as the bridge reads it.
pub fn run(
    config: &Config,
    instance: Option<&Source>,
    github: &dyn GitHub,
    tracker: &dyn Tracker,
    host: &str,
) -> Vec<Check> {
    let mut out = vec![
        check("bridge-host", Verdict::Ok, host),
        check(
            "instance overlay",
            Verdict::Ok,
            instance.map_or("none", |i| i.label.as_str()),
        ),
    ];
    let Some(repo) = RepoSlug::parse(&config.product.repository) else {
        out.push(check(
            "repository",
            Verdict::Invalid,
            "product.repository is not <owner>/<repo>",
        ));
        return out;
    };
    out.push(match github.identity() {
        Ok(i) => check("github-app", Verdict::Ok, i.to_string()),
        Err(e) => check("github-app", Verdict::Missing, e.to_string()),
    });
    let default = match github.default_branch(&repo) {
        Ok(b) => {
            out.push(check(
                "repository",
                Verdict::Ok,
                format!("{repo}, default branch {b}"),
            ));
            b
        }
        Err(e) => {
            out.push(check(
                "repository",
                Verdict::Missing,
                format!("{repo}: {e}"),
            ));
            return out;
        }
    };
    out.extend(files(config, instance, github, &repo, &default));
    out.push(match github.required_checks(&repo, &default) {
        Ok(r) if r.contexts.contains("required-check") => check(
            "ruleset",
            Verdict::Ok,
            format!(
                "{default} requires `required-check`{}",
                if r.strict { ", up to date" } else { "" }
            ),
        ),
        Ok(r) => check(
            "ruleset",
            Verdict::Missing,
            format!(
                "{default} does not require `required-check` (required: {})",
                r.contexts.iter().cloned().collect::<Vec<_>>().join(", ")
            ),
        ),
        Err(e) => check("ruleset", Verdict::Missing, e.to_string()),
    });
    out.extend(tracker_checks(config, tracker));
    out
}

fn files(
    config: &Config,
    instance: Option<&Source>,
    github: &dyn GitHub,
    repo: &RepoSlug,
    rev: &str,
) -> Vec<Check> {
    let mut out = Vec::new();
    let read = |path: &str| github.file(repo, path, rev);
    out.push(match read(crate::config::FILE_NAME) {
        Ok(text) => match Config::parse_with(
            &Source {
                label: format!("{} at {rev}", crate::config::FILE_NAME),
                text,
            },
            instance,
        ) {
            Ok(_) => check("spl-asdlc.toml", Verdict::Ok, format!("valid at {rev}")),
            Err(e) => check(
                "spl-asdlc.toml",
                Verdict::Invalid,
                e.0.iter()
                    .map(|x| x.to_string())
                    .collect::<Vec<_>>()
                    .join("; "),
            ),
        },
        Err(e) if e.is_not_found() => {
            check("spl-asdlc.toml", Verdict::Missing, format!("not at {rev}"))
        }
        Err(e) => check("spl-asdlc.toml", Verdict::Missing, e.to_string()),
    });
    let registry = &config.release.registry_path;
    out.push(match read(registry) {
        Ok(text) => {
            let dup = business_rules::duplicate_registry_ids(&text);
            if dup.is_empty() {
                check(
                    "business-rules",
                    Verdict::Ok,
                    format!(
                        "{registry}: {} rule(s)",
                        business_rules::registry_ids(&text).len()
                    ),
                )
            } else {
                check(
                    "business-rules",
                    Verdict::Invalid,
                    format!("{registry}: ids declared twice: {}", dup.join(", ")),
                )
            }
        }
        Err(e) => check(
            "business-rules",
            Verdict::Missing,
            format!("{registry}: {e}"),
        ),
    });
    if let Some(path) = &config.review.sensitive_paths {
        out.push(match read(path) {
            Ok(text) => match review::parse_sensitive_paths(&text) {
                Ok(p) => check(
                    "review-paths",
                    Verdict::Ok,
                    format!("{path}: {} glob(s)", p.len()),
                ),
                Err(e) => check("review-paths", Verdict::Invalid, format!("{path}: {e}")),
            },
            Err(e) => check(
                "review-paths",
                Verdict::Missing,
                format!("{path}: {e} — every PR waits until it is fixed"),
            ),
        });
    }
    for path in [
        &config.release.release_please_config,
        &config.release.release_please_manifest,
    ] {
        let path = path.as_str();
        out.push(match read(path) {
            Ok(text) if serde_json::from_str::<serde_json::Value>(&text).is_ok() => {
                check(path, Verdict::Ok, "valid JSON")
            }
            Ok(_) => check(path, Verdict::Invalid, "not JSON"),
            Err(e) => check(path, Verdict::Missing, e.to_string()),
        });
    }
    for wf in [
        &config.release.cut_workflow,
        &config.release.gate_workflow,
        &config.release.release_please_workflow,
    ] {
        let path = format!(".github/workflows/{wf}");
        out.push(match read(&path) {
            Ok(_) => check(&format!("workflow {wf}"), Verdict::Ok, path),
            Err(e) => check(&format!("workflow {wf}"), Verdict::Missing, e.to_string()),
        });
    }
    out
}

fn tracker_checks(config: &Config, tracker: &dyn Tracker) -> Vec<Check> {
    let mut out = Vec::new();
    match tracker.identity() {
        Ok(me)
            if me
                .name
                .eq_ignore_ascii_case(&config.tracker.service_identity) =>
        {
            out.push(check(
                "tracker",
                Verdict::Ok,
                format!("writing as {}", me.name),
            ));
        }
        Ok(me) => out.push(check(
            "tracker",
            Verdict::Invalid,
            format!(
                "writing as {}, the configuration names {}",
                me.name, config.tracker.service_identity
            ),
        )),
        Err(e) => out.push(check("tracker", Verdict::Missing, e.to_string())),
    }
    let table = StatusTable::identity();
    out.push(match tracker.status_names() {
        Ok(names) => {
            let missing = table.missing(&names);
            if missing.is_empty() {
                check(
                    "statuses",
                    Verdict::Ok,
                    "the tracker names every canonical status",
                )
            } else {
                check(
                    "statuses",
                    Verdict::Missing,
                    format!(
                        "the tracker lacks {}: it must name each canonical status exactly so",
                        missing
                            .iter()
                            .map(|s| table.tracker_name(*s))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                )
            }
        }
        Err(e) => check("statuses", Verdict::Missing, e.to_string()),
    });
    out.push(match tracker.user(&config.owner.tracker_user) {
        Ok(u) => check(
            "owner",
            Verdict::Ok,
            format!("{} is known to the tracker", u.name),
        ),
        Err(e) => check("owner", Verdict::Missing, e.to_string()),
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::fake::FakeGitHub;
    use crate::github::{Identity, RequiredChecks};
    use crate::tracker::User;
    use crate::tracker::fake::FakeTracker;

    fn config() -> Config {
        crate::config::spec_example::config()
    }

    #[test]
    fn each_point_of_the_procedure_is_ok_missing_or_invalid() {
        let c = config();
        let g = FakeGitHub::new();
        let repo = RepoSlug::parse("example-org/acme").unwrap();
        {
            let mut s = g.state.borrow_mut();
            s.identity = Some(Identity::App {
                slug: "spl-asdlc".into(),
                app_id: "1".into(),
            });
            s.default_branch.insert(repo.clone(), "main".into());
            let mut file = |p: &str, text: &str| {
                s.files
                    .insert((repo.clone(), "main".into(), p.into()), Ok(text.into()));
            };
            file(
                "docs/business-rules.md",
                "| BR-ACME-001 | a | b | c | Enforced |\n",
            );
            file(".github/review-paths.json", "{\"paths\": []}");
            file("release-please-config.json", "{}");
            s.required_checks.insert(
                (repo.clone(), "main".into()),
                Ok(RequiredChecks {
                    contexts: ["required-check".to_string()].into(),
                    strict: true,
                }),
            );
        }
        let t = FakeTracker::default();
        t.state.borrow_mut().users.push(User {
            id: "u-owner".into(),
            name: "owner-handle".into(),
        });
        let checks = run(&c, None, &g, &t, "workstation");
        let by = |n: &str| {
            checks
                .iter()
                .find(|c| c.name == n)
                .unwrap_or_else(|| panic!("{n}: {checks:#?}"))
                .verdict
        };
        assert_eq!(by("github-app"), Verdict::Ok);
        assert!(
            checks
                .iter()
                .any(|c| c.to_string() == "instance overlay: ok — none"),
            "{checks:#?}"
        );
        assert_eq!(by("spl-asdlc.toml"), Verdict::Missing);
        assert_eq!(by("business-rules"), Verdict::Ok);
        assert_eq!(
            by("review-paths"),
            Verdict::Invalid,
            "an empty list makes nothing sensitive"
        );
        assert_eq!(by("release-please-config.json"), Verdict::Ok);
        assert_eq!(by(".release-please-manifest.json"), Verdict::Missing);
        assert_eq!(by("ruleset"), Verdict::Ok);
        assert_eq!(by("tracker"), Verdict::Ok);
        assert_eq!(by("statuses"), Verdict::Ok);
        assert_eq!(by("owner"), Verdict::Ok);
        assert!(checks.iter().any(|c| {
            c.to_string()
                .starts_with("workflow release-cut.yml: missing")
        }));
    }

    #[test]
    fn a_canonical_status_the_team_lacks_is_reported_by_its_exact_name() {
        let c = config();
        let mut t = FakeTracker::default();
        t.status_names.retain(|n| n != "To Deploy");
        t.status_names.push("To deploy".into());
        let checks = tracker_checks(&c, &t);
        let statuses = checks.iter().find(|c| c.name == "statuses").unwrap();
        assert_eq!(statuses.verdict, Verdict::Missing);
        assert_eq!(
            statuses.to_string(),
            "statuses: missing — the tracker lacks To Deploy: it must name each canonical status exactly so"
        );
    }

    /// The repository's file at the default branch is neutral: the doctor
    /// judges it merged with the overlay it was given, as the bridge reads it.
    #[test]
    fn the_file_at_the_default_branch_is_judged_with_the_instance_overlay() {
        use crate::config::{INSTANCE_FILE_NAME, spec_example};
        let g = FakeGitHub::new();
        let repo = RepoSlug::parse("example-org/acme").unwrap();
        {
            let mut s = g.state.borrow_mut();
            s.identity = Some(Identity::App {
                slug: "spl-asdlc".into(),
                app_id: "1".into(),
            });
            s.default_branch.insert(repo.clone(), "main".into());
            s.files.insert(
                (repo.clone(), "main".into(), "spl-asdlc.toml".into()),
                Ok(spec_example::repository().into()),
            );
        }
        let t = FakeTracker::default();
        let overlay = spec_example::source(INSTANCE_FILE_NAME, spec_example::instance());
        let verdict = |instance: Option<&Source>| {
            let checks = run(&config(), instance, &g, &t, "workstation");
            let line = |n: &str| {
                checks
                    .iter()
                    .find(|c| c.name == n)
                    .unwrap_or_else(|| panic!("{n}: {checks:#?}"))
                    .clone()
            };
            (line("instance overlay"), line("spl-asdlc.toml"))
        };
        let (instance, file) = verdict(Some(&overlay));
        assert_eq!(
            instance.to_string(),
            "instance overlay: ok — spl-asdlc.instance.toml"
        );
        assert_eq!(file.verdict, Verdict::Ok, "{file}");
        let (instance, file) = verdict(None);
        assert_eq!(instance.to_string(), "instance overlay: ok — none");
        assert_eq!(file.verdict, Verdict::Invalid, "{file}");
        assert!(file.detail.contains("product.repository"), "{file}");
    }
}
