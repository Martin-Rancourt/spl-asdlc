//! `pr-guards` (REQ-CI-026..028, 031), its steps run as shipped against a
//! real git repository.

use crate::harness::{Ran, Repo, run, run_body, step, workflow};

fn step_body(id: &str) -> String {
    run_body(&step(
        &workflow(".github/workflows/pr-guards.yml"),
        "guards",
        id,
    ))
}

fn prose_default() -> String {
    workflow(".github/workflows/pr-guards.yml")["on"]["workflow_call"]["inputs"]["prose-regex"]["default"]
        .as_str()
        .unwrap()
        .to_string()
}

fn detect(repo: &Repo, base: &str, head: &str) -> Ran {
    run(
        &step_body("code"),
        &[
            ("BASE_SHA", base),
            ("HEAD_SHA", head),
            ("PROSE_RE", &prose_default()),
        ],
        &repo.dir.0,
    )
}

#[test]
fn only_a_prose_only_change_skips_the_tests() {
    let repo = Repo::new();
    let base = repo.git(&["rev-parse", "HEAD"]);
    repo.write("docs/guide.md", "words\n");
    repo.write("README.md", "more\n");
    let prose = repo.commit("prose");
    assert_eq!(
        detect(&repo, &base, &prose).output("run").as_deref(),
        Some("false")
    );
    repo.write("src/lib.rs", "pub fn g() {}\n");
    let code = repo.commit("code");
    assert_eq!(
        detect(&repo, &base, &code).output("run").as_deref(),
        Some("true")
    );
    // A new top-level directory is not prose.
    repo.write("tools/x.sh", "true\n");
    let tools = repo.commit("tools");
    assert_eq!(
        detect(&repo, &prose, &tools).output("run").as_deref(),
        Some("true")
    );
}

#[test]
fn code_moved_into_docs_is_still_code() {
    let repo = Repo::new();
    repo.git(&["config", "diff.renames", "copies"]);
    let base = repo.git(&["rev-parse", "HEAD"]);
    std::fs::create_dir_all(repo.dir.0.join("docs")).unwrap();
    repo.git(&["mv", "src/lib.rs", "docs/lib.md"]);
    let head = repo.commit("move");
    assert_eq!(
        detect(&repo, &base, &head).output("run").as_deref(),
        Some("true")
    );
}

#[test]
fn an_unreadable_or_empty_diff_runs_the_tests() {
    let repo = Repo::new();
    let head = repo.git(&["rev-parse", "HEAD"]);
    let r = detect(&repo, "0000000000000000000000000000000000000000", &head);
    assert_eq!(r.output("run").as_deref(), Some("true"), "{}", r.log);
    assert!(r.log.contains("::warning::"), "{}", r.log);
    let r = detect(&repo, &head, &head);
    assert_eq!(r.output("run").as_deref(), Some("true"), "{}", r.log);
    let r = detect(&repo, "", &head);
    assert_eq!(r.output("run").as_deref(), Some("true"), "{}", r.log);
}

#[test]
fn a_huge_diff_is_classified_without_a_broken_pipe() {
    let repo = Repo::new();
    let base = repo.git(&["rev-parse", "HEAD"]);
    for i in 0..3000 {
        repo.write(&format!("docs/page-{i:05}.md"), "x\n");
    }
    repo.write("src/z.rs", "pub fn z() {}\n");
    let head = repo.commit("many");
    assert_eq!(
        detect(&repo, &base, &head).output("run").as_deref(),
        Some("true")
    );
}

#[test]
fn the_extra_detectors_fail_in_the_safe_direction() {
    let repo = Repo::new();
    let base = repo.git(&["rev-parse", "HEAD"]);
    repo.write("apps/website/page.tsx", "x\n");
    let head = repo.commit("website");
    let detectors = r#"[
        {"id": "db", "skip-regex": "(\\.md$|^docs/|^apps/website/)"},
        {"id": "specs", "match-regex": "^tests/quality/.+\\.spec\\.ts$", "diff-filter": "AMR"},
        {"id": "web", "match-regex": "^apps/website/"}
    ]"#;
    let r = run(
        &step_body("detectors"),
        &[
            ("BASE_SHA", &base),
            ("HEAD_SHA", &head),
            ("DETECTORS", detectors),
        ],
        &repo.dir.0,
    );
    assert_eq!(r.code, 0, "{}", r.log);
    let d = r.output("detections").unwrap();
    assert!(d.contains(r#""db":"false""#), "{d}");
    assert!(d.contains(r#""specs":"false""#), "{d}");
    assert!(d.contains(r#""web":"true""#), "{d}");
    // An unreadable diff runs every detector.
    let r = run(
        &step_body("detectors"),
        &[
            ("BASE_SHA", "0000000000000000000000000000000000000000"),
            ("HEAD_SHA", &head),
            ("DETECTORS", detectors),
        ],
        &repo.dir.0,
    );
    let d = r.output("detections").unwrap();
    assert!(
        d.contains(r#""db":"true""#) && d.contains(r#""specs":"true""#),
        "{d}"
    );
    // A malformed detector is refused.
    let r = run(
        &step_body("detectors"),
        &[
            ("BASE_SHA", &base),
            ("HEAD_SHA", &head),
            ("DETECTORS", r#"[{"id": "x"}]"#),
        ],
        &repo.dir.0,
    );
    assert_eq!(r.code, 1, "{}", r.log);
}

#[test]
fn every_guard_command_runs_with_the_refs_and_all_must_pass() {
    let repo = Repo::new();
    let base = repo.git(&["rev-parse", "HEAD"]);
    let guards = r#"[
        {"name": "refs are set", "run": "test -n \"$BASE_SHA\" && test -n \"$HEAD_SHA\" && test \"$HEAD_REF\" = feat/x"},
        {"name": "fails", "run": "exit 3"},
        {"name": "still runs", "run": "touch ran"}
    ]"#;
    let r = run(
        &step_body("Guard commands"),
        &[
            ("BASE_SHA", &base),
            ("HEAD_SHA", &base),
            ("HEAD_REF", "feat/x"),
            ("BASE_REF", "main"),
            ("GUARDS", guards),
        ],
        &repo.dir.0,
    );
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(r.log.contains("Failed guard(s): fails"), "{}", r.log);
    assert!(
        repo.dir.0.join("ran").exists(),
        "a failure does not stop the next guard"
    );
    let r = run(
        &step_body("Guard commands"),
        &[
            ("BASE_SHA", &base),
            ("HEAD_SHA", &base),
            ("HEAD_REF", "feat/x"),
            ("BASE_REF", "main"),
            ("GUARDS", "[]"),
        ],
        &repo.dir.0,
    );
    assert_eq!(r.code, 0, "{}", r.log);
}

#[test]
fn release_pleases_heads_skip_everything() {
    let body = step_body("skip");
    let repo = Repo::new();
    let r = run(
        &body,
        &[
            ("HEAD_REF", "release-please--branches--main"),
            ("SKIP_PREFIXES", "release-please--"),
        ],
        &repo.dir.0,
    );
    assert_eq!(r.output("skipped").as_deref(), Some("true"));
    let r = run(
        &body,
        &[
            ("HEAD_REF", "feat/x"),
            ("SKIP_PREFIXES", "release-please--"),
        ],
        &repo.dir.0,
    );
    assert_eq!(r.output("skipped").as_deref(), Some("false"));
    let r = run(
        &body,
        &[("HEAD_REF", "release-please--x"), ("SKIP_PREFIXES", "")],
        &repo.dir.0,
    );
    assert_eq!(r.output("skipped").as_deref(), Some("false"), "{}", r.log);
}
