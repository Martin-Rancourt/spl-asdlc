//! `pr-guards` (REQ-CI-026..028, 031), its steps run as shipped against a
//! real git repository.

use crate::harness::{Ran, Repo, run, run_body, step, workflow};

fn step_body(id: &str) -> String {
    run_body(&step(
        &workflow(".github/workflows/asdlc-pr-guards.yml"),
        "guards",
        id,
    ))
}

fn prose_default() -> String {
    workflow(".github/workflows/asdlc-pr-guards.yml")["on"]["workflow_call"]["inputs"]["prose-regex"]["default"]
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
            ("HEAD_REPO", "o/r"),
            ("REPO", "o/r"),
            ("SKIP_PREFIXES", "release-please--"),
        ],
        &repo.dir.0,
    );
    assert_eq!(r.output("skipped").as_deref(), Some("true"));
    let r = run(
        &body,
        &[
            ("HEAD_REF", "feat/x"),
            ("HEAD_REPO", "o/r"),
            ("REPO", "o/r"),
            ("SKIP_PREFIXES", "release-please--"),
        ],
        &repo.dir.0,
    );
    assert_eq!(r.output("skipped").as_deref(), Some("false"));
    let r = run(
        &body,
        &[
            ("HEAD_REF", "release-please--x"),
            ("HEAD_REPO", "o/r"),
            ("REPO", "o/r"),
            ("SKIP_PREFIXES", ""),
        ],
        &repo.dir.0,
    );
    assert_eq!(r.output("skipped").as_deref(), Some("false"), "{}", r.log);
    // A fork naming its branch like release-please's is never skipped.
    let r = run(
        &body,
        &[
            ("HEAD_REF", "release-please--x"),
            ("HEAD_REPO", "fork/r"),
            ("REPO", "o/r"),
            ("SKIP_PREFIXES", "release-please--"),
        ],
        &repo.dir.0,
    );
    assert_eq!(r.output("skipped").as_deref(), Some("false"), "{}", r.log);
    let r = run(
        &body,
        &[
            ("HEAD_REF", "release-please--x"),
            ("HEAD_REPO", ""),
            ("REPO", "o/r"),
            ("SKIP_PREFIXES", "release-please--"),
        ],
        &repo.dir.0,
    );
    assert_eq!(r.output("skipped").as_deref(), Some("false"), "{}", r.log);
}

// ---- the process page (BR-ASDLC-026, BR-ASDLC-028) ----

const PROCESS: &str = "docs/agents/process.md";

/// The process step's script, and the blob id it carries.
fn process_step() -> (String, String) {
    let s = step(
        &workflow(".github/workflows/asdlc-pr-guards.yml"),
        "guards",
        "process",
    );
    let blob = s["env"]["PINNED_BLOB"]
        .as_str()
        .expect("the step carries the pinned blob id")
        .to_string();
    (run_body(&s), blob)
}

fn check_page(repo: &Repo, head: &str, page: &str) -> Ran {
    let (body, blob) = process_step();
    run(
        &body,
        &[("HEAD_SHA", head), ("PAGE", page), ("PINNED_BLOB", &blob)],
        &repo.dir.0,
    )
}

fn method_page() -> String {
    std::fs::read_to_string(crate::harness::root().join(PROCESS)).unwrap()
}

#[test]
fn the_guards_carry_the_blob_id_of_this_versions_process_page() {
    // The workflow of a version knows that version's page by its git blob
    // id: a PR that changes the page changes the id in the same commit.
    let out = std::process::Command::new("git")
        .args(["hash-object", PROCESS])
        .current_dir(crate::harness::root())
        .output()
        .unwrap();
    assert!(out.status.success(), "git hash-object {PROCESS}");
    let actual = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let (_, pinned) = process_step();
    assert_eq!(
        pinned, actual,
        "write PINNED_BLOB: {actual} in the process step of asdlc-pr-guards.yml"
    );
    // The default place in a project, and this repository's own call.
    let doc = workflow(".github/workflows/asdlc-pr-guards.yml");
    assert_eq!(
        doc["on"]["workflow_call"]["inputs"]["process-page"]["default"].as_str(),
        Some(".spl-asdlc/process.md")
    );
    let own = workflow(".github/workflows/pr.yml");
    assert_eq!(
        own["jobs"]["guards"]["with"]["process-page"].as_str(),
        Some(".spl-asdlc/process.md"),
        "this repository checks its copy at the pinned version, as any project (BR-ASDLC-023)"
    );
}

