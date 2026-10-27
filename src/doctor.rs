//! `spl-asdlc doctor`: each point of the setup procedure (`docs/setup/`)
//! checked through the bridge's own identities, reported `ok`, `missing` or
//! `invalid`, each check not `ok` with the gesture that fixes it and the
//! page of `docs/setup/` that describes it. It reads; it never writes.

use std::fmt;

use serde::Serialize;

use crate::config::{Config, Source};
use crate::github::{CompareStatus, GitHub, Read, RepoSlug, Ruleset};
use crate::method::business_rules;
use crate::method::pin::{self, Pin, PinError, WORKFLOWS};
use crate::method::release::shape::Shape;
use crate::method::texts::protocol::{Job, angle_path, retired_in};
use crate::method::workflow_callers::reusable_callers;
use crate::tracker::{StatusTable, Tracker};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Check {
    pub name: String,
    #[serde(rename = "state")]
    pub verdict: Verdict,
    /// What was found, and why it is not ok when it is not.
    pub detail: String,
    /// The gesture that fixes a check that is not `ok`; `None` when it is.
    pub fix: Option<Fix>,
}

/// How a check that is not `ok` is fixed: a sentence, and the page of
/// `docs/setup/` that describes the step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Fix {
    pub gesture: String,
    pub page: String,
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
        fix: None,
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
    run_embedded(config, instance, github, tracker, host, None)
}

/// [`run`] for a host that embeds this crate at `embedded` (a version
/// `X.Y.Z`): `embedded version` also says whether the workflow callers
/// name its minor line `vX.Y`, so that the method's texts and workflows a
/// project runs are those of the bridge the host runs.
pub fn run_embedded(
    config: &Config,
    instance: Option<&Source>,
    github: &dyn GitHub,
    tracker: &dyn Tracker,
    host: &str,
    embedded: Option<&str>,
) -> Vec<Check> {
    let mut out = checks(config, instance, github, tracker, host, embedded);
    for c in &mut out {
        if c.verdict != Verdict::Ok {
            c.fix = Some(fix_of(&c.name, config));
        }
    }
    out
}

fn checks(
    config: &Config,
    instance: Option<&Source>,
    github: &dyn GitHub,
    tracker: &dyn Tracker,
    host: &str,
    embedded: Option<&str>,
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
    out.push(pause_check(github, &repo));
    out.extend(files(config, instance, github, &repo, &default));
    out.push(method_checks(github, &repo, &default));
    out.extend(method_version(config, github, &repo, &default, embedded));
    out.push(branches_and_prs(config, github, &repo, &default));
    out.extend(ruleset_checks(config, &default, &github.rulesets(&repo)));
    out.extend(tracker_checks(config, tracker));
    out
}

/// The pause switch (`crate::tick::pause`): a paused project is reported,
/// as is a switch the App cannot read, which pauses nothing.
fn pause_check(github: &dyn GitHub, repo: &RepoSlug) -> Check {
    use crate::tick::pause::{self, Switch};
    let name = "pause";
    let var = pause::VARIABLE;
    match pause::switch(github, repo) {
        Switch::Off => check(
            name,
            Verdict::Ok,
            format!("not paused ({var} unset or false)"),
        ),
        Switch::On(v) => check(
            name,
            Verdict::Ok,
            format!(
                "paused: {var} is true{}; the bridge writes nothing for {repo} until it is unset or false",
                v.updated_at
                    .map(|at| format!(" (set {at})"))
                    .unwrap_or_default()
            ),
        ),
        Switch::Other(value) => check(
            name,
            Verdict::Invalid,
            format!("{var} is \"{value}\": only `true` pauses the bridge, which acts"),
        ),
        Switch::Unreadable(e) => check(
            name,
            Verdict::Missing,
            format!(
                "{e}: the GitHub App needs Variables: read; until then the bridge acts as not paused"
            ),
        ),
    }
}

