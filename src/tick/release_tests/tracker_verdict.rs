//! A coordinator's verdict given in the tracker (rule 2W8R): the request
//! posted once every angle is merged, the verdict a person signs committed
//! by the bridge in a PR of its own and merged as any verdict PR, every
//! other verdict comment told once and ignored.

use super::*;
use crate::tick::tracker_verdict::{fenced_json, request_marker, verdict_marker};

fn path() -> String {
    format!(".spl-asdlc/qa/{CAND}/consolidation.json")
}

fn verdict_branch() -> String {
    format!("qa/{CAND}/consolidation")
}

/// The round of `CAND` open, the release item the owner's, the release
/// branch's tip readable.
fn opened() -> World {
    let w = World::new();
    w.owner_known();
    w.tick();
    w.github.state.borrow_mut().commits.insert(
        (repo(), BRANCH.into()),
        Ok(Commit {
            sha: "c1".into(),
            message: "chore: candidate".into(),
            committer_date: None,
            parents: vec![],
        }),
    );
    w
}

/// Both angles merged, one finding each.
fn angles_merged(w: &World, verdict: Option<String>) {
    w.merged_with_verdict(
        &[
            (
                30,
                "security",
                report_json("security", &finding("security-1", true)),
            ),
            (
                31,
                "product",
                report_json("product", &finding("product-1", false)),
            ),
        ],
        verdict,
    );
}

/// A valid verdict on both findings: the blocking one accepted, the other
/// rejected, so `hold`.
fn valid_verdict() -> String {
    verdict_json(
        "hold",
        &[
            judged("security", "security-1", Some(true)),
            judged("product", "product-1", None),
        ]
        .join(","),
    )
}

/// A verdict comment on `CAND`, for the coordination item `cons`.
fn given(cons: &str, verdict: &str) -> String {
    format!(
        "{}\nMy verdict.\n\n```json\n{}\n```\n",
        verdict_marker(CAND),
        verdict.replace("ABC-9", cons)
    )
}

/// What the bridge commits for `verdict`: its text unchanged.
fn committed(cons: &str, verdict: &str) -> String {
    format!("{}\n", verdict.replace("ABC-9", cons).trim())
}

fn bridge_comments(w: &World, key: &str, marker: &str) -> Vec<String> {
    w.tracker
        .comments(key)
        .unwrap()
        .into_iter()
        .filter(|c| c.by_bridge && texts::has_marker(&c.body, marker))
        .map(|c| c.body)
        .collect()
}

fn branch_writes(w: &World) -> Vec<String> {
    w.github
        .writes()
        .into_iter()
        .filter(|l| l.contains(&verdict_branch()))
        .collect()
}

/// The bridge's own verdict PR, open and green, its file `text`.
fn bridge_pr(w: &World, cons: &str, head: &str, text: &str) -> PullRequest {
    let mut pr = green(pull_request(40));
    pr.base_ref = BRANCH.into();
    pr.head_ref = verdict_branch();
    pr.head_sha = head.into();
    pr.title = format!("chore(qa): the verdict on {CAND} ({cons})");
    pr.author = Some(Actor {
        login: bot("spl-asdlc"),
        is_bot: true,
    });
    let mut s = w.github.state.borrow_mut();
    s.files
        .insert((repo(), head.into(), path()), Ok(text.to_string()));
    s.compares.insert(
        (repo(), BRANCH.into(), head.into()),
        cmp(CompareStatus::Ahead, &["v1"], &[&path()]),
    );
    s.comments.insert((repo(), 40), Ok(Vec::new()));
    pr
}

#[test]
fn the_findings_are_asked_on_the_coordination_item_once_every_angle_is_merged() {
    let w = opened();
    let cons = w.consolidation(CAND).key;
    w.tick();
    assert!(
        bridge_comments(&w, &cons, &request_marker(CAND)).is_empty(),
        "no request while an angle is not merged"
    );
    angles_merged(&w, None);
    w.tick();
    w.tick();
    let asked = bridge_comments(&w, &cons, &request_marker(CAND));
    assert_eq!(asked.len(), 1, "once per candidate");
    let v: serde_json::Value =
        serde_json::from_str(&fenced_json(&asked[0]).expect("a json block")).unwrap();
    assert_eq!(v["schema"], "spl-asdlc/verdict-request/1");
    assert_eq!(v["candidate"], CAND);
    assert_eq!(v["item"], cons.as_str());
    let found: Vec<(String, String, bool)> = v["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            (
                f["angle"].as_str().unwrap().to_string(),
                f["id"].as_str().unwrap().to_string(),
                f["blocking"].as_bool().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        found,
        [
            ("security".to_string(), "security-1".to_string(), true),
            ("product".to_string(), "product-1".to_string(), false),
        ]
    );
    assert_eq!(v["findings"][0]["proposed_test"], "test_security-1");
}

/// Rule 2W8R: the verdict the release's owner signs in the tracker is
/// committed unchanged by the bridge, from the release branch's tip, in a PR
/// naming the coordination item.
#[test]
fn a_verdict_the_owner_signs_in_the_tracker_is_committed_unchanged_in_a_pr_of_the_bridges() {
    let w = opened();
    let cons = w.consolidation(CAND).key;
    angles_merged(&w, None);
    w.tracker
        .human_comment(&cons, "owner", &given(&cons, &valid_verdict()));
    w.tick();
    let writes = branch_writes(&w);
    assert_eq!(
        writes,
        [
            format!("create-branch example-org/acme {} at c1", verdict_branch()),
            format!(
                "put-file example-org/acme {} {} (chore(qa): the verdict on {CAND}, given in {cons})",
                verdict_branch(),
                path()
            ),
            format!(
                "create-pull example-org/acme {} -> {BRANCH} chore(qa): the verdict on {CAND} ({cons})",
                verdict_branch()
            ),
        ],
        "{:?}",
        w.github.writes()
    );
    assert_eq!(
        w.github.file(&repo(), &path(), &verdict_branch()).unwrap(),
        committed(&cons, &valid_verdict())
    );
    assert!(!w.github.writes().iter().any(|l| l.starts_with("merge")));
}