#[test]
fn the_pinned_process_page_passes_and_a_missing_or_edited_one_is_refused() {
    let repo = Repo::new();
    // Missing: refused, with the generic way to refresh it.
    let head = repo.git(&["rev-parse", "HEAD"]);
    let r = check_page(&repo, &head, ".spl-asdlc/process.md");
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(
        r.log.contains("::error title=No process page::"),
        "{}",
        r.log
    );
    assert!(r.log.contains("of the spl-asdlc version"), "{}", r.log);

    // The pinned page, unchanged: accepted, whatever the working tree holds.
    repo.write(".spl-asdlc/process.md", &method_page());
    let pinned = repo.commit("the process page");
    repo.write(".spl-asdlc/process.md", "not committed\n");
    let r = check_page(&repo, &pinned, ".spl-asdlc/process.md");
    assert_eq!(r.code, 0, "{}", r.log);
    assert!(
        r.log.contains("is the page of the pinned version"),
        "{}",
        r.log
    );

    // Edited by hand, one line added: refused, with the command that
    // refreshes it from the ref the project's callers name.
    repo.write(
        ".github/workflows/pr.yml",
        "jobs:\n  guards:\n    uses: example-org/spl-asdlc/.github/workflows/asdlc-pr-guards.yml@v1\n",
    );
    repo.write(
        ".spl-asdlc/process.md",
        &format!("{}\n- A project's own rule.\n", method_page()),
    );
    let edited = repo.commit("edit the page");
    let r = check_page(&repo, &edited, ".spl-asdlc/process.md");
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(
        r.log.contains("::error title=Process page changed::"),
        "{}",
        r.log
    );
    assert!(
        r.log.contains(
            r#"gh api "repos/example-org/spl-asdlc/contents/docs/agents/process.md?ref=v1" -H "Accept: application/vnd.github.raw" > .spl-asdlc/process.md"#
        ),
        "{}",
        r.log
    );
    assert!(r.log.contains("AGENTS.md"), "{}", r.log);

    // Another place, named by the caller, is read the same way.
    let r = check_page(&repo, &pinned, PROCESS);
    assert_eq!(r.code, 1, "{}", r.log);
    repo.write(PROCESS, &method_page());
    let moved = repo.commit("the page elsewhere");
    let r = check_page(&repo, &moved, PROCESS);
    assert_eq!(r.code, 0, "{}", r.log);

    // A directory in its place, or an empty input, is refused.
    let r = check_page(&repo, &moved, ".spl-asdlc");
    assert_eq!(r.code, 1, "{}", r.log);
    let r = check_page(&repo, &moved, "");
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(r.log.contains("Invalid process-page"), "{}", r.log);
}

#[test]
fn the_process_check_runs_on_every_pr_but_the_skipped_heads() {
    let s = step(
        &workflow(".github/workflows/asdlc-pr-guards.yml"),
        "guards",
        "process",
    );
    // No prose filter and no switch: only the release chain's own heads
    // skip it, as they skip every guard.
    assert_eq!(
        s["if"].as_str(),
        Some("steps.skip.outputs.skipped != 'true'")
    );
    assert!(s["continue-on-error"].is_badvalue());
}

fn check_branch(branch: &str, title: &str, check: &str) -> Ran {
    let repo = Repo::new();
    run(
        &step_body("branch"),
        &[("HEAD_REF", branch), ("TITLE", title), ("CHECK", check)],
        &repo.dir.0,
    )
}

