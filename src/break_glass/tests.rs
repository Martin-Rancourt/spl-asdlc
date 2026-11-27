//! `release patch --break-glass` on real histories (ASDLC-KEUY): a clone of
//! a bare remote, the release in production tagged, then the default branch
//! moving on with work not yet released and the one fix to ship.

use super::*;
use crate::config::Config;

/// A bare remote and its clone, in a scratch directory removed on drop.
struct Repo {
    root: PathBuf,
    dir: PathBuf,
}

impl Repo {
    fn new() -> Repo {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "spl-asdlc-break-glass-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let r = Repo {
            dir: root.join("work"),
            root,
        };
        r.git_in(
            &r.root,
            &["init", "-q", "--bare", "-b", "main", "origin.git"],
        );
        r.git_in(&r.root, &["clone", "-q", "origin.git", "work"]);
        for (k, v) in [
            ("user.email", "test@localhost"),
            ("user.name", "test"),
            ("commit.gpgsign", "false"),
            ("tag.gpgsign", "false"),
        ] {
            r.git(&["config", k, v]);
        }
        r
    }

    fn git_in(&self, dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn git(&self, args: &[&str]) -> String {
        self.git_in(&self.dir, args)
    }

    fn succeeds(&self, args: &[&str]) -> bool {
        Command::new("git")
            .args(args)
            .current_dir(&self.dir)
            .output()
            .unwrap()
            .status
            .success()
    }

    fn write(&self, path: &str, text: &str) {
        let p = self.dir.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn read(&self, path: &str) -> String {
        std::fs::read_to_string(self.dir.join(path)).unwrap_or_default()
    }

    fn commit(&self, message: &str) -> String {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "--allow-empty", "-m", message]);
        self.git(&["rev-parse", "HEAD"])
    }

    fn options(&self, commits: &[&str], tests: &[&str]) -> Options {
        Options {
            dir: self.dir.clone(),
            commits: commits.iter().map(ToString::to_string).collect(),
            tests: tests.iter().map(ToString::to_string).collect(),
            remote: "origin".into(),
            repository: Some("example-org/acme".into()),
            date: Some("2026-11-26".into()),
            stamp: Some("2026-11-26-23-51".into()),
            ..Options::default()
        }
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

const CONFIG: &str = "schema = 1\n[product]\nname = \"acme\"\n[release]\nangles = []\nrelease_tag = \"v{version}\"\ncandidate_tag = \"v{version}-rc.{rc}\"\n";

fn preparation() -> Preparation {
    Config::preparation(CONFIG).unwrap_or_else(|e| panic!("{e}"))
}

/// v0.28.0 in production; then, on `main`, a feature not released yet and
/// the one fix the runner needs. Returns `(feature, fix)`.
fn v0_28_0_then_a_feature_and_a_fix() -> (Repo, String, String) {
    let r = Repo::new();
    r.write(
        "Cargo.toml",
        "[package]\nname = \"acme\"\nversion = \"0.28.0\"\nedition = \"2024\"\n",
    );
    r.write(
        "src/refinement.rs",
        "fn language_line(t: &str) -> &str { &t[..2] }\n",
    );
    r.commit("feat: the refinement reads the release note (#540)");
    r.commit("chore(release): v0.28.0");
    r.git(&["tag", "-a", "v0.28.0", "-m", "Release v0.28.0"]);
    r.write("src/feature.rs", "pub fn unreleased() {}\n");
    let feature = r.commit("feat: a feature not released yet (#558)");
    r.write(
        "src/refinement.rs",
        "fn language_line(t: &str) -> String { t.chars().take(2).collect() }\n",
    );
    let fix = r.commit("fix: the runner reads a release note that starts with an accent (#560)");
    r.git(&["push", "-q", "origin", "main", "--tags"]);
    (r, feature, fix)
}

#[test]
fn v0_28_1_replayed_a_patch_branch_ready_tests_green_and_no_tag() {
    let (r, _feature, fix) = v0_28_0_then_a_feature_and_a_fix();
    let ready = run(
        &preparation(),
        &r.options(
            &[&fix],
            &["grep -q chars src/refinement.rs && ! test -e src/feature.rs"],
        ),
    )
    .unwrap_or_else(|e| panic!("{e}"));

    assert_eq!(ready.from, "v0.28.0");
    assert_eq!(ready.tag, "v0.28.1");
    assert_eq!(ready.branch, "release/2026-11-26-23-51-utc");
    assert_eq!(ready.carried.len(), 1);
    assert_eq!(ready.carried[0].0, fix);
    // The branch: v0.28.0, the fix and the release commit, nothing else.
    assert_eq!(r.git(&["rev-parse", "--abbrev-ref", "HEAD"]), ready.branch);
    assert_eq!(r.git(&["rev-parse", "HEAD"]), ready.head);
    let log = r.git(&["log", "--format=%s", "v0.28.0..HEAD"]);
    assert_eq!(
        log.lines().collect::<Vec<_>>(),
        [
            "chore(release): v0.28.1",
            "fix: the runner reads a release note that starts with an accent (#560)"
        ]
    );
    assert!(
        r.git(&["log", "-1", "--format=%b", "HEAD~1"])
            .contains(&format!("(cherry picked from commit {fix})"))
    );
    assert!(!r.dir.join("src/feature.rs").exists());
    assert!(r.read("Cargo.toml").contains("version = \"0.28.1\""));
    let changelog = r.read("CHANGELOG.md");
    assert!(changelog.contains("0.28.1"), "{changelog}");
    assert!(changelog.contains("starts with an accent"), "{changelog}");
    assert!(!changelog.contains("not released yet"), "{changelog}");
    assert!(
        std::fs::read_to_string(&ready.notes)
            .unwrap()
            .contains("starts with an accent")
    );
    assert!(r.git(&["status", "--porcelain"]).is_empty());
    // Nothing tagged, nothing pushed: the go is a person's.
    assert!(!r.succeeds(&["rev-parse", "--verify", "--quiet", "refs/tags/v0.28.1"]));
    let remote = r.git(&["ls-remote", "origin"]);
    assert!(!remote.contains("v0.28.1"), "{remote}");
    assert!(!remote.contains(&ready.branch), "{remote}");
    // The exact commands of the go: the branch, the tag on the release
    // commit (a patch, for the publication), the release.
    assert_eq!(ready.go[0], format!("git push origin {}", ready.branch));
    assert!(ready.go[1].starts_with(&format!("git tag -a v0.28.1 {} ", ready.head)));
    assert!(ready.go[1].contains("Release-Patch-From: v0.28.0"));
    assert_eq!(ready.go[2], "git push origin v0.28.1");
    assert!(ready.go[3].starts_with("gh release create v0.28.1 --repo example-org/acme"));
}

#[test]
fn a_commit_not_merged_into_the_default_branch_is_refused_and_nothing_changes() {
    let (r, _feature, fix) = v0_28_0_then_a_feature_and_a_fix();
    r.git(&["checkout", "-q", "-b", "fix/unmerged"]);
    r.write("src/side.rs", "pub fn side() {}\n");
    let unmerged = r.commit("fix: never merged (#561)");
    r.git(&["push", "-q", "origin", "fix/unmerged"]);
    let e = run(&preparation(), &r.options(&[&fix, &unmerged], &["true"])).unwrap_err();
    assert!(e.contains("is not on origin/main"), "{e}");
    assert_eq!(
        r.git(&["rev-parse", "--abbrev-ref", "HEAD"]),
        "fix/unmerged"
    );
    assert!(!r.succeeds(&[
        "rev-parse",
        "--verify",
        "--quiet",
        "refs/heads/release/2026-11-26-23-51-utc"
    ]));
}

#[test]
fn a_commit_only_local_is_not_merged_and_is_refused() {
    let (r, _feature, fix) = v0_28_0_then_a_feature_and_a_fix();
    r.write("src/local.rs", "pub fn local() {}\n");
    let local = r.commit("fix: committed on main, never pushed");
    let e = run(&preparation(), &r.options(&[&fix, &local], &["true"])).unwrap_err();
    assert!(e.contains("is not on origin/main"), "{e}");
}

#[test]
fn a_commit_already_in_production_is_refused() {
    let (r, _feature, _fix) = v0_28_0_then_a_feature_and_a_fix();
    let released = r.git(&["rev-parse", "v0.28.0^{commit}"]);
    let e = run(&preparation(), &r.options(&[&released], &["true"])).unwrap_err();
    assert!(e.contains("already in production (v0.28.0)"), "{e}");
}

#[test]
fn a_commit_that_does_not_apply_removes_the_branch() {
    let (r, _feature, _fix) = v0_28_0_then_a_feature_and_a_fix();
    // A change on top of the feature: it does not apply on v0.28.0.
    r.write("src/feature.rs", "pub fn unreleased() { todo!() }\n");
    let on_feature = r.commit("fix: the unreleased feature (#562)");
    r.git(&["push", "-q", "origin", "main"]);
    let e = run(&preparation(), &r.options(&[&on_feature], &["true"])).unwrap_err();
    assert!(e.contains("does not apply on v0.28.0"), "{e}");
    assert!(e.contains("removed, nothing pushed"), "{e}");
    assert_eq!(r.git(&["rev-parse", "--abbrev-ref", "HEAD"]), "main");
    assert!(r.git(&["status", "--porcelain"]).is_empty());
    assert!(!r.succeeds(&[
        "rev-parse",
        "--verify",
        "--quiet",
        "refs/heads/release/2026-11-26-23-51-utc"
    ]));
}

#[test]
fn red_tests_leave_the_branch_for_inspection_and_no_go() {
    let (r, _feature, fix) = v0_28_0_then_a_feature_and_a_fix();
    let e = run(&preparation(), &r.options(&[&fix], &["true", "exit 3"])).unwrap_err();
    assert!(e.contains("the tests failed (`exit 3`)"), "{e}");
    assert!(e.contains("nothing pushed nor tagged"), "{e}");
    assert!(!r.succeeds(&["rev-parse", "--verify", "--quiet", "refs/tags/v0.28.1"]));
}

#[test]
fn a_patch_names_its_commits_and_its_tests() {
    let (r, _feature, fix) = v0_28_0_then_a_feature_and_a_fix();
    let e = run(&preparation(), &r.options(&[], &["true"])).unwrap_err();
    assert!(e.contains("--commit"), "{e}");
    let e = run(&preparation(), &r.options(&[&fix], &[])).unwrap_err();
    assert!(e.contains("--test"), "{e}");
}

#[test]
fn several_commits_go_in_the_order_the_default_branch_took_them() {
    let (r, _feature, fix) = v0_28_0_then_a_feature_and_a_fix();
    r.write("src/other.rs", "pub fn other() {}\n");
    let later = r.commit("fix: a second fix (#563)");
    r.git(&["push", "-q", "origin", "main"]);
    let ready = run(&preparation(), &r.options(&[&later, &fix], &["true"]))
        .unwrap_or_else(|e| panic!("{e}"));
    let order: Vec<_> = ready.carried.iter().map(|(s, _)| s.clone()).collect();
    assert_eq!(order, [fix, later]);
}
