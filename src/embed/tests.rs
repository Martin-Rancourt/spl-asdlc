use super::*;
use crate::github::Identity;
use crate::github::fake::FakeGitHub;
use crate::tracker::fake::FakeTracker;

#[test]
fn the_minor_line_of_a_version() {
    assert_eq!(minor_line("1.4.2"), "v1.4");
    assert_eq!(minor_line("10.0.0"), "v10.0");
    assert!(minor_line(VERSION).starts_with('v'));
}

fn github() -> FakeGitHub {
    let g = FakeGitHub::new();
    let repo = RepoSlug::parse("example-org/acme").unwrap();
    let mut s = g.state.borrow_mut();
    s.identity = Some(Identity::App {
        slug: "spl-asdlc".into(),
        app_id: "1".into(),
    });
    s.default_branch.insert(repo.clone(), "main".into());
    s.files.insert(
        (repo, "main".into(), DEFAULT_PATH.into()),
        Ok(crate::config::spec_example::repository().into()),
    );
    drop(s);
    g
}

fn overlay() -> Source {
    Source {
        label: "overlay".into(),
        text: crate::config::spec_example::instance().into(),
    }
}

#[test]
fn the_configuration_is_read_at_the_default_branch_and_merged() {
    let g = github();
    let repo = RepoSlug::parse("example-org/acme").unwrap();
    let read = read_config(&g, &repo, Some(&overlay())).unwrap();
    assert_eq!(read.config, crate::config::spec_example::config());
    assert_eq!(read.default_branch, "main");
    assert_eq!(read.source, "example-org/acme:.spl-asdlc/config.toml@main");
    // The repository's file alone lacks its identities.
    let err = read_config(&g, &repo, None).unwrap_err();
    assert!(matches!(err, ReadError::Invalid { .. }), "{err}");
    let unknown = RepoSlug::parse("example-org/none").unwrap();
    assert!(matches!(
        read_config(&g, &unknown, None).unwrap_err(),
        ReadError::Repository { .. }
    ));
    assert!(g.writes().is_empty());
}

/// A gesture refused (no release branch) says why, and writes its log
/// where the host said, as the CLI does.
#[test]
fn a_gesture_returns_what_it_did_or_why_it_was_refused() {
    let g = github();
    let repo = RepoSlug::parse("example-org/acme").unwrap();
    g.state
        .borrow_mut()
        .branches
        .insert(repo.clone(), Vec::new());
    let read = read_config(&g, &repo, Some(&overlay())).unwrap();
    let tracker = FakeTracker::default();
    let dir = std::env::temp_dir().join(format!("spl-asdlc-embed-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let log = dir.join("gestures.jsonl");
    let _ = std::fs::remove_file(&log);
    let p = Project::new(&read.config, &g, &tracker, Some(log.clone()), &|_| None).unwrap();
    let refused = p.cut(false, "a person");
    assert_eq!(refused.gesture, "release cut");
    assert!(!refused.done);
    assert!(refused.message.contains("no release branch"), "{refused:?}");
    assert!(g.writes().is_empty());

    let done = p.cut(true, "a person");
    assert!(done.done, "{done:?}");
    assert!(done.message.contains("action open"), "{done:?}");
    let line = std::fs::read_to_string(&log).unwrap();
    assert!(
        line.contains("release cut") && line.contains("a person"),
        "{line}"
    );
    let json = serde_json::to_value(&done).unwrap();
    assert_eq!(json["done"], true);
}
