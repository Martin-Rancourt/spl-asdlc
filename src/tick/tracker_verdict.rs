//! A coordinator's verdict given in the tracker (rule 2W8R,
//! `docs/reference/06-qa-report.md`, "The verdict given in the tracker"):
//!
//! 1. once every angle PR of the candidate is merged and its coordination
//!    item is still open, the bridge posts on that item, once per candidate,
//!    the findings to judge: a comment opening with the marker
//!    `spl-asdlc:verdict-request candidate=<tag>` and carrying a fenced
//!    `json` block (`spl-asdlc/verdict-request/1`), every finding of every
//!    merged report as the reports give them;
//! 2. it reads, on the same item, the latest comment opening with the marker
//!    `spl-asdlc:verdict candidate=<tag>` and carrying a fenced `json` block
//!    with a verdict (`spl-asdlc/qa-verdict/1`), taken only from a person
//!    (the tracker says its author is human) who is the item's assignee or
//!    the release's owner; any other author is told once, on the item, why
//!    it is ignored;
//! 3. a valid verdict is committed unchanged by the bridge: the branch
//!    `qa/<candidate>/consolidation` from the release branch's tip, the
//!    verdict file on it, a PR to the release branch naming the coordination
//!    item. The release pass merges that PR by the rules of any verdict PR,
//!    its author being the bridge on the strength of the signed comment
//!    ([`Outcome::Signed`]). An invalid verdict is told once, with its
//!    errors, and nothing is committed.
//!
//! Idempotent: each marker carries the candidate; the release pass never
//! calls this once the candidate's verdict is merged; a newer verdict before
//! the merge replaces the file on the same branch, one more commit.

use serde_json::json;

use crate::action_log::ActionLog;
use crate::github::PullRequest;
use crate::method::release::go::AngleState;
use crate::method::release::prs;
use crate::method::release::report::Finding;
use crate::method::release::verdict;
use crate::method::texts;
use crate::tracker::{Comment, Item, User};

use super::merge::for_tracker;
use super::release::{Candidate, MergedReport};
use super::{Report, Tick};

/// The schema of the bridge's request for a verdict.
pub const REQUEST_SCHEMA: &str = "spl-asdlc/verdict-request/1";

/// What the tracker's verdict means for the candidate's verdict PR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// No verdict given in the tracker stands: the verdict PR is judged as
    /// any verdict PR.
    None,
    /// The bridge wrote on GitHub this tick: its verdict PR waits for the
    /// next tick, when GitHub shows the new head.
    Wrote,
    /// The latest signed verdict, valid, as the bridge commits it: the
    /// bridge's own verdict PR counts as the coordinator's while its file is
    /// this text.
    Signed(String),
}

/// The marker of the bridge's request for the candidate's verdict.
pub fn request_marker(candidate: &str) -> String {
    texts::marker("verdict-request", &[("candidate", candidate)])
}

/// The marker that opens a verdict given in the tracker.
pub fn verdict_marker(candidate: &str) -> String {
    texts::marker("verdict", &[("candidate", candidate)])
}

/// Whether `body` opens with `marker`, as an HTML comment or, on a tracker
/// that drops those, as inline code.
pub fn opens_with(body: &str, marker: &str) -> bool {
    let body = body.trim_start();
    let token = texts::token(marker);
    let rest = if let Some(r) = body.strip_prefix(marker.trim()) {
        r
    } else if let Some(r) = body
        .strip_prefix('`')
        .and_then(|r| r.strip_prefix(token))
        .and_then(|r| r.strip_prefix('`'))
    {
        r
    } else {
        return false;
    };
    rest.is_empty() || rest.starts_with(char::is_whitespace)
}