/// Rule 2W8R: the bridge's verdict PR is merged on the strength of the
/// signed comment, by the rules of any verdict PR; once the file is no
/// longer what was signed, the bridge's authorship counts for nothing.
#[test]
fn the_bridges_verdict_pr_is_merged_while_it_carries_the_signed_verdict() {
    let w = opened();
    let cons = w.consolidation(CAND).key;
    angles_merged(&w, None);
    w.tracker
        .human_comment(&cons, "owner", &given(&cons, &valid_verdict()));
    w.tick();
    let text = committed(&cons, &valid_verdict());
    let pr = bridge_pr(&w, &cons, "vhead00000001", &text);
    w.open_on_branch(vec![pr]);
    let before = branch_writes(&w).len();
    w.tick();
    assert_eq!(branch_writes(&w).len(), before, "nothing written again");
    assert!(
        w.github
            .writes()
            .contains(&"merge example-org/acme#40 at vhead00000001 merge".to_string()),
        "{:?}",
        w.github.writes()
    );

    // Its file changed by hand: neither the coordinator's nor signed.
    let w = opened();
    let cons = w.consolidation(CAND).key;
    angles_merged(&w, None);
    let other = verdict_json(
        "go",
        &[
            judged("security", "security-1", Some(false)),
            judged("product", "product-1", None),
        ]
        .join(","),
    )
    .replace("ABC-9", &cons);
    let pr = bridge_pr(&w, &cons, "vhead00000002", &other);
    w.open_on_branch(vec![pr]);
    w.tick();
    assert!(!w.github.writes().iter().any(|l| l.starts_with("merge")));
    assert!(
        w.github
            .writes()
            .iter()
            .any(|l| l.starts_with("comment example-org/acme#40 ")),
        "{:?}",
        w.github.writes()
    );
}

/// A newer verdict before the merge replaces the file on the same branch,
/// one more commit; a merged verdict is never committed again.
#[test]
fn a_newer_verdict_replaces_the_file_and_a_merged_one_is_never_committed_again() {
    let w = opened();
    let cons = w.consolidation(CAND).key;
    angles_merged(&w, None);
    w.tracker
        .human_comment(&cons, "owner", &given(&cons, &valid_verdict()));
    w.tick();
    let pr = bridge_pr(
        &w,
        &cons,
        "vhead00000003",
        &committed(&cons, &valid_verdict()),
    );
    w.open_on_branch(vec![pr]);
    let newer = verdict_json(
        "hold",
        &[
            judged("security", "security-1", Some(true)),
            judged("product", "product-1", Some(false)),
        ]
        .join(","),
    );
    w.tracker
        .human_comment(&cons, "owner", &given(&cons, &newer));
    let before = branch_writes(&w).len();
    w.tick();
    let after: Vec<String> = branch_writes(&w)[before..].to_vec();
    assert_eq!(after.len(), 1, "{after:?}");
    assert!(after[0].starts_with("put-file "), "{after:?}");
    assert_eq!(
        w.github.file(&repo(), &path(), &verdict_branch()).unwrap(),
        committed(&cons, &newer)
    );
    assert!(
        !w.github.writes().iter().any(|l| l.starts_with("merge")),
        "the PR waits for its new head"
    );

    // Merged: never committed again, whatever the tracker says next.
    let w = opened();
    let cons = w.consolidation(CAND).key;
    angles_merged(&w, Some(valid_verdict()));
    w.tracker
        .human_comment(&cons, "owner", &given(&cons, &newer));
    w.tick();
    assert!(branch_writes(&w).is_empty(), "{:?}", w.github.writes());
}

/// Rule 2W8R: a verdict signed by an agent, or by a person who is neither
/// the coordination item's assignee nor the release's owner, is ignored and
/// told once; an invalid one is told once with its errors; nothing is
/// committed.
#[test]
fn a_verdict_from_anyone_else_or_invalid_is_told_once_and_commits_nothing() {
    let w = opened();
    let cons = w.consolidation(CAND).key;
    angles_merged(&w, None);
    w.tracker
        .agent_comment(&cons, "owner", &given(&cons, &valid_verdict()));
    w.tracker
        .human_comment(&cons, "stranger", &given(&cons, &valid_verdict()));
    w.tracker.human_comment(
        &cons,
        "owner",
        &given(
            &cons,
            &verdict_json("go", &judged("security", "security-1", Some(true))),
        ),
    );
    w.tick();
    w.tick();
    let ignored = bridge_comments(&w, &cons, "spl-asdlc:verdict-ignored");
    assert_eq!(ignored.len(), 2, "{ignored:?}");
    assert!(ignored.iter().any(|c| c.contains("person")));
    assert!(ignored.iter().any(|c| c.contains("neither")));
    let invalid = bridge_comments(&w, &cons, "spl-asdlc:verdict-invalid");
    assert_eq!(invalid.len(), 1, "{invalid:?}");
    assert!(invalid[0].contains("`product-1`"), "{}", invalid[0]);
    assert!(branch_writes(&w).is_empty(), "{:?}", w.github.writes());
}