#[test]
fn a_branch_named_after_its_ticket_passes_silently() {
    for (branch, title) in [
        (
            "fix/lvl-7gtc-mcp-asdlc-settings",
            "fix(mcp): ASDLC settings (LVL-7GTC)",
        ),
        ("feat/abc-12", "feat!: new screen (ABC-12)"),
        ("docs/abc-12-guide", "docs: guide"),
        // Not the method's to name.
        ("release/2026-10-25-15-55-utc", "fix: gate"),
        ("qa/security", "docs: security report"),
        ("dependabot/cargo/serde-1.0.200", "chore(deps): bump serde"),
        ("chore/merge-back-2026-10-25", "chore: merge back"),
    ] {
        let r = check_branch(branch, title, "refuse");
        assert_eq!(r.code, 0, "{branch}: {}", r.log);
        assert!(!r.log.contains("::warning"), "{branch}: {}", r.log);
    }
}

#[test]
fn a_misnamed_branch_warns_with_the_expected_name_and_refuses_when_asked() {
    for (branch, title) in [
        (
            "someone/lvl-7gtc-mcp-asdlc",
            "fix(mcp): ASDLC settings (LVL-7GTC)",
        ),
        (
            "fix/LVL-7GTC-mcp-asdlc",
            "fix(mcp): ASDLC settings (LVL-7GTC)",
        ),
        (
            "feat/lvl-7gtc-mcp-asdlc",
            "fix(mcp): ASDLC settings (LVL-7GTC)",
        ),
        (
            "fix/lvl-7gtd-mcp-asdlc",
            "fix(mcp): ASDLC settings (LVL-7GTC)",
        ),
    ] {
        let r = check_branch(branch, title, "warn");
        assert_eq!(r.code, 0, "{branch}: warn only: {}", r.log);
        assert!(
            r.log.contains("::warning title=Branch name::")
                && r.log.contains("expected 'fix/lvl-7gtc-asdlc-settings'"),
            "{branch}: {}",
            r.log
        );
        let r = check_branch(branch, title, "refuse");
        assert_eq!(r.code, 1, "{branch}: {}", r.log);
        assert!(r.log.contains("::error title=Branch name::"), "{}", r.log);
    }
    // A title without its type or id: the format alone, the rest to fill.
    let r = check_branch("patch-1", "Update README", "warn");
    assert!(
        r.log
            .contains("expected '<type>/<ticket-id>-update-readme'"),
        "{}",
        r.log
    );
}

#[test]
fn the_branch_check_warns_by_default_on_this_minor_line_and_rejects_a_bad_value() {
    let w = workflow(".github/workflows/asdlc-pr-guards.yml");
    assert_eq!(
        w["on"]["workflow_call"]["inputs"]["branch-name-check"]["default"].as_str(),
        Some("warn"),
        "1.4 only warns: open PRs of the projects must stay green"
    );
    let r = check_branch("fix/abc-1", "fix: x (ABC-1)", "maybe");
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(r.log.contains("Invalid branch-name-check"), "{}", r.log);
}

// ---- the status of the business rules a PR changes (ASDLC-R8WY) ----

/// The rules step's script and the registry it reads.
fn rules_step() -> (String, String) {
    let s = step(
        &workflow(".github/workflows/asdlc-pr-guards.yml"),
        "guards",
        "rules",
    );
    let dir = s["env"]["RULES_DIR"]
        .as_str()
        .expect("the step names the registry")
        .to_string();
    (run_body(&s), dir)
}

fn check_rules(repo: &Repo, base: &str, head: &str) -> Ran {
    let (body, dir) = rules_step();
    run(
        &body,
        &[("BASE_SHA", base), ("HEAD_SHA", head), ("RULES_DIR", &dir)],
        &repo.dir.0,
    )
}

/// A rule file, valid but for what `status` puts under its title.
fn rule_file(id: &str, status: &str) -> String {
    format!(
        "# {id} — A rule.\n\n{status}\n\n## Rule\n\nThe rule.\n\n## Evidence\n\nA decision.\n\n## Test\n\n`a_test`.\n"
    )
}