/// What reviews a PR (spec 04 §11): its angles on every PR and what can add one,
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
    let every: Vec<&str> = r
        .angles
        .iter()
        .filter(|a| a.paths.is_empty())
        .map(|a| a.key.as_str())
        .collect();
    parts.push(if every.is_empty() {
        "no angle on every PR".to_string()
    } else {
        format!("on every PR: {}", every.join(", "))
    });
    for a in r.angles.iter().filter(|a| !a.paths.is_empty()) {
        parts.push(format!("{} on its paths", a.key));
    }
    if let (true, Some(angle)) = (r.diff_size_extra_angle > 0, &r.diff_size_angle) {
        parts.push(format!(
            "{angle} above {} changed lines",
            r.diff_size_extra_angle
        ));
    }
    let detail = if every.is_empty() {
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
    /// A `required_status_checks` rule naming a check of this job of a
    /// reusable workflow, `<caller job> / <job>` (or the bare job name).
    CalledCheck(&'static str),
    /// A rule of this type, described for the report.
    Rule(&'static str, &'static str),
}

impl Need {
    fn met_by(self, rulesets: &[&Ruleset]) -> bool {
        rulesets.iter().any(|r| match self {
            Need::Check(c) => r.required_checks.contains(c),
            Need::CalledCheck(job) => r
                .required_checks
                .iter()
                .any(|c| c == job || c.ends_with(&format!(" / {job}"))),
            Need::Rule(kind, _) => r.rules.contains(kind),
        })
    }
}

impl fmt::Display for Need {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Need::Check(c) => write!(f, "the status check `{c}`"),
            Need::CalledCheck(job) => write!(f, "the status check `… / {job}`"),
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
/// release branches and the release and candidate tags. Rulesets are read by
/// id, so that a release branch or a tag that does not exist yet is judged
/// against the configured shape. Only an
/// `active` ruleset counts; one that covers a ref but is `evaluate` or
/// `disabled` makes the line `invalid`. The bypass list is not judged: an
/// App's token reads it empty.
const BRANCHES_AND_PRS: &str = "branches and PRs";

/// The branches and open PRs that would block the method once the rulesets
/// are on (BR-ASDLC-039), read before the ruleset checks: a branch under the
/// release prefix that is no release branch of the method, or one whose
/// every commit is in the default branch (a finished cycle's leftover),
/// keeps the cut refused, and deleting it then needs a bypass; an open PR
/// whose title the title check refuses cannot merge. A release branch with
/// commits of its own is a cycle in flight, never flagged.
fn branches_and_prs(config: &Config, github: &dyn GitHub, repo: &RepoSlug, default: &str) -> Check {
    let shape = Shape::of(&config.product.name, &config.release);
    let prefix = &config.release.branch_prefix;
    let branches = match github.branches(repo, prefix) {
        Ok(b) => b,
        Err(e) => {
            return check(
                BRANCHES_AND_PRS,
                Verdict::Missing,
                format!("the branches under {prefix}: {e}"),
            );
        }
    };
    let mut found = Vec::new();
    for b in &branches {
        if !shape.is_release_branch(&b.name) {
            found.push(format!(
                "branch {}: under {prefix} but no release branch of the method ({prefix}<YYYY-MM-DD-HH-MM>-utc), so the cut refuses while it exists",
                b.name
            ));
            continue;
        }
        match github.compare_status(repo, default, &b.name) {
            Ok(CompareStatus::Behind | CompareStatus::Identical) => found.push(format!(
                "branch {}: every commit of it is in {default}, a leftover of a finished cycle that the cut takes as in flight",
                b.name
            )),
            Ok(_) => {}
            Err(e) => {
                return check(
                    BRANCHES_AND_PRS,
                    Verdict::Missing,
                    format!("{default}...{}: {e}", b.name),
                );
            }
        }
    }
    let pulls = match github.open_pulls(repo, None) {
        Ok(p) => p,
        Err(e) => {
            return check(
                BRANCHES_AND_PRS,
                Verdict::Missing,
                format!("the open PRs: {e}"),
            );
        }
    };
    let types: Vec<&str> = config
        .release
        .changelog_sections
        .iter()
        .map(|c| c.kind.as_str())
        .collect();
    for pr in &pulls {
        if !title_allowed(&pr.title, &types) {
            found.push(format!(
                "PR #{} \"{}\": its title is not <type>(<scope>)?!?: <summary> with a type of {}, so the title check refuses it",
                pr.number,
                pr.title,
                types.join(", ")
            ));
        }
    }
    if found.is_empty() {
        return check(
            BRANCHES_AND_PRS,
            Verdict::Ok,
            format!(
                "{} branch(es) under {prefix}, {} open PR(s): none would block the method",
                branches.len(),
                pulls.len()
            ),
        );
    }
    check(BRANCHES_AND_PRS, Verdict::Invalid, found.join("; "))
}

/// Whether the PR title check (`asdlc-pr-title.yml`) accepts a title:
/// `<type>(<scope>)?!?: <summary>`, a type of `types`, a scope with no space
/// or parenthesis, a summary that neither starts nor ends with a space.
fn title_allowed(title: &str, types: &[&str]) -> bool {
    let Some((header, summary)) = title.split_once(": ") else {
        return false;
    };
    let header = header.strip_suffix('!').unwrap_or(header);
    let (kind, scope) = match header.split_once('(') {
        Some((k, rest)) => match rest.strip_suffix(')') {
            Some(scope) => (k, Some(scope)),
            None => return false,
        },
        None => (header, None),
    };
    let scope_ok = scope.is_none_or(|s| {
        !s.is_empty() && !s.chars().any(|c| c.is_whitespace() || c == '(' || c == ')')
    });
    types.contains(&kind) && scope_ok && !summary.is_empty() && summary.trim() == summary
}

fn ruleset_checks(config: &Config, default: &str, rulesets: &Read<Vec<Ruleset>>) -> Vec<Check> {
    let mut required = vec![Required {
        name: format!("ruleset {default}"),
        target: "branch",
        refs: vec![format!("refs/heads/{default}")],
        tag_templates: Vec::new(),
        needs: vec![
            Need::Check("required-check"),
            Need::CalledCheck(TITLE_CHECK),
        ],
    }];
    {
        let release = &config.release;
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
            needs: vec![
                Need::Check("required-check"),
                Need::CalledCheck(TITLE_CHECK),
            ],
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

/// The business-rule registry's files at `rev`, `(path, text)`: `None`
/// when the directory is absent (Git keeps no empty directory, and the
/// first rule creates it); an error says what could not be read.
type RegistryRead = Result<Option<Vec<(String, String)>>, String>;

fn read_registry_files(github: &dyn GitHub, repo: &RepoSlug, rev: &str) -> RegistryRead {
    let dir = business_rules::REGISTRY_DIR;
    let names = match github.dir(repo, dir, rev) {
        Ok(names) => names,
        Err(e) if e.is_not_found() => return Ok(None),
        Err(e) => return Err(format!("{dir}/: {e}")),
    };
    let mut files = Vec::new();
    for name in names {
        let path = format!("{dir}/{name}");
        match github.file(repo, &path, rev) {
            Ok(text) => files.push((path, text)),
            Err(e) => return Err(format!("{path}: {e}")),
        }
    }
    Ok(Some(files))
}

/// The business-rule registry at `rev`, every file checked: each invalid
/// one is named by its path. Absent is fine.
fn registry_check(registry: &RegistryRead, rev: &str) -> Check {
    const NAME: &str = "business-rules";
    let dir = business_rules::REGISTRY_DIR;
    let files = match registry {
        Ok(Some(files)) => files,
        Ok(None) => return check(NAME, Verdict::Ok, format!("{dir}/: no rule yet at {rev}")),
        Err(e) => return check(NAME, Verdict::Missing, e.clone()),
    };
    let read = business_rules::parse_registry(files);
    if read.invalid.is_empty() {
        check(
            NAME,
            Verdict::Ok,
            format!("{dir}/: {} rule(s)", read.rules.len()),
        )
    } else {
        check(
            NAME,
            Verdict::Invalid,
            read.invalid
                .iter()
                .map(|(path, why)| format!("{path}: {why}"))
                .collect::<Vec<_>>()
                .join("; "),
        )
    }
}

/// Whether the tests each `Enforced` rule cites exist at `rev` (spec 02 §3,
/// SR-010; `crate::method::rule_tests`): `invalid` names each cited file
/// that is absent and each name absent from its file. A name whose file
/// cannot be located is counted, never a failure. A rule file whose format
/// is wrong is the `business-rules` line's.
fn rule_tests_check(
    registry: &RegistryRead,
    github: &dyn GitHub,
    repo: &RepoSlug,
    rev: &str,
) -> Check {
    match registry {
        Ok(Some(files)) => rule_tests_verdict(files, rev, |path: &str| {
            match github.file(repo, path, rev) {
                Ok(text) => Ok(Some(text)),
                Err(e) if e.is_not_found() => Ok(None),
                Err(e) => Err(format!("{path}: {e}")),
            }
        }),
        Ok(None) => check(RULE_TESTS, Verdict::Ok, "no business rule"),
        Err(e) => check(RULE_TESTS, Verdict::Missing, e.clone()),
    }
}

const RULE_TESTS: &str = "business-rule tests";

/// The `business-rule tests` line for the registry's `files`, each cited
/// file read through `read` (`None` when absent).
fn rule_tests_verdict(
    files: &[(String, String)],
    rev: &str,
    read: impl FnMut(&str) -> Result<Option<String>, String>,
) -> Check {
    use crate::method::rule_tests::{Judge, Tally};
    const NAME: &str = RULE_TESTS;
    let mut judge = Judge::new(read);
    let mut tally = Tally::default();
    let (mut rules, mut enforced) = (0, 0);
    for (path, text) in files {
        let Ok(rule) = business_rules::parse_rule_file(path, text) else {
            continue;
        };
        rules += 1;
        if rule.status != business_rules::RuleStatus::Enforced {
            continue;
        }
        enforced += 1;
        let section = business_rules::test_section(text);
        if let Err(e) = judge.rule(&rule.id, &section, rev, &mut tally) {
            return check(NAME, Verdict::Missing, e);
        }
    }
    if rules == 0 {
        return check(NAME, Verdict::Ok, "no business rule");
    }
    if !tally.problems.is_empty() {
        return check(NAME, Verdict::Invalid, tally.problems.join("; "));
    }
    check(
        NAME,
        Verdict::Ok,
        format!(
            "{enforced} enforced rule(s): {} name(s) found in {} file(s); {} not located",
            tally.found,
            tally.files.len(),
            tally.not_located
        ),
    )
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
    out.push(match read(crate::config::DEFAULT_PATH) {
        Ok(text) => match Config::parse_with(
            &Source {
                label: format!("{} at {rev}", crate::config::DEFAULT_PATH),
                text,
            },
            instance,
        ) {
            Ok(_) => check(
                crate::config::DEFAULT_PATH,
                Verdict::Ok,
                format!("valid at {rev}"),
            ),
            Err(e) => check(
                crate::config::DEFAULT_PATH,
                Verdict::Invalid,
                e.0.iter()
                    .map(|x| x.to_string())
                    .collect::<Vec<_>>()
                    .join("; "),
            ),
        },
        Err(e) if e.is_not_found() => check(
            crate::config::DEFAULT_PATH,
            Verdict::Missing,
            format!("not at {rev}"),
        ),
        Err(e) => check(crate::config::DEFAULT_PATH, Verdict::Missing, e.to_string()),
    });
    // The PR pass's business-rule guard reads it.
    let registry = read_registry_files(github, repo, rev);
    out.push(registry_check(&registry, rev));
    out.push(rule_tests_check(&registry, github, repo, rev));
    let release = &config.release;
    out.push(check(
        "release cycle",
        Verdict::Ok,
        if release.angles.is_empty() {
            "no QA angle: the GO rests on the release CI".to_string()
        } else {
            format!("{} QA angle(s)", release.angles.len())
        },
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
    // The files `release prepare` writes the version into, besides Rust's
    // own: one it cannot find refuses the cut's `open`.
    for f in &release.version_files {
        let name = format!("version file {}", f.path);
        out.push(match read(&f.path) {
            Ok(_) => check(&name, Verdict::Ok, format!("pattern `{}`", f.pattern)),
            Err(e) => check(&name, Verdict::Missing, e.to_string()),
        });
    }
    for wf in [
        &release.cut_workflow,
        &release.gate_workflow,
        &release.gate_rerun_workflow,
        &release.publish_workflow,
    ] {
        let path = format!(".github/workflows/{wf}");
        out.push(match read(&path) {
            Ok(_) => check(&format!("workflow {wf}"), Verdict::Ok, path),
            Err(e) => check(&format!("workflow {wf}"), Verdict::Missing, e.to_string()),
        });
    }
    out.extend(gate_callers(config, github, repo, rev));
    out
}

/// The title check's job in `asdlc-pr-checks.yml` and `asdlc-pr-title.yml`,
/// required by the default and release branch rulesets beside
/// `required-check` (BR-ASDLC-024).
const TITLE_CHECK: &str = "conventional-title";

/// The workflow that runs every method check of a PR in one job
/// (BR-ASDLC-024).
const METHOD_CHECKS: &str = "asdlc-pr-checks.yml";

/// The two workflows that run the same checks in two jobs, before
/// `METHOD_CHECKS`: a project that still calls both runs every check.
const SPLIT_METHOD_CHECKS: [&str; 2] = ["asdlc-pr-guards.yml", "asdlc-pr-title.yml"];

/// Whether the repository's workflows at `rev` run every method check: a job
/// calls `METHOD_CHECKS`, or jobs call both of `SPLIT_METHOD_CHECKS`;
/// `missing` names what no job calls. What else its CI runs is the
/// project's, never judged.
fn method_checks(github: &dyn GitHub, repo: &RepoSlug, rev: &str) -> Check {
    const NAME: &str = "method checks";
    const DIR: &str = ".github/workflows";
    let names = match github.dir(repo, DIR, rev) {
        Ok(names) => names,
        Err(e) if e.is_not_found() => Vec::new(),
        Err(e) => return check(NAME, Verdict::Missing, e.to_string()),
    };
    let mut called = std::collections::BTreeSet::new();
    for name in names
        .iter()
        .filter(|n| n.ends_with(".yml") || n.ends_with(".yaml"))
    {
        match github.file(repo, &format!("{DIR}/{name}"), rev) {
            Ok(text) => called.extend(reusable_callers(&text).into_iter().map(|c| c.called)),
            Err(e) => return check(NAME, Verdict::Missing, format!("{name}: {e}")),
        }
    }
    if called.contains(METHOD_CHECKS) {
        return check(NAME, Verdict::Ok, METHOD_CHECKS);
    }
    let absent: Vec<&str> = SPLIT_METHOD_CHECKS
        .into_iter()
        .filter(|w| !called.contains(*w))
        .collect();
    if absent.is_empty() {
        check(NAME, Verdict::Ok, SPLIT_METHOD_CHECKS.join(", "))
    } else {
        check(
            NAME,
            Verdict::Missing,
            format!(
                "no job of {DIR} calls {METHOD_CHECKS}, nor {}: every PR runs the method checks (docs/setup/examples/pr.yml)",
                absent.join(", ")
            ),
        )
    }
}

/// The version of the method the project is pinned on (BR-ASDLC-032),
/// whether its texts can be read with the project's token, and each angle
/// the configuration names against that version (BR-ASDLC-034): the
/// method's, the project's own, or one the method retired.
fn method_version(
    config: &Config,
    github: &dyn GitHub,
    repo: &RepoSlug,
    rev: &str,
    embedded: Option<&str>,
) -> Vec<Check> {
    let mut out = pinned(config, github, repo, rev, embedded);
    if let (Some(version), false) = (embedded, out.iter().any(|c| c.name == EMBEDDED)) {
        out.push(check(
            EMBEDDED,
            Verdict::Missing,
            format!(
                "the workflow callers' ref cannot be read, so it cannot be told whether they name {}, the minor line of the embedded spl-asdlc {version}",
                crate::embed::minor_line(version)
            ),
        ));
    }
    out
}

const EMBEDDED: &str = "embedded version";

/// Whether the ref the callers name is the minor line of the version the
/// host embeds: `vX.Y`, or an exact `vX.Y.Z` of that line.
fn embedded_check(pin: &Pin, version: &str) -> Check {
    let line = crate::embed::minor_line(version);
    let rev = pin.rev.as_str();
    if rev == line {
        check(
            EMBEDDED,
            Verdict::Ok,
            format!("the callers name {line}, the minor line of the embedded spl-asdlc {version}"),
        )
    } else if rev.strip_prefix(line.as_str()).is_some_and(|rest| {
        rest.strip_prefix('.')
            .is_some_and(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
    }) {
        check(
            EMBEDDED,
            Verdict::Ok,
            format!(
                "the callers name {rev}, of {line}, the minor line of the embedded spl-asdlc {version}; {line} would bring the line's fixes"
            ),
        )
    } else {
        check(
            EMBEDDED,
            Verdict::Invalid,
            format!(
                "the callers name {pin}, but the host embeds spl-asdlc {version}: the workflows and texts the project runs are not those of the bridge that serves it"
            ),
        )
    }
}

fn pinned(
    config: &Config,
    github: &dyn GitHub,
    repo: &RepoSlug,
    rev: &str,
    embedded: Option<&str>,
) -> Vec<Check> {
    const NAME: &str = "method version";
    let mut files = Vec::new();
    match github.dir(repo, WORKFLOWS, rev) {
        Ok(names) => {
            for name in names
                .iter()
                .filter(|n| n.ends_with(".yml") || n.ends_with(".yaml"))
            {
                match github.file(repo, &format!("{WORKFLOWS}/{name}"), rev) {
                    Ok(text) => files.push((name.clone(), text)),
                    Err(e) => return vec![check(NAME, Verdict::Missing, e.to_string())],
                }
            }
        }
        Err(e) if e.is_not_found() => {}
        Err(e) => return vec![check(NAME, Verdict::Missing, e.to_string())],
    }
    let pin = match pin::resolve(&files) {
        Ok(p) => p,
        Err(e @ PinError::NoCaller) => return vec![check(NAME, Verdict::Missing, e.to_string())],
        Err(e) => return vec![check(NAME, Verdict::Invalid, e.to_string())],
    };
    let mut out = vec![check(
        NAME,
        Verdict::Ok,
        format!(
            "{pin}, from the workflow callers at {rev}: the agents' review texts are read there"
        ),
    )];
    if let Some(version) = embedded {
        out.push(embedded_check(&pin, version));
    }
    let texts = "method texts";
    match github.file_from(repo, &pin.repo, Job::PrAngle.path(), &pin.rev) {
        Ok(_) => out.push(check(
            texts,
            Verdict::Ok,
            format!("read at {pin} with the token of {repo}"),
        )),
        Err(e) if e.is_not_found() => {
            out.push(check(
                texts,
                Verdict::Missing,
                format!(
                    "{} is not at {pin}: {} has no such ref, or the GitHub App's installation does not include that repository while it is private (docs/setup/github-app.md)",
                    Job::PrAngle.path(),
                    pin.repo
                ),
            ));
            return out;
        }
        Err(e) => {
            out.push(check(texts, Verdict::Missing, e.to_string()));
            return out;
        }
    }
    let mut keys: Vec<(&str, Option<&str>)> = Vec::new();
    let named = config
        .review
        .angles
        .iter()
        .chain(&config.release.angles)
        .map(|a| (a.key.as_str(), a.notes.as_deref()))
        .chain(config.review.diff_size_angle.as_deref().map(|k| (k, None)));
    for (key, notes) in named {
        match keys.iter_mut().find(|(k, _)| *k == key) {
            Some((_, n)) => *n = n.or(notes),
            None => keys.push((key, notes)),
        }
    }
    for (key, notes) in keys {
        let text = github.file_from(repo, &pin.repo, &angle_path(key), &pin.rev);
        out.push(angle_check(key, notes, &pin, text, retired_in(key)));
    }
    out
}

/// A configured angle against the pinned version: its text there (`text`),
/// and the version that retired it, if the method did.
fn angle_check(
    key: &str,
    notes: Option<&str>,
    pin: &Pin,
    text: Read<String>,
    retired: Option<String>,
) -> Check {
    let name = format!("angle {key}");
    match (text, retired) {
        (Ok(_), None) => check(&name, Verdict::Ok, format!("the method's angle at {pin}")),
        (Ok(_), Some(v)) => check(
            &name,
            Verdict::Ok,
            format!(
                "the method's angle at {pin}; the method retired it in spl-asdlc {v}, so an update of the pin to that version reports it"
            ),
        ),
        (Err(e), Some(v)) if e.is_not_found() => check(
            &name,
            Verdict::Invalid,
            format!(
                "the method retired it in spl-asdlc {v} and has no text for it at {pin}: remove it from the configuration, or keep reviewing from that angle as the project's own, under another key with its notes"
            ),
        ),
        (Err(e), None) if e.is_not_found() => check(
            &name,
            Verdict::Ok,
            format!(
                "the project's own angle: the method has no text for it at {pin}; project notes: {}",
                notes.unwrap_or("none")
            ),
        ),
        (Err(e), _) => check(&name, Verdict::Missing, e.to_string()),
    }
}

/// The reusable workflows that take a `gate-workflow` input and, without
/// it, read `release.gate_workflow` from the configuration.
const GATE_READERS: [&str; 2] = ["asdlc-release-publish.yml", "asdlc-release-cut.yml"];

/// Each job of the repository's workflows at `rev` that calls a reusable
/// workflow of `GATE_READERS`: `invalid` when it passes a `gate-workflow`
/// that is not `release.gate_workflow`, since the publication would then
/// look for the runs of another workflow than the gate the bridge reads.
fn gate_callers(config: &Config, github: &dyn GitHub, repo: &RepoSlug, rev: &str) -> Vec<Check> {
    const DIR: &str = ".github/workflows";
    let configured = &config.release.gate_workflow;
    let names = match github.dir(repo, DIR, rev) {
        Ok(names) => names,
        // No workflow at all: the workflow checks above say what is missing.
        Err(e) if e.is_not_found() => return Vec::new(),
        Err(e) => {
            return vec![check(
                "gate-workflow callers",
                Verdict::Missing,
                e.to_string(),
            )];
        }
    };
    let mut out = Vec::new();
    for name in names
        .iter()
        .filter(|n| n.ends_with(".yml") || n.ends_with(".yaml"))
    {
        let path = format!("{DIR}/{name}");
        let text = match github.file(repo, &path, rev) {
            Ok(text) => text,
            Err(e) => {
                out.push(check(
                    &format!("caller {name}"),
                    Verdict::Missing,
                    e.to_string(),
                ));
                continue;
            }
        };
        for caller in reusable_callers(&text) {
            if !GATE_READERS.contains(&caller.called.as_str()) {
                continue;
            }
            let label = format!("caller {name} job {}", caller.job);
            let called = &caller.called;
            out.push(match caller.inputs.get("gate-workflow").map(String::as_str) {
                None | Some("") => check(
                    &label,
                    Verdict::Ok,
                    format!("{called} reads release.gate_workflow ({configured})"),
                ),
                Some(v) if v.contains("${{") => check(
                    &label,
                    Verdict::Ok,
                    format!("{called} gets gate-workflow from an expression ({v}): not judged; release.gate_workflow is {configured}"),
                ),
                Some(v) if v == configured => check(
                    &label,
                    Verdict::Ok,
                    format!("{called} gets gate-workflow {v}, as release.gate_workflow"),
                ),
                Some(v) => check(
                    &label,
                    Verdict::Invalid,
                    format!("{called} gets gate-workflow {v}, but release.gate_workflow is {configured}: drop the input, and it reads the configuration"),
                ),
            });
        }
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
    out.push(project_check(config, tracker));
    // The configured value and whom it resolves to: an owner configured by
    // full name reads as the same person as the tracker's handle.
    out.push(match tracker.user(&config.owner.tracker_user) {
        Ok(u) => check(
            "owner",
            Verdict::Ok,
            format!(
                "\"{}\" is {} in the tracker",
                config.owner.tracker_user, u.name
            ),
        ),
        Err(e) => check("owner", Verdict::Missing, e.to_string()),
    });
    // The hold label of an escalation: absent is fine, the bridge creates it
    // the first time it escalates.
    {
        let release = &config.release;
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
    // The label of a ticket with nothing to deploy: informational, never
    // required. A person (or refinement) puts it on a ticket; the bridge
    // reads it at the merge and never creates it.
    let name = "tracker.no_deploy_label";
    let label = &config.tracker.no_deploy_label;
    let does = "a ticket carrying it goes Done at its merge, never embarked by a release";
    out.push(match tracker.label_exists(label) {
        Ok(true) => check(
            name,
            Verdict::Ok,
            format!("{label}: {does}; it exists in the tracker"),
        ),
        Ok(false) => check(
            name,
            Verdict::Ok,
            format!("{label}: {does}; it does not exist in the tracker, which is fine until a ticket has nothing to deploy"),
        ),
        Err(e) => check(
            name,
            Verdict::Ok,
            format!("{label}: {does}; whether the tracker has it could not be read ({e}), and it is never required"),
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

/// The tracker project every created item lands in (`tracker.project`): a
/// project the tracker does not know is invalid, since every creation would
/// be refused.
fn project_check(config: &Config, tracker: &dyn Tracker) -> Check {
    let name = "tracker.project";
    let Some(reference) = &config.tracker.project else {
        return check(
            name,
            Verdict::Ok,
            "none: the bridge creates its items in the team, in no project",
        );
    };
    match tracker.project(reference) {
        Ok(Some(p)) => check(
            name,
            Verdict::Ok,
            format!(
                "\"{reference}\" is project {p} in the tracker: every item the bridge creates lands in it"
            ),
        ),
        Ok(None) => check(
            name,
            Verdict::Ok,
            format!(
                "\"{reference}\" is ignored: this tracker does not place its items in a project yet, the bridge creates them in the team"
            ),
        ),
        Err(e) if e.is_not_found() => check(
            name,
            Verdict::Invalid,
            format!("{e}: unknown to the tracker, every creation is refused"),
        ),
        Err(e) => check(name, Verdict::Missing, e.to_string()),
    }
}

/// The configured assignees, named by their key: each angle's `assignee`
/// each round's `coordinator_assignee`, and the release fixer.
fn participants(config: &Config) -> Vec<(String, &str)> {
    let mut out = Vec::new();
    let release = Some((
        "release",
        &config.release.angles,
        &config.release.coordinator_assignee,
    ));
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
    if let Some(name) = &config.release.fix_assignee {
        out.push(("release.fix_assignee".to_string(), name.as_str()));
    }
    out
}

/// The gesture that fixes a check that is not `ok`, by the check's name.
fn fix_of(name: &str, config: &Config) -> Fix {
    let tracker = match config.tracker.kind {
        crate::config::TrackerKind::Linear => "docs/setup/tracker-linear.md",
        crate::config::TrackerKind::Livela => "docs/setup/tracker-livela.md",
    };
    const APP: &str = "docs/setup/github-app.md";
    const REPOSITORY: &str = "docs/setup/repository.md";
    let (gesture, page) = match name {
        "repository" => (
            "Set product.repository to <owner>/<repo> and install the GitHub App on that repository.",
            APP,
        ),
        "github-app" => (
            "Give the bridge's host the App's id and private key (SPL_ASDLC_APP_ID, SPL_ASDLC_APP_PRIVATE_KEY), or the token of the fallback identity.",
            APP,
        ),
        "pause" => (
            "Set the repository variable SPL_ASDLC_PAUSED to `true` or unset it, and give the App Variables: read.",
            "docs/setup/manual-gestures.md",
        ),
        crate::config::DEFAULT_PATH => (
            "Commit a valid .spl-asdlc/config.toml on the default branch; `spl-asdlc config check` says what is wrong.",
            REPOSITORY,
        ),
        "business-rules" | RULE_TESTS => (
            "Fix the business-rule registry under .spl-asdlc/business-rules/ and the tests each rule cites.",
            REPOSITORY,
        ),
        "method checks" => (
            "Call asdlc-pr-checks.yml from the project's workflows, as docs/setup/examples/pr.yml does.",
            REPOSITORY,
        ),
        "method version" => (
            "Make every workflow caller of the method name the same <owner>/spl-asdlc@<ref>.",
            REPOSITORY,
        ),
        "method texts" => (
            "Pin a ref the method's repository has, or include that repository in the App's installation while it is private.",
            APP,
        ),
        EMBEDDED => (
            "Move every workflow caller of the method to the minor line the host embeds (@vX.Y), in a PR of the project.",
            REPOSITORY,
        ),
        "tracker" => (
            "Give the bridge the tracker identity tracker.service_identity names.",
            tracker,
        ),
        "statuses" => (
            "Give the tracker every canonical status, named exactly so.",
            tracker,
        ),
        "tracker.project" => (
            "Name in [tracker] project a project the tracker knows, by its id when two share a name.",
            tracker,
        ),
        "owner" => (
            "Set owner.tracker_user to a person the tracker knows.",
            tracker,
        ),
        "release.escalation_label" => (
            "Let the bridge's identity read the tracker's labels.",
            tracker,
        ),
        n if n.ends_with(".assignee")
            || n.ends_with(".coordinator_assignee")
            || n == "release.fix_assignee" =>
        {
            ("Name a participant the tracker knows.", tracker)
        }
        BRANCHES_AND_PRS => (
            "Before installing the rulesets, delete each branch listed (its fixes merged into the default branch first) and retitle each PR listed `<type>(<scope>): <summary> (<TICKET-ID>)`: once the release prefix's ruleset is on, deleting a branch under it needs a bypass.",
            REPOSITORY,
        ),
        n if n.starts_with("angle ") => (
            "Drop the angle the method retired, or keep it as the project's own under another key with its notes.",
            REPOSITORY,
        ),
        n if n.starts_with("ruleset ") => (
            "Create or fix the ruleset as the page describes.",
            REPOSITORY,
        ),
        n if n.starts_with("workflow ") => (
            "Add the workflow caller from docs/setup/examples/ under the name the configuration gives.",
            REPOSITORY,
        ),
        n if n.starts_with("caller ") || n == "gate-workflow callers" => (
            "Drop the caller's gate-workflow input: the workflow reads release.gate_workflow.",
            REPOSITORY,
        ),
        n if n.starts_with("version file ") => (
            "Add the file, or fix its entry in release.version_files.",
            REPOSITORY,
        ),
        _ => (
            "Follow the setup step the check names.",
            "docs/setup/README.md",
        ),
    };
    Fix {
        gesture: gesture.to_string(),
        page: page.to_string(),
    }
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
                ".spl-asdlc/business-rules/BR-ACME-001.md",
                "# BR-ACME-001 — A rule.\n\nStatus: Enforced\n\n## Rule\nIt holds.\n\n## Evidence\nWhy.\n\n## Test\n`a_test`\n",
            );
            file("web/package.json", "{\"version\": \"0.1.0\"}");
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
        c.release.coordinator_assignee = Some("nobody-known".into());
        c.release.fix_assignee = Some("no-such-fixer".into());
        // The owner configured otherwise than by the tracker's handle.
        c.owner.tracker_user = "u-owner".into();
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
            checks.iter().any(
                |c| c.to_string() == "pause: ok — not paused (SPL_ASDLC_PAUSED unset or false)"
            ),
            "{checks:#?}"
        );
        assert!(
            checks
                .iter()
                .any(|c| c.to_string() == "instance overlay: ok — none"),
            "{checks:#?}"
        );
        assert_eq!(by(".spl-asdlc/config.toml"), Verdict::Missing);
        assert_eq!(by("business-rules"), Verdict::Ok);
        assert!(
            checks.iter().all(|c| c.name != "review-paths"),
            "no sensitive-path list is read: {checks:#?}"
        );
        assert_eq!(by("version file web/package.json"), Verdict::Ok);
        assert_eq!(by("version file docs/version.txt"), Verdict::Missing);
        assert_eq!(by("ruleset main"), Verdict::Ok);
        assert_eq!(by("ruleset release/**"), Verdict::Ok);
        assert_eq!(by("ruleset tags"), Verdict::Ok);
        assert_eq!(by("tracker"), Verdict::Ok);
        assert_eq!(by("statuses"), Verdict::Ok);
        assert!(
            checks
                .iter()
                .any(|c| c.to_string() == "owner: ok — \"u-owner\" is owner-handle in the tracker"),
            "the configured value and the identity it resolves to: {checks:#?}"
        );
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
        assert_eq!(by("release.fix_assignee"), Verdict::Missing);
        assert_eq!(by("release.angles.security.assignee"), Verdict::Missing);
        assert!(checks.iter().any(|c| {
            c.to_string()
                .starts_with("workflow release-cut.yml: missing")
        }));
        assert!(
            checks
                .iter()
                .any(|c| c.to_string() == "release cycle: ok — 2 QA angle(s)"),
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

    fn branch(name: &str) -> crate::github::GitRef {
        crate::github::GitRef {
            name: name.into(),
            sha: format!("sha-{name}"),
            object_type: crate::github::ObjectType::Commit,
        }
    }

    fn compare(status: CompareStatus) -> crate::github::Comparison {
        crate::github::Comparison {
            status,
            ahead_by: 0,
            behind_by: 0,
            commits: Vec::new(),
            files: Vec::new(),
            base_committed_at: None,
        }
    }

    #[test]
    fn leftover_release_branches_and_refused_pr_titles_are_flagged_before_the_rulesets() {
        let c = config();
        let g = FakeGitHub::new();
        let repo = RepoSlug::parse("example-org/acme").unwrap();
        let prefix = c.release.branch_prefix.clone();
        let stamped = |s: &str| format!("{prefix}{s}");
        let pr = |n: u64, title: &str, draft: bool| crate::github::PullRequest {
            title: title.into(),
            draft,
            ..crate::github::fake::pull_request(n)
        };
        {
            let mut s = g.state.borrow_mut();
            s.branches.insert(
                repo.clone(),
                vec![
                    branch(&stamped("0.1.18")),
                    branch(&stamped("2026-10-01-12-00-utc")),
                    branch(&stamped("2026-10-20-09-30-utc")),
                    branch("feat/abc-1-thing"),
                ],
            );
            for (b, st) in [
                ("2026-10-01-12-00-utc", CompareStatus::Behind),
                ("2026-10-20-09-30-utc", CompareStatus::Ahead),
            ] {
                s.compares
                    .insert((repo.clone(), "main".into(), stamped(b)), Ok(compare(st)));
            }
            s.open_pulls.insert(
                repo.clone(),
                Ok(vec![
                    pr(7, "feat(web): a thing (ABC-1)", false),
                    pr(8, "Update the readme", false),
                    pr(9, "wip: try it", true),
                    pr(10, "fix(): empty scope", false),
                ]),
            );
        }
        let got = branches_and_prs(&c, &g, &repo, "main");
        assert_eq!(got.verdict, Verdict::Invalid, "{got}");
        let d = &got.detail;
        assert!(
            d.contains(&format!("branch {}: under", stamped("0.1.18"))),
            "{d}"
        );
        assert!(
            d.contains(&format!(
                "branch {}: every commit",
                stamped("2026-10-01-12-00-utc")
            )),
            "{d}"
        );
        // A cycle in flight, and a branch outside the prefix: not flagged.
        assert!(!d.contains("2026-10-20-09-30-utc"), "{d}");
        assert!(!d.contains("feat/abc-1-thing"), "{d}");
        assert!(!d.contains("PR #7"), "{d}");
        for n in [8, 9, 10] {
            assert!(d.contains(&format!("PR #{n} ")), "{d}");
        }
        // Its fix says what to do before the rulesets.
        let fix = fix_of(BRANCHES_AND_PRS, &c);
        assert!(
            fix.gesture.starts_with("Before installing the rulesets"),
            "{fix:?}"
        );
        assert!(g.writes().is_empty(), "doctor never writes");

        // Cleared: ok, with what was read.
        {
            let mut s = g.state.borrow_mut();
            s.branches
                .insert(repo.clone(), vec![branch(&stamped("2026-10-20-09-30-utc"))]);
            s.open_pulls.insert(
                repo.clone(),
                Ok(vec![pr(7, "feat(web): a thing (ABC-1)", false)]),
            );
        }
        let got = branches_and_prs(&c, &g, &repo, "main");
        assert_eq!(
            got.to_string(),
            format!(
                "branches and PRs: ok — 1 branch(es) under {prefix}, 1 open PR(s): none would block the method"
            )
        );

        // A list that cannot be read is never ok.
        g.state.borrow_mut().open_pulls.remove(&repo);
        assert_eq!(
            branches_and_prs(&c, &g, &repo, "main").verdict,
            Verdict::Missing
        );
        g.state.borrow_mut().branches.remove(&repo);
        assert_eq!(
            branches_and_prs(&c, &g, &repo, "main").verdict,
            Verdict::Missing
        );
    }

    #[test]
    fn the_title_check_is_read_as_the_title_workflow_reads_it() {
        let types = ["feat", "fix", "chore"];
        for ok in [
            "feat: a",
            "fix(web): a thing (ABC-1)",
            "feat(api)!: breaking",
            "chore!: drop it",
        ] {
            assert!(title_allowed(ok, &types), "{ok}");
        }
        for refused in [
            "Feat: a",
            "feat:a",
            "feat: ",
            "feat:  a",
            "feat: a ",
            "feat(): a",
            "feat(a b): c",
            "feat(a: b",
            "docs: a",
            "a thing",
        ] {
            assert!(!title_allowed(refused, &types), "{refused}");
        }
    }

    #[test]
    fn a_paused_project_is_reported_and_an_unreadable_switch_pauses_nothing() {
        use crate::github::Variable;
        let g = FakeGitHub::new();
        let repo = RepoSlug::parse("example-org/acme").unwrap();
        let set = |value: Read<Variable>| {
            g.state
                .borrow_mut()
                .variables
                .insert((repo.clone(), "SPL_ASDLC_PAUSED".into()), value);
        };
        let var = |value: &str| {
            Ok(Variable {
                name: "SPL_ASDLC_PAUSED".into(),
                value: value.into(),
                updated_at: Some("2026-10-18T09:00:00Z".into()),
            })
        };
        set(var("true"));
        assert_eq!(
            pause_check(&g, &repo).to_string(),
            "pause: ok — paused: SPL_ASDLC_PAUSED is true (set 2026-10-18T09:00:00Z); the bridge writes nothing for example-org/acme until it is unset or false"
        );
        set(var("false"));
        assert_eq!(pause_check(&g, &repo).verdict, Verdict::Ok);
        set(var("yes"));
        assert_eq!(pause_check(&g, &repo).verdict, Verdict::Invalid);
        set(Err(ReadError::Unreadable {
            what: "variable SPL_ASDLC_PAUSED of example-org/acme".into(),
            status: Some(403),
            detail: "HTTP 403: Resource not accessible by integration".into(),
        }));
        let c = pause_check(&g, &repo);
        assert_eq!(c.verdict, Verdict::Missing, "{c}");
        assert!(c.detail.contains("Variables: read"), "{c}");
    }

    #[test]
    fn the_method_version_its_texts_and_each_angle_are_judged_against_the_pin() {
        let c = config();
        let g = FakeGitHub::new();
        let repo = RepoSlug::parse("example-org/acme").unwrap();
        let lines = |g: &FakeGitHub| -> Vec<String> {
            method_version(&c, g, &repo, "main", None)
                .iter()
                .map(ToString::to_string)
                .collect()
        };
        let none = lines(&g);
        assert_eq!(none.len(), 1, "{none:?}");
        assert!(
            none[0].starts_with("method version: missing — no workflow under .github/workflows calls a reusable workflow of the method"),
            "{none:?}"
        );
        g.pin_method(&repo, "v1");
        // `product` has no text at that version: the project's own angle.
        g.state.borrow_mut().files.remove(&(
            RepoSlug::parse("example-org/spl-asdlc").unwrap(),
            "v1".into(),
            "docs/agents/angles/product.md".into(),
        ));
        assert_eq!(
            lines(&g),
            [
                "method version: ok — example-org/spl-asdlc@v1, from the workflow callers at main: the agents' review texts are read there",
                "method texts: ok — read at example-org/spl-asdlc@v1 with the token of example-org/acme",
                "angle maintainability: ok — the method's angle at example-org/spl-asdlc@v1",
                "angle security: ok — the method's angle at example-org/spl-asdlc@v1",
                "angle product: ok — the project's own angle: the method has no text for it at example-org/spl-asdlc@v1; project notes: none",
            ]
        );
        // A ref the method's repository lacks, or a private repository the
        // App's installation does not include: GitHub says 404 for both.
        g.pin_caller(&repo, "example-org/spl-asdlc", "v9.9.9");
        let unread = lines(&g);
        assert_eq!(unread.len(), 2, "{unread:?}");
        assert!(
            unread[1].starts_with("method texts: missing — docs/agents/review-pr.md is not at example-org/spl-asdlc@v9.9.9")
                && unread[1].contains("docs/setup/github-app.md"),
            "{unread:?}"
        );
        // Callers on two versions tell none.
        g.state.borrow_mut().files.insert(
            (repo.clone(), "main".into(), ".github/workflows/cut.yml".into()),
            Ok("jobs:\n  cut:\n    uses: example-org/spl-asdlc/.github/workflows/asdlc-release-cut.yml@v1.2.0\n".into()),
        );
        let several = lines(&g);
        assert!(
            several[0].starts_with("method version: invalid — the workflow callers pin the method on more than one version"),
            "{several:?}"
        );
    }

    /// A host that embeds the crate: the callers' ref is judged against the
    /// minor line of the version it embeds.
    #[test]
    fn the_callers_ref_is_judged_against_the_embedded_minor_line() {
        let c = config();
        let g = FakeGitHub::new();
        let repo = RepoSlug::parse("example-org/acme").unwrap();
        let embedded = |g: &FakeGitHub| -> Check {
            method_version(&c, g, &repo, "main", Some("1.4.2"))
                .into_iter()
                .find(|c| c.name == EMBEDDED)
                .expect("an embedded version check")
        };
        assert_eq!(embedded(&g).verdict, Verdict::Missing, "no caller");
        for (rev, verdict) in [
            ("v1.4", Verdict::Ok),
            ("v1.4.0", Verdict::Ok),
            ("v1.4.9", Verdict::Ok),
            ("v1", Verdict::Invalid),
            ("v1.3", Verdict::Invalid),
            ("v1.40", Verdict::Invalid),
            ("v1.4.x", Verdict::Invalid),
            ("main", Verdict::Invalid),
        ] {
            g.pin_method(&repo, rev);
            let check = embedded(&g);
            assert_eq!(check.verdict, verdict, "{rev}: {check}");
        }
        g.pin_method(&repo, "v1");
        assert!(
            embedded(&g)
                .detail
                .contains("the host embeds spl-asdlc 1.4.2"),
            "{}",
            embedded(&g)
        );
        // The binary at the command line embeds nothing: no such check.
        assert!(
            method_version(&c, &g, &repo, "main", None)
                .iter()
                .all(|c| c.name != EMBEDDED)
        );
    }

    /// Every check not ok says how it is fixed and where; the result
    /// serialises with each check's state.
    #[test]
    fn a_check_not_ok_carries_its_fix_and_the_report_serialises() {
        let c = config();
        let g = FakeGitHub::new();
        let tracker = FakeTracker::default();
        let checks = run(&c, None, &g, &tracker, "test");
        let not_ok: Vec<_> = checks.iter().filter(|c| c.verdict != Verdict::Ok).collect();
        assert!(!not_ok.is_empty());
        for check in &checks {
            match &check.fix {
                Some(f) => {
                    assert!(check.verdict != Verdict::Ok, "{check}");
                    assert!(f.page.starts_with("docs/setup/"), "{f:?}");
                    let page = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(&f.page);
                    assert!(page.exists(), "{}", f.page);
                    assert!(f.gesture.ends_with('.'), "{f:?}");
                }
                None => assert_eq!(check.verdict, Verdict::Ok, "{check}"),
            }
        }
        let json = serde_json::to_value(&checks).unwrap();
        let first_missing = json
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["state"] != "ok")
            .unwrap();
        assert!(first_missing["fix"]["gesture"].is_string());
        assert_eq!(json[0]["state"], "ok");
        assert!(json[0]["fix"].is_null());
        // Each name of the fix table is a check doctor emits or a family.
        let tracker_page = fix_of("statuses", &c).page;
        assert_eq!(tracker_page, "docs/setup/tracker-linear.md");
        assert_eq!(fix_of("ruleset main", &c).page, "docs/setup/repository.md");
        assert_eq!(
            fix_of("release.angles.security.assignee", &c).page,
            tracker_page
        );
    }

    #[test]
    fn a_retired_angle_is_reported_once_the_pin_reaches_its_retirement() {
        let pin = Pin {
            repo: RepoSlug::parse("example-org/spl-asdlc").unwrap(),
            rev: "v1.5.0".into(),
        };
        let absent = || {
            Err(crate::github::ReadError::NotFound {
                what: "docs/agents/angles/legacy.md".into(),
            })
        };
        let retired = angle_check("legacy", None, &pin, absent(), Some("1.4.0".into()));
        assert_eq!(retired.verdict, Verdict::Invalid, "{retired}");
        assert!(
            retired
                .detail
                .starts_with("the method retired it in spl-asdlc 1.4.0 and has no text for it at example-org/spl-asdlc@v1.5.0"),
            "{retired}"
        );
        // Pinned before the retirement: still the method's, and said.
        let older = Pin {
            rev: "v1.3.0".into(),
            ..pin.clone()
        };
        let kept = angle_check(
            "legacy",
            None,
            &older,
            Ok("# Angle: legacy".into()),
            Some("1.4.0".into()),
        );
        assert_eq!(kept.verdict, Verdict::Ok, "{kept}");
        assert!(
            kept.detail
                .contains("an update of the pin to that version reports it"),
            "{kept}"
        );
        // Never retired: the project's own.
        let own = angle_check(
            "accessibility",
            Some(".spl-asdlc/angles/a11y.md"),
            &pin,
            absent(),
            None,
        );
        assert_eq!(own.verdict, Verdict::Ok, "{own}");
        assert!(
            own.detail
                .ends_with("project notes: .spl-asdlc/angles/a11y.md"),
            "{own}"
        );
    }

    #[test]
    fn a_caller_whose_gate_workflow_differs_from_the_configuration_is_invalid() {
        let c = config();
        assert_eq!(c.release.gate_workflow, "release-gate.yml");
        let g = FakeGitHub::new();
        let repo = RepoSlug::parse("example-org/acme").unwrap();
        let caller = |job: &str, called: &str, with: &str| {
            format!(
                "jobs:\n  {job}:\n    uses: acme/spl-asdlc/.github/workflows/{called}@v1\n    with:\n      stage: verify\n{with}"
            )
        };
        {
            let mut s = g.state.borrow_mut();
            let mut file = |p: &str, text: String| {
                s.files
                    .insert((repo.clone(), "main".into(), p.into()), Ok(text));
            };
            file(
                ".github/workflows/release-publish.yml",
                caller(
                    "verify",
                    "asdlc-release-publish.yml",
                    "      gate-workflow: pre-release-qa.yml\n",
                ),
            );
            file(
                ".github/workflows/release-cut.yml",
                caller(
                    "cut",
                    "asdlc-release-cut.yml",
                    "      gate-workflow: release-gate.yml\n",
                ),
            );
            file(
                ".github/workflows/deploy.yaml",
                caller("verify", "asdlc-release-publish.yml", ""),
            );
            file(
                ".github/workflows/dynamic.yml",
                caller(
                    "verify",
                    "asdlc-release-publish.yml",
                    "      gate-workflow: ${{ vars.GATE }}\n",
                ),
            );
            // Another reusable workflow's input of the same name is not judged.
            file(
                ".github/workflows/release-gate.yml",
                caller(
                    "gate",
                    "asdlc-release-gate.yml",
                    "      gate-workflow: other.yml\n",
                ),
            );
            file(".github/workflows/notes.txt", "jobs:\n".into());
        }
        let checks = gate_callers(&c, &g, &repo, "main");
        let lines: Vec<String> = checks.iter().map(ToString::to_string).collect();
        assert_eq!(
            lines,
            [
                "caller deploy.yaml job verify: ok — asdlc-release-publish.yml reads release.gate_workflow (release-gate.yml)",
                "caller dynamic.yml job verify: ok — asdlc-release-publish.yml gets gate-workflow from an expression (${{ vars.GATE }}): not judged; release.gate_workflow is release-gate.yml",
                "caller release-cut.yml job cut: ok — asdlc-release-cut.yml gets gate-workflow release-gate.yml, as release.gate_workflow",
                "caller release-publish.yml job verify: invalid — asdlc-release-publish.yml gets gate-workflow pre-release-qa.yml, but release.gate_workflow is release-gate.yml: drop the input, and it reads the configuration",
            ],
            "{lines:#?}"
        );
        // The check runs within doctor, at the default branch.
        g.state
            .borrow_mut()
            .default_branch
            .insert(repo.clone(), "main".into());
        let all = run(&c, None, &g, &FakeTracker::default(), "workstation");
        assert!(
            all.iter()
                .any(|x| x.name == "caller release-publish.yml job verify"
                    && x.verdict == Verdict::Invalid),
            "{all:#?}"
        );
        // No workflow directory: nothing to judge, the workflow lines say so.
        let empty = FakeGitHub::new();
        assert!(gate_callers(&c, &empty, &repo, "main").is_empty());
        // An unreadable directory is never ok.
        empty.state.borrow_mut().dirs.insert(
            (repo.clone(), "main".into(), ".github/workflows".into()),
            Err(ReadError::unreadable("the directory", "boom")),
        );
        let unreadable = gate_callers(&c, &empty, &repo, "main");
        assert_eq!(unreadable.len(), 1);
        assert_eq!(unreadable[0].verdict, Verdict::Missing, "{unreadable:?}");
    }

    #[test]
    fn a_project_that_does_not_call_every_method_check_is_reported() {
        let g = FakeGitHub::new();
        let repo = RepoSlug::parse("example-org/acme").unwrap();
        let caller = |called: &str| {
            format!("jobs:\n  check:\n    uses: acme/spl-asdlc/.github/workflows/{called}@v1\n")
        };
        let file = |p: &str, text: String| {
            g.state
                .borrow_mut()
                .files
                .insert((repo.clone(), "main".into(), p.into()), Ok(text));
        };
        // No workflow at all.
        let none = method_checks(&g, &repo, "main");
        assert_eq!(none.verdict, Verdict::Missing, "{none}");
        file(".github/workflows/pr.yml", caller("asdlc-pr-guards.yml"));
        file(
            ".github/workflows/heavy.yml",
            "jobs:\n  test:\n    runs-on: ubuntu-latest\n".into(),
        );
        let half = method_checks(&g, &repo, "main");
        assert_eq!(
            half.to_string(),
            "method checks: missing — no job of .github/workflows calls asdlc-pr-checks.yml, nor asdlc-pr-title.yml: every PR runs the method checks (docs/setup/examples/pr.yml)"
        );
        // The two workflows of before asdlc-pr-checks.yml, both called.
        file(
            ".github/workflows/pr-title.yml",
            caller("asdlc-pr-title.yml"),
        );
        let all = method_checks(&g, &repo, "main");
        assert_eq!(
            all.to_string(),
            "method checks: ok — asdlc-pr-guards.yml, asdlc-pr-title.yml"
        );
    }

    #[test]
    fn the_one_workflow_of_every_method_check_is_enough() {
        let g = FakeGitHub::new();
        let repo = RepoSlug::parse("example-org/acme").unwrap();
        g.state.borrow_mut().files.insert(
            (repo.clone(), "main".into(), ".github/workflows/pr.yml".into()),
            Ok("jobs:\n  guards:\n    name: title\n    uses: acme/spl-asdlc/.github/workflows/asdlc-pr-checks.yml@v1\n".into()),
        );
        let one = method_checks(&g, &repo, "main");
        assert_eq!(one.to_string(), "method checks: ok — asdlc-pr-checks.yml");
    }

    #[test]
    fn the_registry_check_reads_every_rule_file_and_names_each_invalid_one() {
        let g = FakeGitHub::new();
        let repo = RepoSlug::parse("example-org/acme").unwrap();
        let checked = || registry_check(&read_registry_files(&g, &repo, "main"), "main");
        // No directory at a revision: no rule yet, which is fine.
        let none = checked();
        assert_eq!(none.verdict, Verdict::Ok, "{none}");
        assert!(none.detail.contains("no rule yet"), "{none}");
        let put = |name: &str, text: &str| {
            g.state.borrow_mut().files.insert(
                (
                    repo.clone(),
                    "main".into(),
                    format!(".spl-asdlc/business-rules/{name}"),
                ),
                Ok(text.into()),
            );
        };
        let rule = |id: &str| {
            format!(
                "# {id} — A rule.\n\nStatus: Proposed\n\n## Rule\nIt holds.\n\n## Evidence\nWhy.\n\n## Test\nNone yet.\n"
            )
        };
        put("BR-ACME-001.md", &rule("BR-ACME-001"));
        put("README.md", "# Index\n\nNot a rule.\n");
        let ok = checked();
        assert_eq!(ok.verdict, Verdict::Ok, "{ok}");
        assert!(ok.detail.contains("1 rule(s)"), "{ok}");
        put("BR-ACME-002.md", &rule("BR-ACME-003"));
        put("notes.md", "Anything.");
        let bad = checked();
        assert_eq!(bad.verdict, Verdict::Invalid, "{bad}");
        assert!(
            bad.detail.contains(
                ".spl-asdlc/business-rules/BR-ACME-002.md: the title names `BR-ACME-003`"
            ),
            "{bad}"
        );
        assert!(
            bad.detail
                .contains(".spl-asdlc/business-rules/notes.md: the file name"),
            "{bad}"
        );
        // A listing that fails is not a registry without rules.
        g.state.borrow_mut().dirs.insert(
            (
                repo.clone(),
                "main".into(),
                ".spl-asdlc/business-rules".into(),
            ),
            Err(ReadError::unreadable("the listing", "HTTP 502")),
        );
        assert_eq!(checked().verdict, Verdict::Missing);
    }

    /// A rule file at `main`, its status and Test section given.
    fn rule_file(id: &str, status: &str, test: &str) -> (String, String) {
        (
            format!(".spl-asdlc/business-rules/{id}.md"),
            format!(
                "# {id} — A rule.\n\nStatus: {status}\n\n## Rule\nIt holds.\n\n## Evidence\nWhy.\n\n## Test\n\n{test}\n"
            ),
        )
    }

    fn rule_tests_of(g: &FakeGitHub, repo: &RepoSlug) -> Check {
        rule_tests_check(&read_registry_files(g, repo, "main"), g, repo, "main")
    }

    #[test]
    fn the_tests_a_rule_cites_are_found_in_the_files_it_names() {
        let g = FakeGitHub::new();
        let repo = RepoSlug::parse("example-org/acme").unwrap();
        let none = rule_tests_of(&g, &repo);
        assert_eq!(
            none.to_string(),
            "business-rule tests: ok — no business rule"
        );
        let put = |(path, text): (String, String)| {
            g.state
                .borrow_mut()
                .files
                .insert((repo.clone(), "main".into(), path), Ok(text));
        };
        let file = |path: &str, text: &str| put((path.into(), text.into()));
        // A JS project: the file, then the nested names in a list.
        put(rule_file(
            "BR-ACME-001",
            "Enforced",
            "`apps/webapp/src/app/api/process/__tests__/pdfImportWorker.test.ts`, in the\n\
             `webapp#test` task of the `build-test` job:\n\n\
             - `runPdfImportJob > duplicate delivery while a first attempt is still running > refuses the second delivery without calling the provider`",
        ));
        file(
            "apps/webapp/src/app/api/process/__tests__/pdfImportWorker.test.ts",
            "describe('runPdfImportJob', () => {\n  it('refuses the second delivery without calling the provider', async () => {});\n});\n",
        );
        // Rust: a module path, a wildcard, a file then its names, in prose.
        put(rule_file(
            "BR-ACME-002",
            "Enforced",
            "The flow holds it: `tick::release_tests::a_case`, `method::ticket_state::tests::*`;\n\
             `tests/workflows/own.rs`: `name_a`, `name_b`, run by the `ci` job.",
        ));
        file("src/tick/release_tests.rs", "#[test]\nfn a_case() {}\n");
        file("src/method/ticket_state.rs", "mod tests {}\n");
        file("tests/workflows/own.rs", "fn name_a() {}\nfn name_b() {}\n");
        // Enforced by review, its names nowhere to be located: counted.
        put(rule_file(
            "BR-ACME-003",
            "Enforced",
            "Enforced by review, not by code. The bridge's side is `tick::tests::a_case_elsewhere`, and `a_bare_name`.",
        ));
        // A Proposed rule is not judged.
        put(rule_file("BR-ACME-004", "Proposed", "`tests/not_yet.rs`"));
        let ok = rule_tests_of(&g, &repo);
        assert_eq!(
            ok.to_string(),
            "business-rule tests: ok — 3 enforced rule(s): 5 name(s) found in 4 file(s); 2 not located",
        );
    }

    #[test]
    fn a_cited_test_that_is_absent_is_invalid_and_an_unreadable_file_is_missing() {
        let g = FakeGitHub::new();
        let repo = RepoSlug::parse("example-org/acme").unwrap();
        let put = |(path, text): (String, String)| {
            g.state
                .borrow_mut()
                .files
                .insert((repo.clone(), "main".into(), path), Ok(text));
        };
        put(rule_file(
            "BR-ACME-001",
            "Enforced",
            "`tests/workflows/own.rs`: `name_a`, `name_c`; `tick::release_tests::gone_case`",
        ));
        put(rule_file(
            "BR-ACME-002",
            "Enforced",
            "`gone_case_too` (`tests/gone.rs`)",
        ));
        put(("tests/workflows/own.rs".into(), "fn name_a() {}\n".into()));
        put((
            "src/tick/release_tests.rs".into(),
            "fn a_case() {}\n".into(),
        ));
        let bad = rule_tests_of(&g, &repo);
        assert_eq!(bad.verdict, Verdict::Invalid, "{bad}");
        assert_eq!(
            bad.detail,
            "BR-ACME-001: `name_c` is not in `tests/workflows/own.rs`; \
             BR-ACME-001: `tick::release_tests::gone_case` is not in `src/tick/release_tests.rs`; \
             BR-ACME-002: `tests/gone.rs` is absent at main"
        );
        // A file that cannot be read concludes nothing.
        g.state.borrow_mut().files.insert(
            (repo.clone(), "main".into(), "tests/gone.rs".into()),
            Err(ReadError::unreadable("tests/gone.rs", "HTTP 502")),
        );
        let unread = rule_tests_of(&g, &repo);
        assert_eq!(unread.verdict, Verdict::Missing, "{unread}");
        assert!(unread.detail.starts_with("tests/gone.rs: "), "{unread}");
    }

    /// This repository runs no bridge (BR-ASDLC-023): its own rules are
    /// held to the check here, read from the working tree.
    #[test]
    fn this_repositorys_rules_cite_tests_that_exist() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let dir = root.join(business_rules::REGISTRY_DIR);
        let mut files: Vec<(String, String)> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .map(|p| {
                let name = p.file_name().unwrap().to_string_lossy().to_string();
                (
                    format!("{}/{name}", business_rules::REGISTRY_DIR),
                    std::fs::read_to_string(&p).unwrap(),
                )
            })
            .collect();
        files.sort();
        let line = rule_tests_verdict(&files, "the working tree", |path: &str| {
            match std::fs::read_to_string(root.join(path)) {
                Ok(text) => Ok(Some(text)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(format!("{path}: {e}")),
            }
        });
        assert_eq!(line.verdict, Verdict::Ok, "{line}");
        assert!(line.detail.ends_with("; 0 not located"), "{line}");
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
    fn the_no_deploy_label_is_reported_and_never_required() {
        let c = config();
        let t = FakeTracker::default();
        let line = |t: &FakeTracker| {
            tracker_checks(&c, t)
                .into_iter()
                .find(|c| c.name == "tracker.no_deploy_label")
                .unwrap_or_else(|| panic!("no no-deploy line"))
        };
        let absent = line(&t);
        assert_eq!(absent.verdict, Verdict::Ok, "{absent}");
        assert!(
            absent.detail.starts_with("no-deploy: ")
                && absent.detail.contains("does not exist in the tracker"),
            "{absent}"
        );
        t.ensure_label("no-deploy").unwrap();
        let present = line(&t);
        assert_eq!(present.verdict, Verdict::Ok, "{present}");
        assert!(
            present.detail.ends_with("it exists in the tracker"),
            "{present}"
        );
    }

    #[test]
    fn the_tracker_project_is_reported_found_unknown_ignored_or_none() {
        let mut c = config();
        let mut t = FakeTracker {
            projects: Some(vec!["Acme".into()]),
            ..FakeTracker::default()
        };
        let line = |c: &Config, t: &FakeTracker| {
            tracker_checks(c, t)
                .into_iter()
                .find(|c| c.name == "tracker.project")
                .unwrap_or_else(|| panic!("no project line"))
        };
        let none = line(&c, &t);
        assert_eq!(none.verdict, Verdict::Ok, "{none}");
        assert!(none.detail.starts_with("none: "), "{none}");

        c.tracker.project = Some("acme".into());
        let found = line(&c, &t);
        assert_eq!(found.verdict, Verdict::Ok, "{found}");
        assert!(
            found
                .detail
                .starts_with("\"acme\" is project Acme in the tracker"),
            "{found}"
        );

        c.tracker.project = Some("elsewhere".into());
        let unknown = line(&c, &t);
        assert_eq!(unknown.verdict, Verdict::Invalid, "{unknown}");
        assert!(
            unknown.detail.contains("unknown to the tracker"),
            "{unknown}"
        );

        t.projects = None;
        let ignored = line(&c, &t);
        assert_eq!(ignored.verdict, Verdict::Ok, "{ignored}");
        assert!(ignored.detail.contains("is ignored"), "{ignored}");
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
            line.starts_with("PR review: ok — on every PR: maintainability; security on its paths"),
            "{line}"
        );
        c.review.angles.retain(|a| !a.paths.is_empty());
        let line = pr_review(&c).to_string();
        assert!(line.contains("no angle on every PR"), "{line}");
        assert!(line.contains("security on its paths"), "{line}");
        c.review.angles.clear();
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
                (repo.clone(), "main".into(), ".spl-asdlc/config.toml".into()),
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
            (line("instance overlay"), line(".spl-asdlc/config.toml"))
        };
        let (instance, file) = verdict(Some(&overlay));
        assert_eq!(
            instance.to_string(),
            "instance overlay: ok — config.instance.toml"
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
                [
                    "required-check".to_string(),
                    "title / conventional-title".to_string(),
                ]
                .into()
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
            "ruleset main: ok — the status check `required-check` and the status check `… / conventional-title`, enforced by `main`"
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
    fn a_branch_ruleset_that_does_not_require_the_title_check_is_missing() {
        let mut r = procedure_rulesets();
        for rs in r.iter_mut() {
            rs.required_checks.retain(|c| c == "required-check");
        }
        let lines = ruleset_lines(Ok(r));
        let main = lines.iter().find(|c| c.name == "ruleset main").unwrap();
        assert_eq!(main.verdict, Verdict::Missing, "{main}");
        assert!(
            main.detail
                .contains("lacks the status check `… / conventional-title`"),
            "{main}"
        );
        // The bare job name counts too, as a check named by its job alone.
        let mut r = procedure_rulesets();
        for rs in r.iter_mut().filter(|rs| rs.target == "branch") {
            rs.required_checks = [
                "required-check".to_string(),
                "conventional-title".to_string(),
            ]
            .into();
        }
        let lines = ruleset_lines(Ok(r));
        assert!(
            lines
                .iter()
                .filter(|c| c.name.starts_with("ruleset ") && c.name != "ruleset tags")
                .all(|c| c.verdict == Verdict::Ok),
            "{lines:#?}"
        );
    }

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
