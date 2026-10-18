//! `pr-title` (REQ-CI-015..020), its step run as shipped.

use crate::harness::{Ran, Scratch, run, run_body, step, workflow};

fn check(title: &str, head: &str, base: &str) -> Ran {
    let doc = workflow(".github/workflows/asdlc-pr-title.yml");
    let body = run_body(&step(&doc, "conventional-title", "title"));
    let inputs = &doc["on"]["workflow_call"]["inputs"];
    let default = |k: &str| inputs[k]["default"].as_str().unwrap().to_string();
    let (types, branch, rel, rp, de, re) = (
        default("allowed-types"),
        default("default-branch"),
        default("release-branch-prefix"),
        default("release-please-head-prefix"),
        default("default-branch-exempt-heads"),
        default("release-branch-exempt-heads"),
    );
    let dir = Scratch::new("title");
    run(
        &body,
        &[
            ("TITLE", title),
            ("HEAD_REF", head),
            ("BASE_REF", base),
            ("ALLOWED_TYPES", &types),
            ("DEFAULT_BRANCH", &branch),
            ("RELEASE_PREFIX", &rel),
            ("RELEASE_PLEASE_PREFIX", &rp),
            ("DEFAULT_EXEMPT", &de),
            ("RELEASE_EXEMPT", &re),
        ],
        &dir.0,
    )
}

#[test]
fn conventional_titles_pass() {
    for t in [
        "fix: a subject",
        "feat(merge): a subject (ABC-12)",
        "feat!: drop the old flag",
        "refactor(pr-merge)!: x",
        "docs: x",
    ] {
        let r = check(t, "feat/x", "main");
        assert_eq!(r.code, 0, "{t}: {}", r.log);
        assert_eq!(r.output("exempt").as_deref(), Some("false"));
    }
}

#[test]
fn a_refused_title_says_what_to_fix() {
    for (t, hint) in [
        ("ABC-12 feat: add a filter", "starts with a ticket id"),
        ("add a filter", "no 'type: ' prefix"),
        ("Fix: a subject", "lower-case: 'fix'"),
        ("fix:a subject", "exactly one space"),
        ("fix:  a subject", "exactly one space"),
        ("fix(): a subject", "scope is empty"),
        ("fix(pr merge): a subject", "holds no space"),
        ("feature: a subject", "'feature' is not an allowed type"),
        ("fix: a subject ", "does not end with a space"),
    ] {
        let r = check(t, "feat/x", "main");
        assert_eq!(r.code, 1, "{t} must be refused: {}", r.log);
        assert!(
            r.log
                .contains("::error title=PR title is not a Conventional Commit::"),
            "{t}: {}",
            r.log
        );
        assert!(
            r.log.contains(hint),
            "{t}: expected the hint {hint:?}: {}",
            r.log
        );
        assert!(r.log.contains("Fix:    edit the PR title"), "{t}");
    }
}

#[test]
fn the_release_chains_own_prs_are_exempt_by_branch_and_base() {
    for (head, base) in [
        ("release-please--branches--main", "main"),
        (
            "release-please--branches--release/2026-10-05-14-03-utc",
            "release/2026-10-05-14-03-utc",
        ),
        ("release/2026-10-05-14-03-utc", "main"),
        ("chore/merge-back-2026-10-05", "main"),
        (
            "qa/acme-v1.2.0-rc.1/security",
            "release/2026-10-05-14-03-utc",
        ),
    ] {
        let r = check("not conventional at all", head, base);
        assert_eq!(r.code, 0, "{head} → {base}: {}", r.log);
        assert_eq!(
            r.output("exempt").as_deref(),
            Some("true"),
            "{head} → {base}"
        );
    }
    // Never exempt: a delivery PR into a release branch, a release branch or
    // a merge-back resolution into anything but main, an angle branch into
    // main, a head named like the former gate record (gone with it).
    for (head, base) in [
        ("fix/hotfix", "release/2026-10-05-14-03-utc"),
        (
            "release-record/acme-v1.2.0-rc.1",
            "release/2026-10-05-14-03-utc",
        ),
        ("release/2026-10-05-14-03-utc", "release/other"),
        (
            "chore/merge-back-2026-10-05",
            "release/2026-10-05-14-03-utc",
        ),
        ("qa/acme-v1.2.0-rc.1/security", "main"),
    ] {
        let r = check("not conventional at all", head, base);
        assert_eq!(r.code, 1, "{head} → {base} must be checked: {}", r.log);
    }
}

#[test]
fn a_title_cannot_inject_shell() {
    let dir = Scratch::new("inject");
    let marker = dir.0.join("pwned");
    let title = format!(
        "fix: $(touch {m}) `touch {m}` \"; touch {m}",
        m = marker.display()
    );
    let r = check(&title, "feat/x", "main");
    assert_eq!(r.code, 0, "{}", r.log);
    assert!(!marker.exists(), "the title ran a command");
}