/// The first fenced `json` block of a comment, its text trimmed.
pub fn fenced_json(body: &str) -> Option<String> {
    let mut lines = body.lines();
    lines.by_ref().find(|l| {
        let l = l.trim();
        l.strip_prefix("```")
            .is_some_and(|lang| lang.trim().eq_ignore_ascii_case("json"))
    })?;
    let mut out = Vec::new();
    for l in lines {
        if l.trim() == "```" {
            let text = out.join("\n").trim().to_string();
            return (!text.is_empty()).then_some(text);
        }
        out.push(l);
    }
    None
}

/// JSON text that can sit in a fenced block of a comment of the bridge's:
/// the same value, with no character that would end the fence or forge a
/// marker (`<`, `` ` `` and `spl-asdlc:` escaped, inside strings only,
/// where JSON's syntax puts them).
fn inert_json(value: &serde_json::Value) -> String {
    serde_json::to_string_pretty(value)
        .unwrap_or_default()
        .replace('<', "\\u003c")
        .replace('`', "\\u0060")
        .replace("spl-asdlc:", "spl-asdlc\\u003a")
}

/// The request's JSON: every finding of every merged report, as the
/// reports give them.
pub fn request_json(candidate: &str, item: &str, findings: &[(String, Finding)]) -> String {
    let findings: Vec<serde_json::Value> = findings
        .iter()
        .map(|(angle, f)| {
            json!({
                "angle": angle,
                "id": f.id,
                "blocking": f.blocking,
                "summary": f.summary,
                "anchor": f.anchor,
                "evidence": f.evidence,
                "testable": f.testable,
                "proposed_test": f.proposed_test,
            })
        })
        .collect();
    inert_json(&json!({
        "schema": REQUEST_SCHEMA,
        "candidate": candidate,
        "item": item,
        "findings": findings,
    }))
}

/// The bridge's request for the candidate's verdict, on its coordination
/// item.
pub fn request_comment(candidate: &str, item: &str, findings: &[(String, Finding)]) -> String {
    format!(
        "{}\nEvery angle report of `{candidate}` is merged: the findings below wait for the verdict (`docs/reference/06-qa-report.md`, \"The verdict\"). \
         Judge each one once, accepted (blocking or not) or rejected with its reason, and give `go` or `hold`. \
         The assignee of this item or the release's owner may give it here, in a comment that opens with the line \
         `{}` and carries the verdict (`{}`) in a fenced `json` block: the bridge commits it unchanged in a PR from \
         `qa/{candidate}/consolidation` and merges it as any verdict PR. The coordinator may also commit it in that PR themselves.\n\n```json\n{}\n```",
        request_marker(candidate),
        verdict_marker(candidate),
        verdict::SCHEMA,
        request_json(candidate, item, findings)
    )
}

/// Why the author of a verdict comment cannot give the verdict; `None`
/// when they can: a person (as the tracker says) who is the coordination
/// item's assignee or one of the release's owners.
pub fn refusal(comment: &Comment, owners: &[User]) -> Option<String> {
    if !comment.by_person {
        return Some(
            "the tracker does not say its author is a person: a verdict given in the tracker counts only when a person signs it; an agent commits its verdict in its own PR from the verdict branch".into(),
        );
    }
    let Some(author) = comment.author.as_ref() else {
        return Some("the tracker names no author for it".into());
    };
    if owners.iter().any(|o| o.id == author.id) {
        return None;
    }
    Some(format!(
        "{} is neither this item's assignee nor the release's owner, who alone give the verdict here",
        author.name
    ))
}

/// The verdict branch of the candidate.
fn verdict_branch(c: &Candidate) -> String {
    format!("qa/{}/{}", c.tag, verdict::KEY)
}

fn is_verdict_pr(pr: &PullRequest, c: &Candidate) -> bool {
    prs::parse_angle_branch(&pr.head_ref) == Some((c.tag.clone(), verdict::KEY.to_string()))
}

/// Whether the bridge's own verdict PR at `text` (its verdict file at its
/// head) carries the signed verdict.
pub fn vouches(t: &Tick, pr: &PullRequest, text: &str, signed: &Outcome) -> bool {
    match signed {
        Outcome::Signed(s) => by_bridge(t, pr) && text.trim() == s.trim(),
        _ => false,
    }
}

