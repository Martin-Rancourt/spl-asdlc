//! `spl-asdlc doctor`: each point of the setup procedure (`docs/setup/`)
//! checked through the bridge's own identities, reported `ok`, `missing` or
//! `invalid`. It reads; it never writes.

use std::fmt;

use crate::config::{Config, Source};
use crate::github::{GitHub, Read, RepoSlug, Ruleset};
use crate::method::business_rules;
use crate::method::release::shape::Shape;
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
        pr_review(config),
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
    out.extend(ruleset_checks(config, &default, &github.rulesets(&repo)));
    out.extend(tracker_checks(config, tracker));
    out
}

/// What reviews a PR (spec 04 §11): its base angles and what can add one,
/// or none, when no PR can get an angle and a PR merges on its CI.
fn pr_review(config: &Config) -> Check {
    let r = &config.review;
    if !r.angle_possible() {
        return check(
            "PR review",
            Verdict::Ok,
            "none — a PR merges on its CI and the other merge conditions, with no round and no coordinator approval",
        );
    }
    let mut parts = Vec::new();
    parts.push(if r.angles.is_empty() {
        "no base angle".to_string()
    } else {
        format!(
            "base angles {}",
            r.angles
                .iter()
                .map(|a| a.key.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    });
    for pa in &r.path_angles {
        parts.push(format!("{} on its paths", pa.angle));
    }
    if r.sensitive_paths.is_some() {
        parts.push(format!("{} on sensitive paths", r.sensitive_angle));
    }
    if let (true, Some(angle)) = (r.diff_size_extra_angle > 0, &r.diff_size_angle) {
        parts.push(format!(
            "{angle} above {} changed lines",
            r.diff_size_extra_angle
        ));
    }
    let detail = if r.angles.is_empty() {
        format!(
            "{}; a PR that gets no angle merges on its CI with no round",
            parts.join("; ")
        )
    } else {
        parts.join("; ")
    };
    check("PR review", Verdict::Ok, detail)
}

/// What a required ruleset must enforce on a ref.
#[derive(Debug, Clone, Copy)]
enum Need {
    /// A `required_status_checks` rule naming this context.
    Check(&'static str),
    /// A rule of this type, described for the report.
    Rule(&'static str, &'static str),
}

impl Need {
    fn met_by(self, rulesets: &[&Ruleset]) -> bool {
        rulesets.iter().any(|r| match self {
            Need::Check(c) => r.required_checks.contains(c),
            Need::Rule(kind, _) => r.rules.contains(kind),
        })
    }
}

impl fmt::Display for Need {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Need::Check(c) => write!(f, "the status check `{c}`"),
            Need::Rule(_, what) => f.write_str(what),
        }
    }
}

/// One ruleset of `docs/setup/repository.md`: the refs it must cover (samples
/// of what the configuration creates) and what it must enforce on each.
struct Required {
    name: String,
    target: &'static str,
    refs: Vec<String>,
    /// Tag templates which must be covered as whole languages, not merely by
    /// a few example tag names. Empty for branch rulesets.
    tag_templates: Vec<String>,
    needs: Vec<Need>,
}

/// One line per ruleset the procedure asks for: the default branch, the
/// release branches and the release and candidate tags (these two only with
/// a release cycle). Rulesets are read by
/// id, so that a release branch or a tag that does not exist yet is judged
/// against the configured shape. Only an
/// `active` ruleset counts; one that covers a ref but is `evaluate` or
/// `disabled` makes the line `invalid`. The bypass list is not judged: an
/// App's token reads it empty.
fn ruleset_checks(config: &Config, default: &str, rulesets: &Read<Vec<Ruleset>>) -> Vec<Check> {
    let mut required = vec![Required {
        name: format!("ruleset {default}"),
        target: "branch",
        refs: vec![format!("refs/heads/{default}")],
        tag_templates: Vec::new(),
        needs: vec![Need::Check("required-check")],
    }];
    // Without a release cycle there is no release branch nor release tag
    // to protect (spec 05, "Without a release cycle").
    if let Some(release) = &config.release {
        let shape = Shape::of(&config.product.name, release);
        let prefix = &release.branch_prefix;
        required.push(Required {
            name: format!("ruleset {prefix}**"),
            target: "branch",
            refs: ["2026-01-01-00-00-utc", "2031-12-31-23-59-utc"]
                .iter()
                .map(|stamp| format!("refs/heads/{prefix}{stamp}"))
                .collect(),
            tag_templates: Vec::new(),
            needs: vec![Need::Check("required-check")],
        });
        required.push(Required {
            name: "ruleset tags".to_string(),
            target: "tag",
            refs: Vec::new(),
            tag_templates: vec![
                format!("refs/tags/{}", shape.release_template),
                format!("refs/tags/{}", shape.candidate_template),
            ],
            needs: vec![
                Need::Rule("update", "updates restricted"),
                Need::Rule("deletion", "deletions restricted"),
            ],
        });
    }
    let default_ref = format!("refs/heads/{default}");
    required
        .iter()
        .map(|req| match rulesets {
            Ok(all) => judge(req, all, &default_ref),
            Err(e) => check(&req.name, Verdict::Missing, e.to_string()),
        })
        .collect()
}

fn judge(req: &Required, all: &[Ruleset], default_ref: &str) -> Check {
    let names = |rs: &[&Ruleset]| {
        rs.iter()
            .map(|r| format!("`{}`", r.name))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut used: Vec<&Ruleset> = Vec::new();
    let mut problems = Vec::new();
    let mut inactive_seen = false;
    let subjects = if req.tag_templates.is_empty() {
        req.refs.clone()
    } else {
        req.tag_templates.clone()
    };
    for git_ref in &subjects {
        let covering: Vec<&Ruleset> = all
            .iter()
            .filter(|r| {
                r.target == req.target
                    && if req.tag_templates.is_empty() {
                        covers(r, git_ref, default_ref)
                    } else {
                        covers_tag_template(r, git_ref)
                    }
            })
            .collect();
        let (active, inactive): (Vec<&Ruleset>, Vec<&Ruleset>) =
            covering.iter().partition(|r| r.is_active());
        let lacking: Vec<String> = req
            .needs
            .iter()
            .filter(|n| !n.met_by(&active))
            .map(|n| n.to_string())
            .collect();
        if lacking.is_empty() {
            for r in active {
                if !used.iter().any(|u| u.id == r.id) {
                    used.push(r);
                }
            }
            continue;
        }
        let mut problem = if covering.is_empty() {
            format!("{git_ref}: no {} ruleset covers it", req.target)
        } else {
            format!("{git_ref}: lacks {}", lacking.join(" and "))
        };
        if !inactive.is_empty() {
            inactive_seen = true;
            let states = inactive
                .iter()
                .map(|r| format!("`{}` is {}", r.name, r.enforcement))
                .collect::<Vec<_>>()
                .join(", ");
            problem.push_str(&format!(" ({states}, not active)"));
        }
        problems.push(problem);
    }
    if problems.is_empty() {
        let needs = req
            .needs
            .iter()
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(" and ");
        return check(
            &req.name,
            Verdict::Ok,
            format!("{needs}, enforced by {}", names(&used)),
        );
    }
    check(
        &req.name,
        if inactive_seen {
            Verdict::Invalid
        } else {
            Verdict::Missing
        },
        problems.join("; "),
    )
}

/// Whether a ruleset demonstrably covers every tag made by a configured
/// template. GitHub ruleset patterns are globs, whereas a template contains
/// unbounded decimal fields; testing representative versions cannot establish
/// containment. We therefore accept only forms whose containment is obvious:
/// `~ALL`, the template's own wildcard form, or its literal prefix followed by
/// one terminal `*`. Exclusions are conservatively not accepted, because an
/// arbitrary glob exclusion can remove a future version.
fn covers_tag_template(r: &Ruleset, template: &str) -> bool {
    if !r.exclude.is_empty() {
        return false;
    }
    let wildcard = template.replace("{version}", "*").replace("{rc}", "*");
    let literal_prefix = wildcard.split('*').next().unwrap_or_default();
    r.include.iter().any(|pattern| {
        pattern == "~ALL"
            || pattern == &wildcard
            // This documented ruleset shape covers any candidate tag: all
            // configured candidate templates end in `-rc.{rc}`.
            || (pattern == "refs/tags/*-rc.*" && template.ends_with("-rc.{rc}"))
            || pattern
                .strip_suffix('*')
                .is_some_and(|prefix| !prefix.contains('*') && literal_prefix.starts_with(prefix))
    })
}

/// Whether a ruleset's `ref_name` condition takes in `git_ref` (GitHub's
/// syntax: `~ALL`, `~DEFAULT_BRANCH`, fnmatch patterns on full refs, the
/// exclusions winning over the inclusions).
fn covers(r: &Ruleset, git_ref: &str, default_ref: &str) -> bool {
    let hit = |p: &String| match p.as_str() {
        "~ALL" => true,
        "~DEFAULT_BRANCH" => git_ref == default_ref,
        p => fnmatch(
            &p.chars().collect::<Vec<_>>(),
            &git_ref.chars().collect::<Vec<_>>(),
        ),
    };
    r.include.iter().any(hit) && !r.exclude.iter().any(hit)
}

/// fnmatch as GitHub applies it to rulesets (Ruby's `File.fnmatch` with
/// `FNM_PATHNAME`): `*` and `?` never match `/`, `**/` matches any number of
/// whole directories, `[…]` is a class (`!` or `^` negates), `\` escapes.
/// Anything else, braces included, is literal: an unknown syntax can only
/// make a ref look uncovered, never covered.
fn fnmatch(p: &[char], s: &[char]) -> bool {
    match p.first() {
        None => s.is_empty(),
        Some('*') if p.get(1) == Some(&'*') && p.get(2) == Some(&'/') => {
            let rest = &p[3..];
            fnmatch(rest, s)
                || s.iter()
                    .enumerate()
                    .any(|(i, c)| *c == '/' && fnmatch(rest, &s[i + 1..]))
        }
        Some('*') => {
            let rest = &p[p.iter().take_while(|c| **c == '*').count()..];
            for i in 0..=s.len() {
                if fnmatch(rest, &s[i..]) {
                    return true;
                }
                if s.get(i) == Some(&'/') {
                    break;
                }
            }
            false
        }
        Some('?') => s.first().is_some_and(|c| *c != '/') && fnmatch(&p[1..], &s[1..]),
        Some('[') => match s.first().map(|c| (*c, class(&p[1..], *c))) {
            Some((c, Some((hit, len)))) => c != '/' && hit && fnmatch(&p[1 + len..], &s[1..]),
            Some((c, None)) => c == '[' && fnmatch(&p[1..], &s[1..]),
            None => false,
        },
        Some('\\') if p.len() > 1 => s.first() == Some(&p[1]) && fnmatch(&p[2..], &s[1..]),
        Some(c) => s.first() == Some(c) && fnmatch(&p[1..], &s[1..]),
    }
}

/// Whether `c` is in the `[…]` class that `p` starts after its `[`, and the
/// class's length up to its `]` included; `None` when it is not closed (the
/// `[` is then literal).
fn class(p: &[char], c: char) -> Option<(bool, usize)> {
    let negated = matches!(p.first(), Some('!' | '^'));
    let mut i = usize::from(negated);
    let mut hit = false;
    let mut first = true;
    loop {
        let x = *p.get(i)?;
        if x == ']' && !first {
            break;
        }
        first = false;
        let (lo, next) = if x == '\\' {
            (*p.get(i + 1)?, i + 2)
        } else {
            (x, i + 1)
        };
        let (hi, next) = match (p.get(next), p.get(next + 1)) {
            (Some('-'), Some(hi)) if *hi != ']' => (*hi, next + 2),
            _ => (lo, next),
        };
        hit |= lo <= c && c <= hi;
        i = next;
    }
    Some((hit != negated, i + 1))
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
    // The PR pass's business-rule guard reads it, with or without a release
    // cycle.
    let registry = config.registry_path();
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
    // Without a release cycle, the release-only checks are skipped: the
    // project carries none of these files (spec 05, "Without a release
    // cycle").
    let Some(release) = &config.release else {
        out.push(check(
            "release cycle",
            Verdict::Ok,
            "none (no [release] section: a merged ticket is Done; the release-please files and the release workflows are not checked)",
        ));
        return out;
    };
    out.push(check(
        "release cycle",
        Verdict::Ok,
        format!("[release], {} angle(s)", release.angles.len()),
    ));
    // The base a candidate is measured from. None is no setup step: the
    // first release is measured from the repository's root.
    let shape = Shape::of(&config.product.name, release);
    out.push(match github.tags(repo, shape.release_tag_prefix()) {
        Ok(tags) => match shape.previous_release(tags.iter().map(|r| r.name.as_str()), None) {
            Some((tag, _)) => check(
                "release base",
                Verdict::Ok,
                format!("{tag}: the next candidate is measured from the highest release below its version"),
            ),
            None => check(
                "release base",
                Verdict::Ok,
                "no published release yet: the first candidate is measured from the repository's root (its whole history), with no tag to set by hand",
            ),
        },
        Err(e) => check("release base", Verdict::Missing, e.to_string()),
    });
    for path in [
        &release.release_please_config,
        &release.release_please_manifest,
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
        &release.cut_workflow,
        &release.gate_workflow,
        &release.gate_rerun_workflow,
        &release.release_please_workflow,
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
    // The hold label of an escalation: absent is fine, the bridge creates it
    // the first time it escalates.
    if let Some(release) = &config.release {
        let name = "release.escalation_label";
        let label = &release.escalation_label;
        out.push(match tracker.label_exists(label) {
            Ok(true) => check(name, Verdict::Ok, format!("{label} exists in the tracker")),
            Ok(false) => check(
                name,
                Verdict::Ok,
                format!("{label} does not exist yet: it will be created at the first escalation"),
            ),
            Err(e) => check(name, Verdict::Missing, e.to_string()),
        });
    }
    // The hands-off label: never required. A person adds it to an item (or,
    // under the same name, to a PR) to take it over by hand; the bridge never
    // creates it.
    let name = "bridge.hands_off_label";
    let label = &config.bridge.hands_off_label;
    out.push(match tracker.label_exists(label) {
        Ok(true) => check(
            name,
            Verdict::Ok,
            format!("{label}: an item or PR carrying it is left to people; it exists in the tracker"),
        ),
        Ok(false) => check(
            name,
            Verdict::Ok,
            format!("{label}: an item or PR carrying it is left to people; it does not exist in the tracker, which is fine until someone takes an item over"),
        ),
        Err(e) => check(
            name,
            Verdict::Ok,
            format!("{label}: an item or PR carrying it is left to people; whether the tracker has it could not be read ({e}), and it is never required"),
        ),
    });
    // The participants the bridge assigns items to: an unknown one leaves
    // its items unassigned, so it is reported here rather than at a tick.
    for (key, name) in participants(config) {
        out.push(match tracker.user(name) {
            Ok(u) => check(
                &key,
                Verdict::Ok,
                format!("{} is known to the tracker", u.name),
            ),
            Err(e) => check(
                &key,
                Verdict::Missing,
                format!("{e}: its items are created unassigned"),
            ),
        });
    }
    out
}

/// The configured assignees, named by their key: each angle's `assignee`
/// and each round's `coordinator_assignee`.
fn participants(config: &Config) -> Vec<(String, &str)> {
    let mut out = Vec::new();
    let release = config
        .release
        .as_ref()
        .map(|r| ("release", &r.angles, &r.coordinator_assignee));
    let review = Some((
        "review",
        &config.review.angles,
        &config.review.coordinator_assignee,
    ));
    for (table, angles, coordinator) in [release, review].into_iter().flatten() {
        for a in angles {
            if let Some(name) = &a.assignee {
                out.push((format!("{table}.angles.{}.assignee", a.key), name.as_str()));
            }
        }
        if let Some(name) = coordinator {
            out.push((format!("{table}.coordinator_assignee"), name.as_str()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::fake::FakeGitHub;
    use crate::github::{Identity, ReadError};
    use crate::tracker::User;
    use crate::tracker::fake::FakeTracker;

    fn config() -> Config {
        crate::config::spec_example::config()
    }

    #[test]
    fn each_point_of_the_procedure_is_ok_missing_or_invalid() {
        let mut c = config();
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
            s.rulesets.insert(repo.clone(), Ok(procedure_rulesets()));
            // A candidate, never a published release: the first release.
            s.tags.insert(
                repo.clone(),
                vec![crate::github::GitRef {
                    name: "acme-v0.1.0-rc.1".into(),
                    sha: "c1".into(),
                    object_type: crate::github::ObjectType::Tag,
                }],
            );
        }
        let t = FakeTracker::default();
        t.state.borrow_mut().users.push(User {
            id: "u-owner".into(),
            name: "owner-handle".into(),
        });
        t.state.borrow_mut().users.push(User {
            id: "u-coordinator".into(),
            name: "coordinator-handle".into(),
        });
        c.release.as_mut().unwrap().coordinator_assignee = Some("nobody-known".into());
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
        assert_eq!(by("ruleset main"), Verdict::Ok);
        assert_eq!(by("ruleset release/**"), Verdict::Ok);
        assert_eq!(by("ruleset tags"), Verdict::Ok);
        assert_eq!(by("tracker"), Verdict::Ok);
        assert_eq!(by("statuses"), Verdict::Ok);
        assert_eq!(by("owner"), Verdict::Ok);
        assert!(
            checks.iter().any(|c| c.to_string()
                == "release.escalation_label: ok — blocked-by-human does not exist yet: it will be created at the first escalation"),
            "{checks:#?}"
        );
        assert_eq!(by("review.coordinator_assignee"), Verdict::Ok);
        let unknown = checks
            .iter()
            .find(|c| c.name == "release.coordinator_assignee")
            .unwrap_or_else(|| panic!("{checks:#?}"));
        assert_eq!(unknown.verdict, Verdict::Missing, "{unknown}");
        assert!(unknown.detail.contains("unassigned"), "{unknown}");
        assert_eq!(by("release.angles.security.assignee"), Verdict::Missing);
        assert!(checks.iter().any(|c| {
            c.to_string()
                .starts_with("workflow release-cut.yml: missing")
        }));
        assert!(
            checks
                .iter()
                .any(|c| c.to_string() == "release cycle: ok — [release], 2 angle(s)"),
            "{checks:#?}"
        );
        // No published release is no setup step: the first candidate is
        // measured from the repository's root.
        assert!(
            checks.iter().any(|c| c.to_string()
                == "release base: ok — no published release yet: the first candidate is measured from the repository's root (its whole history), with no tag to set by hand"),
            "{checks:#?}"
        );
        g.state
            .borrow_mut()
            .tags
            .get_mut(&repo)
            .unwrap()
            .push(crate::github::GitRef {
                name: "acme-v0.1.0".into(),
                sha: "r1".into(),
                object_type: crate::github::ObjectType::Tag,
            });
        let checks = run(&c, None, &g, &t, "workstation");
        assert!(
            checks.iter().any(|c| c.to_string()
                == "release base: ok — acme-v0.1.0: the next candidate is measured from the highest release below its version"),
            "{checks:#?}"
        );
        // A tag list that cannot be read is not "no release".
        g.state.borrow_mut().tags.remove(&repo);
        let checks = run(&c, None, &g, &t, "workstation");
        assert!(
            checks
                .iter()
                .any(|c| c.name == "release base" && c.verdict == Verdict::Missing),
            "{checks:#?}"
        );
    }

    #[test]
    fn without_a_release_section_the_release_checks_are_skipped() {
        let mut c = config();
        c.release = None;
        let g = FakeGitHub::new();
        let repo = RepoSlug::parse("example-org/acme").unwrap();
        g.state
            .borrow_mut()
            .default_branch
            .insert(repo, "main".into());
        let t = FakeTracker::default();
        let checks = run(&c, None, &g, &t, "workstation");
        let cycle = checks
            .iter()
            .find(|c| c.name == "release cycle")
            .unwrap_or_else(|| panic!("{checks:#?}"));
        assert!(
            cycle.to_string().starts_with("release cycle: ok — none"),
            "{cycle}"
        );
        for skipped in [
            "release-please-config.json",
            ".release-please-manifest.json",
        ] {
            assert!(
                checks.iter().all(|c| c.name != skipped),
                "{skipped}: {checks:#?}"
            );
        }
        assert!(
            checks.iter().all(|c| !c.name.starts_with("workflow ")),
            "{checks:#?}"
        );
        assert!(
            checks.iter().all(|c| !c.name.starts_with("release.")),
            "no release participant: {checks:#?}"
        );
        // The PR pass's business-rule guard still reads the registry.
        assert!(checks.iter().any(|c| c.name == "business-rules"));
    }

    #[test]
    fn the_escalation_label_is_said_to_exist_once_the_tracker_has_it() {
        let c = config();
        let t = FakeTracker::default();
        t.ensure_label("blocked-by-human").unwrap();
        let checks = tracker_checks(&c, &t);
        let label = checks
            .iter()
            .find(|c| c.name == "release.escalation_label")
            .unwrap_or_else(|| panic!("{checks:#?}"));
        assert_eq!(
            label.to_string(),
            "release.escalation_label: ok — blocked-by-human exists in the tracker"
        );
    }

    #[test]
    fn the_hands_off_label_is_reported_and_never_required() {
        let c = config();
        let t = FakeTracker::default();
        let line = |t: &FakeTracker| {
            tracker_checks(&c, t)
                .into_iter()
                .find(|c| c.name == "bridge.hands_off_label")
                .unwrap_or_else(|| panic!("no hands-off line"))
        };
        let absent = line(&t);
        assert_eq!(absent.verdict, Verdict::Ok, "{absent}");
        assert!(
            absent.detail.starts_with("spl-asdlc:hands-off: ")
                && absent.detail.contains("does not exist in the tracker"),
            "{absent}"
        );
        t.ensure_label("spl-asdlc:hands-off").unwrap();
        let present = line(&t);
        assert_eq!(present.verdict, Verdict::Ok, "{present}");
        assert!(
            present.detail.ends_with("it exists in the tracker"),
            "{present}"
        );
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

    #[test]
    fn the_pr_review_is_reported_and_none_when_no_pr_can_get_an_angle() {
        let mut c = config();
        let line = pr_review(&c).to_string();
        assert!(
            line.starts_with("PR review: ok — base angles maintainability"),
            "{line}"
        );
        c.review.angles.clear();
        let line = pr_review(&c).to_string();
        assert!(line.contains("no base angle"), "{line}");
        assert!(line.contains("security on sensitive paths"), "{line}");
        c.review.path_angles.clear();
        c.review.sensitive_paths = None;
        c.review.diff_size_extra_angle = 0;
        c.review.coordinators.clear();
        assert_eq!(
            pr_review(&c).to_string(),
            "PR review: ok — none — a PR merges on its CI and the other merge conditions, with no round and no coordinator approval"
        );
        let t = FakeTracker::default();
        let checks = run(&c, None, &FakeGitHub::new(), &t, "workstation");
        assert!(
            checks
                .iter()
                .any(|x| x.name == "PR review" && x.detail.starts_with("none")),
            "{checks:#?}"
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

    // ---- rulesets (docs/setup/repository.md) ----

    fn ruleset(id: u64, name: &str, target: &str, include: &[&str], rules: &[&str]) -> Ruleset {
        Ruleset {
            id,
            name: name.into(),
            target: target.into(),
            enforcement: "active".into(),
            include: include.iter().map(|p| p.to_string()).collect(),
            exclude: Vec::new(),
            rules: rules.iter().map(|r| r.to_string()).collect(),
            required_checks: if rules.contains(&"required_status_checks") {
                ["required-check".to_string()].into()
            } else {
                Default::default()
            },
        }
    }

    const BRANCH_RULES: &[&str] = &[
        "pull_request",
        "required_status_checks",
        "non_fast_forward",
        "deletion",
    ];

    /// The three rulesets of the procedure, as the UI saves them.
    fn procedure_rulesets() -> Vec<Ruleset> {
        vec![
            ruleset(11, "main", "branch", &["~DEFAULT_BRANCH"], BRANCH_RULES),
            ruleset(
                12,
                "release",
                "branch",
                &["refs/heads/release/**"],
                BRANCH_RULES,
            ),
            ruleset(
                13,
                "tags",
                "tag",
                &["refs/tags/acme-v*"],
                &["update", "deletion"],
            ),
        ]
    }

    /// The `ruleset …` lines of a doctor run against these rulesets.
    fn ruleset_lines(rulesets: Read<Vec<Ruleset>>) -> Vec<Check> {
        let g = FakeGitHub::new();
        let repo = RepoSlug::parse("example-org/acme").unwrap();
        {
            let mut s = g.state.borrow_mut();
            s.identity = Some(Identity::App {
                slug: "spl-asdlc".into(),
                app_id: "1".into(),
            });
            s.default_branch.insert(repo.clone(), "main".into());
            s.rulesets.insert(repo, rulesets);
        }
        let checks = run(&config(), None, &g, &FakeTracker::default(), "workstation");
        let lines: Vec<Check> = checks
            .into_iter()
            .filter(|c| c.name.starts_with("ruleset"))
            .collect();
        assert_eq!(
            lines.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
            ["ruleset main", "ruleset release/**", "ruleset tags"],
            "one line per ruleset of the procedure"
        );
        lines
    }

    #[test]
    fn each_ruleset_of_the_procedure_has_its_own_line() {
        let lines = ruleset_lines(Ok(procedure_rulesets()));
        for l in &lines {
            assert_eq!(l.verdict, Verdict::Ok, "{l}");
        }
        assert_eq!(
            lines[0].to_string(),
            "ruleset main: ok — the status check `required-check`, enforced by `main`"
        );
        assert_eq!(
            lines[2].to_string(),
            "ruleset tags: ok — updates restricted and deletions restricted, enforced by `tags`"
        );
    }

    #[test]
    fn a_ruleset_nobody_created_is_missing() {
        let lines = ruleset_lines(Ok(Vec::new()));
        for l in &lines {
            assert_eq!(l.verdict, Verdict::Missing, "{l}");
        }
        assert!(
            lines[1]
                .detail
                .contains("refs/heads/release/2026-01-01-00-00-utc: no branch ruleset covers it"),
            "{}",
            lines[1]
        );
        assert!(
            lines[2]
                .detail
                .contains("refs/tags/acme-v{version}-rc.{rc}: no tag ruleset covers it"),
            "{}",
            lines[2]
        );
    }

    /// The finding of the first install: a `main` ruleset alone said `ok`.
    #[test]
    fn the_default_branch_ruleset_alone_leaves_the_others_missing() {
        let mut only_main = procedure_rulesets();
        only_main.truncate(1);
        let lines = ruleset_lines(Ok(only_main));
        assert_eq!(lines[0].verdict, Verdict::Ok, "{}", lines[0]);
        assert_eq!(lines[1].verdict, Verdict::Missing, "{}", lines[1]);
        assert_eq!(lines[2].verdict, Verdict::Missing, "{}", lines[2]);
    }

    #[test]
    fn a_ruleset_without_the_rule_says_what_is_missing() {
        let mut r = procedure_rulesets();
        r[1].rules.remove("required_status_checks");
        r[1].required_checks.clear();
        r[2].rules.remove("update");
        let lines = ruleset_lines(Ok(r));
        assert_eq!(lines[1].verdict, Verdict::Missing, "{}", lines[1]);
        assert!(
            lines[1]
                .detail
                .contains("lacks the status check `required-check`"),
            "{}",
            lines[1]
        );
        assert_eq!(lines[2].verdict, Verdict::Missing, "{}", lines[2]);
        assert!(
            lines[2]
                .detail
                .contains("refs/tags/acme-v{version}: lacks updates restricted"),
            "{}",
            lines[2]
        );
        assert!(!lines[2].detail.contains("deletions"), "{}", lines[2]);

        // A required check other than `required-check` is not it.
        let mut r = procedure_rulesets();
        r[0].required_checks = ["build".to_string()].into();
        assert_eq!(ruleset_lines(Ok(r))[0].verdict, Verdict::Missing);
    }

    #[test]
    fn a_ruleset_that_is_not_active_is_invalid_never_ok() {
        for enforcement in ["evaluate", "disabled"] {
            let mut r = procedure_rulesets();
            for x in &mut r {
                x.enforcement = enforcement.into();
            }
            let lines = ruleset_lines(Ok(r));
            for l in &lines {
                assert_eq!(l.verdict, Verdict::Invalid, "{l}");
                assert!(
                    l.detail.contains(&format!("is {enforcement}, not active")),
                    "{l}"
                );
            }
        }
        // An inactive duplicate does not spoil an active ruleset.
        let mut r = procedure_rulesets();
        let mut copy = r[0].clone();
        copy.id = 21;
        copy.enforcement = "evaluate".into();
        r.push(copy);
        assert_eq!(ruleset_lines(Ok(r))[0].verdict, Verdict::Ok);
    }

    #[test]
    fn the_tag_ruleset_must_cover_every_tag_the_configuration_creates() {
        // Exact sample refs prove nothing about future versions.
        let mut r = procedure_rulesets();
        r[2].include = vec![
            "refs/tags/acme-v0.1.0".into(),
            "refs/tags/acme-v0.1.0-rc.1".into(),
            "refs/tags/acme-v12.34.56".into(),
            "refs/tags/acme-v12.34.56-rc.12".into(),
        ];
        assert_eq!(ruleset_lines(Ok(r))[2].verdict, Verdict::Missing);

        // The procedure's `*-rc.*` alone leaves the release tags open.
        let mut r = procedure_rulesets();
        r[2].include = vec!["refs/tags/*-rc.*".into()];
        let line = ruleset_lines(Ok(r)).remove(2);
        assert_eq!(line.verdict, Verdict::Missing, "{line}");
        assert!(
            line.detail
                .contains("refs/tags/acme-v{version}: no tag ruleset covers it"),
            "{line}"
        );
        assert!(!line.detail.contains("{rc}:"), "{line}");

        // A pattern that only covers some versions.
        let mut r = procedure_rulesets();
        r[2].include = vec!["refs/tags/acme-v0.*".into()];
        let line = ruleset_lines(Ok(r)).remove(2);
        assert_eq!(line.verdict, Verdict::Missing, "{line}");
        assert!(line.detail.contains("acme-v{version}"), "{line}");

        // Another product's tags, or a branch ruleset on the tag patterns.
        let mut r = procedure_rulesets();
        r[2].include = vec!["refs/tags/other-v*".into()];
        assert_eq!(ruleset_lines(Ok(r))[2].verdict, Verdict::Missing);
        let mut r = procedure_rulesets();
        r[2].target = "branch".into();
        assert_eq!(ruleset_lines(Ok(r))[2].verdict, Verdict::Missing);

        // Two rulesets that together cover both shapes, and an exclusion.
        let mut r = procedure_rulesets();
        r[2].include = vec!["refs/tags/acme-v*-rc.*".into()];
        let mut finals = r[2].clone();
        finals.id = 14;
        finals.name = "final tags".into();
        finals.include = vec!["~ALL".into()];
        r.push(finals.clone());
        let line = ruleset_lines(Ok(r.clone())).remove(2);
        assert_eq!(line.verdict, Verdict::Ok, "{line}");
        r[3].exclude = vec!["refs/tags/acme-v12.*".into()];
        let line = ruleset_lines(Ok(r)).remove(2);
        assert_eq!(line.verdict, Verdict::Missing, "{line}");
        assert!(
            line.detail
                .contains("refs/tags/acme-v{version}: no tag ruleset covers it"),
            "{line}"
        );
    }

    #[test]
    fn the_release_branches_are_judged_without_one_existing() {
        // `~DEFAULT_BRANCH` is not the release branches.
        let mut r = procedure_rulesets();
        r[1].include = vec!["~DEFAULT_BRANCH".into()];
        assert_eq!(ruleset_lines(Ok(r))[1].verdict, Verdict::Missing);
        // `release/*` covers a release branch; `releases/**` does not.
        let mut r = procedure_rulesets();
        r[1].include = vec!["refs/heads/release/*".into()];
        assert_eq!(ruleset_lines(Ok(r))[1].verdict, Verdict::Ok);
        let mut r = procedure_rulesets();
        r[1].include = vec!["refs/heads/releases/**".into()];
        assert_eq!(ruleset_lines(Ok(r))[1].verdict, Verdict::Missing);
    }

    #[test]
    fn rulesets_that_cannot_be_read_are_never_ok() {
        let lines = ruleset_lines(Err(ReadError::unreadable(
            "rulesets of example-org/acme",
            "HTTP 502",
        )));
        for l in &lines {
            assert_eq!(l.verdict, Verdict::Missing, "{l}");
            assert!(l.detail.contains("could not be read"), "{l}");
        }
    }

    #[test]
    fn fnmatch_follows_githubs_pathname_rules() {
        let m = |p: &str, s: &str| {
            fnmatch(
                &p.chars().collect::<Vec<_>>(),
                &s.chars().collect::<Vec<_>>(),
            )
        };
        assert!(m("refs/heads/main", "refs/heads/main"));
        assert!(!m("refs/heads/main", "refs/heads/mainline"));
        assert!(m("refs/heads/release/*", "refs/heads/release/x"));
        assert!(!m("refs/heads/release/*", "refs/heads/release/x/y"));
        assert!(!m("refs/heads/*", "refs/heads/release/x"));
        assert!(m("refs/heads/**/*", "refs/heads/release/x/y"));
        assert!(m("refs/heads/**/x", "refs/heads/x"));
        assert!(m("refs/tags/*-rc.*", "refs/tags/acme-v1.2.3-rc.1"));
        assert!(!m("refs/tags/*-rc.*", "refs/tags/acme-v1.2.3"));
        assert!(m("refs/tags/v?.*", "refs/tags/v1.2"));
        assert!(m("refs/tags/v[0-9]*", "refs/tags/v1.2.3"));
        assert!(!m("refs/tags/v[!0-9]*", "refs/tags/v1.2.3"));
        assert!(m("refs/tags/a\\*", "refs/tags/a*"));
        assert!(!m("refs/tags/a\\*", "refs/tags/ab"));
        assert!(
            m("refs/tags/[x", "refs/tags/[x"),
            "an open class is literal"
        );
        assert!(
            !m("refs/tags/{a,b}", "refs/tags/a"),
            "braces are not expanded"
        );
    }
}