/// `(file name, its status block, whether the doctor accepts it)`: the
/// fixtures the step and `parse_rule_file` must judge alike.
const STATUS_FIXTURES: &[(&str, &str, bool)] = &[
    ("K7QF-enforced.md", "Status: Enforced", true),
    ("M3TX-proposed.md", "Status: Proposed", true),
    ("P8WZ-bold-label.md", "**Status:** Enforced", true),
    ("R4HN-bold-word.md", "**Status**: Proposed", true),
    ("T9JD-bold-value.md", "Status:   **Enforced**  ", true),
    ("V2LS-missing.md", "No status here.", false),
    ("W5CE-draft.md", "Status: Draft", false),
    ("X6GB-lowercase.md", "Status: enforced", false),
    ("Y3KM-two-words.md", "Status: Enforced now", false),
    ("Z8PR-indented.md", "  Status: Enforced", false),
    ("A4TW-fenced.md", "```\nStatus: Enforced\n```", false),
    ("B7NX-tilde-fence.md", "~~~md\nStatus: Enforced\n```", false),
    ("C2QH-unclosed-fence.md", "```\nStatus: Enforced", true),
    (
        "D9VF-first-wins.md",
        "Status: Draft\nStatus: Enforced",
        false,
    ),
    (
        "E5JR-after-fence.md",
        "```\nStatus: Draft\n```\nStatus: Proposed",
        true,
    ),
];

const CRLF_RULE: &str = "F3MZ-crlf.md";

fn crlf_rule() -> String {
    rule_file("F3MZ", "Status: Enforced").replace('\n', "\r\n")
}

fn error_names(log: &str, path: &str) -> bool {
    log.contains(&format!("::error file={path},title=Business rule status::"))
}

#[test]
fn the_rules_step_reads_the_doctors_registry_on_every_pr() {
    let (_, dir) = rules_step();
    assert_eq!(dir, spl_asdlc::method::business_rules::REGISTRY_DIR);
    let s = step(
        &workflow(".github/workflows/asdlc-pr-guards.yml"),
        "guards",
        "rules",
    );
    // Prose-only diffs included: only the release chain's own heads skip it.
    assert_eq!(
        s["if"].as_str(),
        Some("steps.skip.outputs.skipped != 'true'")
    );
    assert!(s["continue-on-error"].is_badvalue());
}

