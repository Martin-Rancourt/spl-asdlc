use super::*;
use crate::config::{Config, LinkFallback, Source};
use crate::github::fake::FakeGitHub;
use crate::tracker::fake::FakeTracker;

fn answers(tracker: TrackerKind) -> Answers {
    Answers {
        product: "acme".into(),
        repository: "example-org/acme".into(),
        tracker,
        review_angles: Vec::new(),
        release_angles: Vec::new(),
        test_command: "cargo test --locked".into(),
        app_slug: "example-app".into(),
        method_repository: Some("example-org/spl-asdlc".into()),
    }
}

/// Test identities, as a host's overlay carries them.
fn overlay(coordinators: bool) -> Source {
    let mut text = String::from(
        "schema = 1\n\n[product]\nrepository = \"example-org/acme\"\n\n[owner]\ntracker_user = \"owner-handle\"\n\n[tracker]\nteam = \"ABC\"\nservice_identity = \"spl-asdlc\"\nteam_keys = [\"ABC\"]\n",
    );
    if coordinators {
        text.push_str(
            "\n[review]\ncoordinators = [\"coordinator-login\"]\n\n[release]\ncoordinators = [\"coordinator-login\"]\n",
        );
    }
    Source {
        label: "the host's overlay".into(),
        text,
    }
}

fn file<'a>(pr: &'a SetupPr, path: &str) -> &'a str {
    &pr.files
        .iter()
        .find(|f| f.path == path)
        .unwrap_or_else(|| panic!("no {path}"))
        .content
}

fn merged(pr: &SetupPr, coordinators: bool) -> Config {
    let repository = Source {
        label: DEFAULT_PATH.into(),
        text: file(pr, DEFAULT_PATH).into(),
    };
    Config::parse_with(&repository, Some(&overlay(coordinators)))
        .unwrap_or_else(|e| panic!("{e}\n{}", repository.text))
}

#[test]
fn the_light_configuration_by_default_with_no_identity() {
    let pr = files(&answers(TrackerKind::Linear)).unwrap();
    let c = merged(&pr, false);
    assert!(!c.review.angle_possible(), "no PR review");
    assert!(c.release.angles.is_empty(), "no QA angle");
    assert_eq!(c.tracker.link_fallback, LinkFallback::None);
    let text = file(&pr, DEFAULT_PATH);
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        for key in ["repository", "team", "service_identity", "project", "coordinators", "[owner]"] {
            assert!(!line.starts_with(key), "{line}");
        }
    }
    // The file alone is not a configuration: its identities are the overlay's.
    assert!(Config::parse(text).is_err());
}

#[test]
fn a_livela_project_links_a_pr_by_the_url_in_its_body() {
    let pr = files(&answers(TrackerKind::Livela)).unwrap();
    let c = merged(&pr, false);
    assert_eq!(c.tracker.link_fallback, LinkFallback::BodyUrl);
}

#[test]
fn each_layer_is_added_only_when_asked() {
    let mut a = answers(TrackerKind::Livela);
    a.review_angles = vec![
        ReviewAngle {
            key: "maintainability".into(),
            paths: vec![],
        },
        ReviewAngle {
            key: "security".into(),
            paths: vec![".github/workflows/**".into()],
        },
    ];
    let pr = files(&a).unwrap();
    let c = merged(&pr, true);
    assert!(c.review.angle_possible());
    assert_eq!(c.review.angles[1].paths, [".github/workflows/**"]);
    assert!(c.release.angles.is_empty());
    assert!(file(&pr, ".github/workflows/release-publish.yml").contains("qa-angles: '[]'"));

    a.review_angles.clear();
    a.release_angles = vec!["security".into(), "product".into()];
    let pr = files(&a).unwrap();
    let c = merged(&pr, true);
    assert!(!c.review.angle_possible());
    let keys: Vec<_> = c.release.angles.iter().map(|a| a.key.as_str()).collect();
    assert_eq!(keys, ["security", "product"]);
    assert!(
        file(&pr, ".github/workflows/release-publish.yml")
            .contains("qa-angles: '[\"security\",\"product\"]'")
    );
}