/// The request, the verdict comments, and the commit of the latest valid
/// signed verdict, for a candidate whose verdict is not merged yet.
#[allow(clippy::too_many_arguments)]
pub fn pass(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    release_item: &Item,
    c: &Candidate,
    branch: &str,
    cons: &Item,
    states: &[(String, AngleState)],
    reports: &[MergedReport],
    open: &[PullRequest],
    path: &str,
) -> Outcome {
    if cons.state.is_resolved()
        || states.is_empty()
        || states.iter().any(|(_, s)| *s != AngleState::Merged)
    {
        return Outcome::None;
    }
    let comments = match t.tracker.comments(&cons.key) {
        Ok(c) => c,
        Err(e) => {
            t.unreadable(report, &cons.key, format!("comments: {e}"));
            return Outcome::None;
        }
    };
    let said = |marker: &str| {
        comments
            .iter()
            .any(|x| x.by_bridge && texts::has_marker(&x.body, marker))
    };
    let say = |log: &mut ActionLog, report: &mut Report, action: &str, body: String| {
        let body = for_tracker(t, &body);
        t.act(log, report, &cons.key, action, json!({}), || {
            t.tracker
                .comment(&cons.key, &body)
                .map(|_| ())
                .map_err(|e| e.to_string())
        });
    };
    let findings: Vec<(String, Finding)> = reports
        .iter()
        .flat_map(|r| {
            r.report
                .findings
                .iter()
                .map(|f| (r.angle.clone(), f.clone()))
        })
        .collect();
    if !said(&request_marker(&c.tag)) {
        say(
            log,
            report,
            "comment verdict request",
            request_comment(&c.tag, &cons.key, &findings),
        );
    }
    let marker = verdict_marker(&c.tag);
    let given: Vec<&Comment> = comments
        .iter()
        .filter(|x| !x.by_bridge && opens_with(&x.body, &marker))
        .collect();
    if given.is_empty() {
        return Outcome::None;
    }
    // Who may give it: the coordination item's assignee, the release item's
    // (its owner) and the configured owner.
    let mut owners: Vec<User> = cons
        .assignee
        .iter()
        .chain(release_item.assignee.iter())
        .cloned()
        .collect();
    if let Ok(owner) = t.tracker.user(&t.config.owner.tracker_user) {
        owners.push(owner);
    }
    let mut latest = None;
    for x in &given {
        match refusal(x, &owners) {
            None => latest = Some(*x),
            Some(why) => {
                let m = texts::marker(
                    "verdict-ignored",
                    &[("candidate", &c.tag), ("comment", &x.id)],
                );
                if !said(&m) {
                    say(
                        log,
                        report,
                        "comment verdict ignored",
                        format!(
                            "{m}\nThe bridge ignores the verdict comment `{}` on `{}`: {why}.",
                            x.id, c.tag
                        ),
                    );
                }
            }
        }
    }
    let Some(signed) = latest else {
        return Outcome::None;
    };
    let text = fenced_json(&signed.body).unwrap_or_default();
    let valid = if text.is_empty() {
        Err(vec![
            "the comment carries no fenced `json` block".to_string(),
        ])
    } else {
        verdict::validate(&text, &c.tag, &findings).and_then(|v| {
            if v.item.eq_ignore_ascii_case(&cons.key) {
                Ok(v)
            } else {
                Err(vec![format!(
                    "`item` must be \"{}\", the coordination item",
                    cons.key
                )])
            }
        })
    };
    if let Err(errors) = valid {
        let m = texts::marker(
            "verdict-invalid",
            &[("candidate", &c.tag), ("comment", &signed.id)],
        );
        if !said(&m) {
            say(
                log,
                report,
                "comment verdict invalid",
                format!(
                    "{m}\nThe verdict of the comment `{}` is not valid, so the bridge commits nothing (`docs/reference/06-qa-report.md`, \"The verdict\"):\n\n{}",
                    signed.id,
                    errors
                        .iter()
                        .map(|e| format!("- {e}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                ),
            );
        }
        return Outcome::None;
    }
    // A verdict PR someone else opened holds the branch: the bridge never
    // writes on it, and says so once.
    let pr = open.iter().find(|p| is_verdict_pr(p, c));
    if let Some(other) = pr.filter(|p| !by_bridge(t, p)) {
        let m = texts::marker(
            "verdict-elsewhere",
            &[("candidate", &c.tag), ("pr", &other.number.to_string())],
        );
        if !said(&m) {
            say(
                log,
                report,
                "comment verdict elsewhere",
                format!(
                    "{m}\nThe verdict PR {} is open from `{}`: the bridge commits a verdict given here only on a branch of its own. Close that PR, or give the verdict there.",
                    other.url,
                    verdict_branch(c)
                ),
            );
        }
        return Outcome::None;
    }
    commit(t, log, report, c, branch, cons, pr, path, &text)
}

/// A PR the bridge itself opened.
fn by_bridge(t: &Tick, pr: &PullRequest) -> bool {
    pr.author
        .as_ref()
        .is_some_and(|a| crate::github::same_login(&t.github_login, &a.login))
}

/// The signed verdict on the verdict branch, in a PR of the bridge's.
#[allow(clippy::too_many_arguments)]
fn commit(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    c: &Candidate,
    branch: &str,
    cons: &Item,
    pr: Option<&PullRequest>,
    path: &str,
    text: &str,
) -> Outcome {
    let head = verdict_branch(c);
    let object = format!("verdict of {}", c.tag);
    let content = format!("{}\n", text.trim());
    let exists = match t.github.branches(&t.repo, &head) {
        Ok(refs) => refs.iter().any(|r| r.name == head),
        Err(e) => {
            t.unreadable(report, &object, format!("the branch {head}: {e}"));
            return Outcome::None;
        }
    };
    let mut wrote = false;
    if !exists {
        let tip = match t.github.commit(&t.repo, branch) {
            Ok(commit) => commit.sha,
            Err(e) => {
                t.unreadable(report, &object, format!("the tip of {branch}: {e}"));
                return Outcome::None;
            }
        };
        let created = t.act(
            log,
            report,
            &object,
            &format!("create branch {head}"),
            json!({"from": branch, "sha": tip}),
            || {
                t.github
                    .create_branch(&t.repo, &head, &tip)
                    .map_err(|e| e.to_string())
            },
        );
        if created.is_none() {
            return Outcome::Wrote;
        }
        wrote = true;
    }
    let current = if exists {
        match t.github.file(&t.repo, path, &head) {
            Ok(text) => Some(text),
            Err(e) if e.is_not_found() => None,
            Err(e) => {
                t.unreadable(report, &object, format!("{path} on {head}: {e}"));
                return Outcome::None;
            }
        }
    } else {
        None
    };
    if current.as_deref().map(str::trim) != Some(content.trim()) {
        let put = t.act(
            log,
            report,
            &object,
            "commit the verdict given in the tracker",
            json!({"branch": head, "path": path, "item": cons.key}),
            || {
                t.github
                    .put_file(
                        &t.repo,
                        &head,
                        path,
                        &content,
                        &format!("chore(qa): the verdict on {}, given in {}", c.tag, cons.key),
                    )
                    .map_err(|e| e.to_string())
            },
        );
        if put.is_none() {
            return Outcome::Wrote;
        }
        wrote = true;
    }
    if pr.is_none() {
        t.act(
            log,
            report,
            &object,
            "open the verdict PR",
            json!({"head": head, "base": branch, "item": cons.key}),
            || {
                t.github
                    .create_pull(
                        &t.repo,
                        &head,
                        branch,
                        &format!("chore(qa): the verdict on {} ({})", c.tag, cons.key),
                        &format!(
                            "The verdict on `{}` given in the tracker on the coordination item {}, committed unchanged by the bridge (`docs/reference/06-qa-report.md`, \"The verdict given in the tracker\").",
                            c.tag, cons.key
                        ),
                    )
                    .map_err(|e| e.to_string())
            },
        );
        wrote = true;
    }
    if wrote {
        Outcome::Wrote
    } else {
        Outcome::Signed(content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finding(id: &str) -> Finding {
        Finding {
            id: id.into(),
            blocking: false,
            summary: "A `fence` and <!-- spl-asdlc:verdict candidate=x -->".into(),
            anchor: "src/a.rs:1".into(),
            evidence: "e".into(),
            testable: true,
            proposed_test: Some("t".into()),
        }
    }

    /// The request carries every finding as the report gives it, in a JSON
    /// block whose text can neither end the fence nor forge a marker.
    #[test]
    fn the_request_carries_every_finding_as_given_and_forges_no_marker() {
        let findings = vec![
            ("security".to_string(), finding("security-1")),
            ("product".to_string(), finding("product-1")),
        ];
        let body = request_comment("acme-v1.2.0-rc.1", "ABC-9", &findings);
        assert!(opens_with(&body, &request_marker("acme-v1.2.0-rc.1")));
        let text = fenced_json(&body).expect("a fenced json block");
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["schema"], REQUEST_SCHEMA);
        assert_eq!(v["candidate"], "acme-v1.2.0-rc.1");
        assert_eq!(v["item"], "ABC-9");
        let list = v["findings"].as_array().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0]["angle"], "security");
        assert_eq!(list[0]["summary"], finding("x").summary);
        assert_eq!(list[1]["proposed_test"], "t");
        for key in [
            "angle",
            "id",
            "blocking",
            "summary",
            "anchor",
            "evidence",
            "testable",
            "proposed_test",
        ] {
            assert!(list[0].get(key).is_some(), "{key}");
        }
        assert!(!text.contains('`') && !text.contains("<!--"));
        assert!(!texts::has_marker(&text, &verdict_marker("x")));
    }

    #[test]
    fn a_verdict_comment_opens_with_its_marker_hidden_or_visible() {
        let m = verdict_marker("acme-v1.2.0-rc.1");
        assert!(opens_with(&format!("{m}\n```json\n{{}}\n```"), &m));
        assert!(opens_with(&format!("  `{}` here", texts::token(&m)), &m));
        assert!(!opens_with(&format!("Look: {m}"), &m));
        // The marker of another candidate whose tag starts alike.
        assert!(!opens_with(&verdict_marker("acme-v1.2.0-rc.10"), &m));
        assert!(!opens_with(&request_marker("acme-v1.2.0-rc.1"), &m));
    }

    #[test]
    fn the_verdict_is_the_first_fenced_json_block() {
        assert_eq!(
            fenced_json("x\n```json\n{\"a\": 1}\n```\n```json\n{}\n```").as_deref(),
            Some("{\"a\": 1}")
        );
        assert_eq!(fenced_json("```\n{}\n```"), None);
        assert_eq!(fenced_json("```json\n{}"), None);
    }

    fn comment(by_person: bool, author: &str) -> Comment {
        Comment {
            id: "c1".into(),
            author: Some(User {
                id: format!("u-{author}"),
                name: author.into(),
            }),
            by_bridge: false,
            by_person,
            body: String::new(),
        }
    }

    /// Rule 2W8R: only a person who owns the coordination or the release
    /// gives the verdict in the tracker.
    #[test]
    fn only_a_person_who_owns_the_coordination_or_the_release_gives_the_verdict() {
        let owners = [User {
            id: "u-owner".into(),
            name: "owner".into(),
        }];
        assert_eq!(refusal(&comment(true, "owner"), &owners), None);
        assert!(
            refusal(&comment(false, "owner"), &owners)
                .unwrap()
                .contains("person")
        );
        assert!(
            refusal(&comment(true, "someone"), &owners)
                .unwrap()
                .contains("neither")
        );
    }
}