#[test]
fn a_changed_rule_file_without_a_valid_status_fails_the_guards() {
    let repo = Repo::new();
    let dir = rules_step().1;
    // Before the PR: an invalid rule nobody touches, and one the PR deletes.
    repo.write(
        &format!("{dir}/G6HT-untouched.md"),
        &rule_file("G6HT", "Status: Draft"),
    );
    repo.write(
        &format!("{dir}/H8LW-deleted.md"),
        &rule_file("H8LW", "Status: Draft"),
    );
    repo.write(
        &format!("{dir}/J2CS-modified.md"),
        &rule_file("J2CS", "Status: Enforced"),
    );
    let base = repo.commit("the registry before the PR");

    // The PR: every fixture, a modified rule that loses its status, the
    // deleted one, and files that are not the registry's rules.
    for (name, status, _) in STATUS_FIXTURES {
        let id = &name[..4];
        repo.write(&format!("{dir}/{name}"), &rule_file(id, status));
    }
    repo.write(&format!("{dir}/{CRLF_RULE}"), &crlf_rule());
    repo.write(
        &format!("{dir}/J2CS-modified.md"),
        &rule_file("J2CS", "Status: Retired"),
    );
    repo.git(&["rm", "-q", &format!("{dir}/H8LW-deleted.md")]);
    repo.write(&format!("{dir}/README.md"), "# The registry\n");
    repo.write(&format!("{dir}/.draft.md"), "# Not a rule\n");
    repo.write(&format!("{dir}/sub/K3PD-nested.md"), "# Not read\n");
    repo.write(&format!("{dir}/notes.txt"), "Status: none\n");
    let head = repo.commit("the PR");

    let r = check_rules(&repo, &base, &head);
    assert_eq!(r.code, 1, "{}", r.log);
    for (name, _, valid) in STATUS_FIXTURES {
        let path = format!("{dir}/{name}");
        assert_eq!(error_names(&r.log, &path), !valid, "{name}: {}", r.log);
    }
    assert!(
        !error_names(&r.log, &format!("{dir}/{CRLF_RULE}")),
        "{}",
        r.log
    );
    let modified = format!("{dir}/J2CS-modified.md");
    assert!(error_names(&r.log, &modified), "{}", r.log);
    assert!(r.log.contains("`Status: Retired`"), "{}", r.log);
    assert!(
        r.log.contains(&format!(
            "{dir}/V2LS-missing.md has no `Status: Proposed` or `Status: Enforced` line"
        )),
        "{}",
        r.log
    );
    for ignored in [
        "G6HT-untouched.md",
        "H8LW-deleted.md",
        "README.md",
        ".draft.md",
        "sub/K3PD-nested.md",
        "notes.txt",
    ] {
        assert!(
            !error_names(&r.log, &format!("{dir}/{ignored}")),
            "{ignored}: {}",
            r.log
        );
    }

    // Valid rules only, the untouched invalid one still there: it passes.
    let repo = Repo::new();
    repo.write(
        &format!("{dir}/G6HT-untouched.md"),
        &rule_file("G6HT", "Status: Draft"),
    );
    let base = repo.commit("the registry before the PR");
    repo.write(
        &format!("{dir}/K7QF-enforced.md"),
        &rule_file("K7QF", "Status: Enforced"),
    );
    repo.write(
        &format!("{dir}/M3TX-proposed.md"),
        &rule_file("M3TX", "**Status:** Proposed"),
    );
    repo.write("docs/guide.md", "Prose only.\n");
    let head = repo.commit("two valid rules");
    let r = check_rules(&repo, &base, &head);
    assert_eq!(r.code, 0, "{}", r.log);
    assert!(
        r.log
            .contains("2 changed business rule(s), each with a status."),
        "{}",
        r.log
    );

    // Outside a pull request, there is no diff to judge.
    let r = check_rules(&repo, "", &head);
    assert_eq!(r.code, 0, "{}", r.log);
    assert!(r.log.contains("are not checked"), "{}", r.log);
}

#[test]
fn the_rules_step_and_the_doctor_read_a_status_alike() {
    // Drift guard: the step's verdict on each fixture is the doctor's
    // (`parse_rule_file`), file by file.
    let repo = Repo::new();
    let dir = rules_step().1;
    let base = repo.git(&["rev-parse", "HEAD"]);
    let mut fixtures: Vec<(String, String)> = STATUS_FIXTURES
        .iter()
        .map(|(name, status, _)| (name.to_string(), rule_file(&name[..4], status)))
        .collect();
    fixtures.push((CRLF_RULE.to_string(), crlf_rule()));
    for (name, text) in &fixtures {
        repo.write(&format!("{dir}/{name}"), text);
    }
    let head = repo.commit("the fixtures");
    let r = check_rules(&repo, &base, &head);
    for (name, text) in &fixtures {
        let doctor = spl_asdlc::method::business_rules::parse_rule_file(name, text);
        let guard_refuses = error_names(&r.log, &format!("{dir}/{name}"));
        assert_eq!(
            guard_refuses,
            doctor.is_err(),
            "{name}: the doctor says {doctor:?}, the guard log:\n{}",
            r.log
        );
    }
    for (name, status, valid) in STATUS_FIXTURES {
        let text = rule_file(&name[..4], status);
        assert_eq!(
            spl_asdlc::method::business_rules::parse_rule_file(name, &text).is_ok(),
            *valid,
            "{name}: the fixture's expectation is the doctor's"
        );
    }
}