#[test]
fn the_callers_name_the_embedded_minor_line_and_the_cut_runs_its_binary() {
    let pr = files(&answers(TrackerKind::Linear)).unwrap();
    let line = minor_line(VERSION);
    let workflows: Vec<_> = pr
        .files
        .iter()
        .filter(|f| f.path.starts_with(".github/workflows/"))
        .collect();
    assert_eq!(workflows.len(), 6);
    for f in &workflows {
        assert!(!f.content.contains("<owner>"), "{}", f.path);
        for uses in f
            .content
            .lines()
            .filter(|l| l.contains("uses: example-org/spl-asdlc/"))
        {
            assert!(uses.ends_with(&format!("@{line}")), "{}: {uses}", f.path);
        }
        assert!(
            !f.content.contains("run: make") && !f.content.contains("\"make "),
            "{}",
            f.path
        );
    }
    let cut = file(&pr, ".github/workflows/release-cut.yml");
    assert!(cut.contains(&format!("spl-asdlc-version: {line}\n")), "{cut}");
    assert!(cut.contains("spl-asdlc-repository: example-org/spl-asdlc\n"));
    let gate = file(&pr, ".github/workflows/release-gate.yml");
    assert!(!gate.contains("preflight"), "{gate}");
    assert!(
        gate.contains("\"run\": \"cargo test --locked 2>&1 | tee"),
        "{gate}"
    );
    assert_eq!(file(&pr, ".spl-asdlc/process.md"), PROCESS);
    assert!(file(&pr, "AGENTS.md").starts_with("# Working on acme\n"));
    assert_eq!(pr.branch, "spl-asdlc/setup");
    assert!(pr.title.starts_with("chore: "), "{}", pr.title);
    assert!(pr.body.contains(&line));
}

#[test]
fn the_default_method_repository_is_this_crates() {
    let mut a = answers(TrackerKind::Linear);
    a.method_repository = None;
    let pr = files(&a).unwrap();
    let method = method_repository();
    assert!(RepoSlug::parse(&method).is_some(), "{method}");
    assert!(
        file(&pr, ".github/workflows/pr.yml").contains(&format!("{method}/.github/workflows/"))
    );
}

#[test]
fn answers_that_make_no_setup_are_refused() {
    let bad = |f: fn(&mut Answers)| {
        let mut a = answers(TrackerKind::Linear);
        f(&mut a);
        files(&a).unwrap_err()
    };
    assert!(bad(|a| a.product = "Acme".into()).contains("product"));
    assert!(bad(|a| a.repository = "acme".into()).contains("repository"));
    assert!(bad(|a| a.release_angles = vec!["Security".into()]).contains("angle"));
    assert!(bad(|a| a.test_command = "a\nb".into()).contains("test command"));
    assert!(bad(|a| a.app_slug = "an app".into()).contains("slug"));
}

/// The files on a test repository: `doctor`'s workflow-caller checks are
/// ok, the embedded version's among them.
#[test]
fn doctor_finds_the_generated_callers_on_a_test_repository() {
    let pr = files(&answers(TrackerKind::Linear)).unwrap();
    let c = merged(&pr, false);
    let g = FakeGitHub::new();
    let repo = RepoSlug::parse("example-org/acme").unwrap();
    {
        let mut s = g.state.borrow_mut();
        s.default_branch.insert(repo.clone(), "main".into());
        for f in &pr.files {
            s.files.insert(
                (repo.clone(), "main".into(), f.path.clone()),
                Ok(f.content.clone()),
            );
        }
    }
    g.method_texts(
        &RepoSlug::parse("example-org/spl-asdlc").unwrap(),
        &minor_line(VERSION),
    );
    let checks = crate::doctor::run_embedded(
        &c,
        Some(&overlay(false)),
        &g,
        &FakeTracker::default(),
        "test",
        Some(VERSION),
    );
    for name in [
        DEFAULT_PATH,
        "method checks",
        "method version",
        "method texts",
        "embedded version",
        "workflow release-cut.yml",
        "workflow release-gate.yml",
        "workflow release-gate-auto-rerun.yml",
        "workflow release-publish.yml",
    ] {
        let check = checks
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("no {name} in {checks:?}"));
        assert_eq!(check.verdict, crate::doctor::Verdict::Ok, "{check}");
    }
    assert!(
        checks
            .iter()
            .filter(|c| c.name.starts_with("caller "))
            .all(|c| c.verdict == crate::doctor::Verdict::Ok)
    );
}
