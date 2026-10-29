//! `release prepare` on real histories (spec 05 §1): the range is read along
//! the graph, so a commit merged into the default branch while a cycle was
//! open is in the next release, whatever its date.

use super::*;
use crate::config::{Config, default_changelog_sections};

/// A repository in a scratch directory of its own, removed on drop.
struct Repo {
    dir: PathBuf,
}

impl Repo {
    fn new() -> Repo {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "spl-asdlc-prepare-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let r = Repo { dir };
        r.git(&["init", "-q", "-b", "main"]);
        r.git(&["config", "user.email", "test@localhost"]);
        r.git(&["config", "user.name", "test"]);
        r.git(&["config", "commit.gpgsign", "false"]);
        r.git(&["config", "tag.gpgsign", "false"]);
        r
    }

    fn git(&self, args: &[&str]) -> String {
        self.git_at(args, None)
    }

    /// `git` with a committer date, so that a history's dates can disagree
    /// with its graph.
    fn git_at(&self, args: &[&str], date: Option<&str>) -> String {
        let mut cmd = Command::new("git");
        cmd.args(args)
            .current_dir(&self.dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1");
        if let Some(d) = date {
            cmd.env("GIT_COMMITTER_DATE", d).env("GIT_AUTHOR_DATE", d);
        }
        let out = cmd.output().unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn write(&self, path: &str, text: &str) {
        let p = self.dir.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn read(&self, path: &str) -> String {
        std::fs::read_to_string(self.dir.join(path)).unwrap_or_default()
    }

    fn commit(&self, message: &str, date: &str) -> String {
        self.git(&["add", "-A"]);
        self.git_at(
            &["commit", "-q", "--allow-empty", "-m", message],
            Some(date),
        );
        self.git(&["rev-parse", "HEAD"])
    }

    fn options(&self) -> Options {
        Options {
            dir: self.dir.clone(),
            date: Some("2026-10-12".into()),
            repository: Some("example-org/acme".into()),
            ..Options::default()
        }
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn preparation(extra: &str) -> Preparation {
    Config::preparation(&format!(
        "schema = 1\n[product]\nname = \"acme\"\n[release]\nangles = []\n{extra}"
    ))
    .unwrap_or_else(|e| panic!("{e}"))
}

const MANIFEST: &str = "[package]\nname = \"acme\"\nversion = \"0.1.0\"\nedition = \"2024\"\n";
const LOCK: &str = "version = 4\n\n[[package]]\nname = \"acme\"\nversion = \"0.1.0\"\n";

/// The cycle of `acme-v0.1.0`, then the next one, as the git graph holds
/// them: the release branch cut from `main`, its version commit tagged;
/// a fix merged into `main` while that cycle was open, committed before the
/// tag; the merge-back; then a feature.
fn released_with_a_merge_back() -> Repo {
    let r = Repo::new();
    r.write("Cargo.toml", MANIFEST);
    r.write("Cargo.lock", LOCK);
    r.commit("feat: the first feature (#1)", "2026-10-01T10:00:00Z");
    r.git(&["checkout", "-q", "-b", "release/2026-10-01-12-00-utc"]);
    r.commit("chore(release): v0.1.0", "2026-10-01T12:00:00Z");
    r.git(&["checkout", "-q", "main"]);
    r.write("src/fix.rs", "pub fn f() {}\n");
    r.commit("fix: merged during the cycle (#7)", "2026-10-01T11:00:00Z");
    r.git(&["checkout", "-q", "release/2026-10-01-12-00-utc"]);
    r.git_at(
        &["tag", "-a", "acme-v0.1.0", "-m", "Release acme-v0.1.0"],
        Some("2026-10-02T09:00:00Z"),
    );
    r.write(".spl-asdlc/qa/acme-v0.1.0-rc.1/security.json", "{}\n");
    r.commit(
        "docs(qa): the security report of rc.1",
        "2026-10-02T08:00:00Z",
    );
    r.git(&["checkout", "-q", "main"]);
    r.git_at(
        &[
            "merge",
            "-q",
            "--no-ff",
            "-m",
            "Merge release/2026-10-01-12-00-utc back into main",
            "release/2026-10-01-12-00-utc",
        ],
        Some("2026-10-02T10:00:00Z"),
    );
    r.write("src/feature.rs", "pub fn g() {}\n");
    r.commit(
        "feat(cli): a feature after the release (#8)",
        "2026-10-03T10:00:00Z",
    );
    r
}

#[test]
fn a_fix_merged_while_a_cycle_was_open_is_in_the_next_release() {
    let r = released_with_a_merge_back();
    let notes = r.dir.join("notes.md");
    let p = run(
        &preparation(""),
        &Options {
            notes: Some(notes.clone()),
            ..r.options()
        },
    )
    .unwrap();
    assert_eq!(p.previous_tag.as_deref(), Some("acme-v0.1.0"));
    assert_eq!(
        p.version.to_string(),
        "0.2.0",
        "a feat before 1.0: the minor"
    );
    assert_eq!(p.tag, "acme-v0.2.0");
    assert!(
        p.notes.starts_with(
            "## [0.2.0](https://github.com/example-org/acme/compare/acme-v0.1.0...acme-v0.2.0) (2026-10-12)\n"
        ),
        "{}",
        p.notes
    );
    assert!(
        p.notes.contains("* merged during the cycle ([#7]("),
        "dated before the release, merged after it: in this release\n{}",
        p.notes
    );
    assert!(
        p.notes
            .contains("* **cli:** a feature after the release ([#8](")
    );
    assert!(p.notes.contains("* the security report of rc.1 ("));
    assert!(!p.notes.contains("the first feature"), "already released");
    assert!(
        !p.notes.contains("v0.1.0 ("),
        "the release commit is the tag's"
    );
    assert!(!p.notes.contains("Merge"), "merge commits are not listed");
    assert_eq!(std::fs::read_to_string(&notes).unwrap(), p.notes);
    assert_eq!(
        p.files,
        vec!["Cargo.toml", "Cargo.lock", CHANGELOG],
        "the files the workflow commits"
    );
    assert!(r.read("Cargo.toml").contains("version = \"0.2.0\""));
    assert!(
        r.read("Cargo.lock")
            .contains("name = \"acme\"\nversion = \"0.2.0\"")
    );
    assert!(r.read("Cargo.lock").starts_with("version = 4\n"));
    assert_eq!(r.read(CHANGELOG), format!("# Changelog\n\n{}", p.notes));
}

#[test]
fn a_changelog_folder_gets_one_file_per_version_written_by_its_release_only() {
    let r = released_with_a_merge_back();
    r.write("changelog/0.1.0.md", "## 0.1.0\n");
    r.commit("docs: the notes of 0.1.0", "2026-10-03T11:00:00Z");
    let prep = preparation("changelog_dir = \"changelog\"\n");
    let p = run(&prep, &r.options()).unwrap();
    assert_eq!(
        p.files,
        vec!["Cargo.toml", "Cargo.lock", CHANGELOG, "changelog/0.2.0.md"]
    );
    assert_eq!(r.read("changelog/0.2.0.md"), p.notes);
    assert_eq!(
        r.read("changelog/0.1.0.md"),
        "## 0.1.0\n",
        "an earlier version is left alone"
    );
    // A version whose file exists is refused, and nothing is written.
    let r = released_with_a_merge_back();
    r.write("notes/0.2.0.md", "kept\n");
    let before = r.read("Cargo.toml");
    let e = run(&preparation("changelog_dir = \"notes\"\n"), &r.options()).unwrap_err();
    assert!(e.contains("notes/0.2.0.md already exists"), "{e}");
    assert_eq!(r.read("Cargo.toml"), before);
    assert_eq!(r.read("notes/0.2.0.md"), "kept\n");
    // Without the key, no folder.
    let r = released_with_a_merge_back();
    let p = run(&preparation(""), &r.options()).unwrap();
    assert!(!p.files.iter().any(|f| f.ends_with("0.2.0.md")));
    // A folder outside the repository is refused by the configuration.
    let e = Config::preparation(
        "schema = 1\n[product]\nname = \"acme\"\n[release]\nangles = []\nchangelog_dir = \"../out\"\n",
    )
    .unwrap_err();
    assert!(e.to_string().contains("release.changelog_dir"), "{e}");
}

#[test]
fn a_first_release_is_measured_from_the_root_and_bumped_from_0_0_0() {
    let r = Repo::new();
    r.commit("chore: the repository", "2026-10-01T10:00:00Z");
    r.commit("fix: a first fix", "2026-10-01T11:00:00Z");
    let p = run(&preparation(""), &r.options()).unwrap();
    assert_eq!(p.previous_tag, None);
    assert_eq!(p.version.to_string(), "0.0.1");
    assert_eq!(p.commits, 2);
    assert!(
        p.notes.starts_with("## 0.0.1 (2026-10-12)\n"),
        "{}",
        p.notes
    );
    assert!(p.notes.contains("* a first fix"));
    assert!(p.notes.contains("* the repository"));
    assert_eq!(
        p.files,
        vec![CHANGELOG],
        "no Rust manifest, no version file"
    );
}

#[test]
fn a_breaking_change_after_1_0_bumps_the_major() {
    let r = Repo::new();
    r.commit("feat: one", "2026-10-01T10:00:00Z");
    r.git(&["tag", "acme-v1.4.2"]);
    r.git(&["tag", "acme-v1.4.2-rc.1"]);
    r.git(&["tag", "other-v9.0.0"]);
    r.commit("feat(api)!: drop v1 (#9)", "2026-10-02T10:00:00Z");
    let p = run(&preparation(""), &r.options()).unwrap();
    assert_eq!(p.previous_tag.as_deref(), Some("acme-v1.4.2"));
    assert_eq!(p.version.to_string(), "2.0.0");
    assert!(p.notes.contains("### ⚠ BREAKING CHANGES"), "{}", p.notes);
}

#[test]
fn a_forced_version_must_be_above_the_last_release() {
    let r = released_with_a_merge_back();
    let p = run(
        &preparation(""),
        &Options {
            version: Some("1.0.0".into()),
            ..r.options()
        },
    )
    .unwrap();
    assert_eq!(p.version.to_string(), "1.0.0");
    assert_eq!(p.tag, "acme-v1.0.0");
    let r = released_with_a_merge_back();
    let before = r.read("Cargo.toml");
    let e = run(
        &preparation(""),
        &Options {
            version: Some("0.1.0".into()),
            ..r.options()
        },
    )
    .unwrap_err();
    assert!(e.contains("not above the last release, acme-v0.1.0"), "{e}");
    assert_eq!(r.read("Cargo.toml"), before, "a refusal writes nothing");
    let e = run(
        &preparation(""),
        &Options {
            version: Some("two".into()),
            ..r.options()
        },
    )
    .unwrap_err();
    assert!(e.contains("is not X.Y.Z"), "{e}");
}

#[test]
fn nothing_to_release_is_refused_and_writes_nothing() {
    let r = Repo::new();
    r.commit("feat: one", "2026-10-01T10:00:00Z");
    r.git(&["tag", "acme-v0.1.0"]);
    let e = run(&preparation(""), &r.options()).unwrap_err();
    assert!(e.contains("no commit since acme-v0.1.0"), "{e}");
    r.commit("wip: not a listed type", "2026-10-02T10:00:00Z");
    r.commit("an update in free text", "2026-10-02T11:00:00Z");
    let e = run(&preparation(""), &r.options()).unwrap_err();
    assert!(e.contains("nothing to release since acme-v0.1.0"), "{e}");
    assert!(!r.dir.join(CHANGELOG).exists());
    // A version forced is the operator's decision.
    let p = run(
        &preparation(""),
        &Options {
            version: Some("0.1.1".into()),
            ..r.options()
        },
    )
    .unwrap();
    assert_eq!(p.version.to_string(), "0.1.1");
}

#[test]
fn a_released_version_is_never_prepared_again() {
    let r = Repo::new();
    r.commit("feat: one", "2026-10-01T10:00:00Z");
    r.git(&["tag", "acme-v0.1.0"]);
    // A release of the next version exists off this branch's history.
    r.git(&["checkout", "-q", "-b", "side"]);
    r.commit("fix: elsewhere", "2026-10-02T09:00:00Z");
    r.git(&["tag", "acme-v0.1.1"]);
    r.git(&["checkout", "-q", "main"]);
    r.commit("fix: two", "2026-10-02T10:00:00Z");
    let e = run(
        &preparation(""),
        &Options {
            version: Some("0.1.1".into()),
            ..r.options()
        },
    )
    .unwrap_err();
    assert!(e.contains("acme-v0.1.1 already exists"), "{e}");
}

#[test]
fn the_configured_version_files_take_the_version_and_a_dry_run_writes_none() {
    let r = Repo::new();
    r.write("web/package.json", "{\n  \"version\": \"0.0.0\"\n}\n");
    r.write("VERSION", "acme 0.0.0\n");
    r.commit("feat: one", "2026-10-01T10:00:00Z");
    let prep = preparation(
        "[[release.version_files]]\npath = \"web/package.json\"\npattern = '\"version\": \"{version}\"'\n\
         [[release.version_files]]\npath = \"VERSION\"\npattern = \"acme {version}\"\n",
    );
    let notes = r.dir.join("notes.md");
    let dry = run(
        &prep,
        &Options {
            dry_run: true,
            notes: Some(notes.clone()),
            ..r.options()
        },
    )
    .unwrap();
    assert_eq!(dry.files, vec!["web/package.json", "VERSION", CHANGELOG]);
    assert!(
        r.read("VERSION").contains("0.0.0"),
        "a dry run writes no file of the tree"
    );
    assert!(!r.dir.join(CHANGELOG).exists());
    assert_eq!(
        std::fs::read_to_string(&notes).unwrap(),
        dry.notes,
        "the notes are the run's output, even dry"
    );
    let p = run(&prep, &r.options()).unwrap();
    assert_eq!(p, dry);
    assert_eq!(
        r.read("web/package.json"),
        "{\n  \"version\": \"0.1.0\"\n}\n"
    );
    assert_eq!(r.read("VERSION"), "acme 0.1.0\n");

    let missing = preparation(
        "[[release.version_files]]\npath = \"nowhere.txt\"\npattern = \"v{version}\"\n",
    );
    let e = run(&missing, &r.options()).unwrap_err();
    assert!(
        e.contains("nowhere.txt: the version file does not exist"),
        "{e}"
    );
}

#[test]
fn the_links_need_a_repository() {
    let r = Repo::new();
    r.commit("feat: one", "2026-10-01T10:00:00Z");
    let e = run(
        &preparation(""),
        &Options {
            repository: None,
            ..r.options()
        },
    )
    .unwrap_err();
    assert!(e.contains("--repository"), "{e}");
    let with_file = Config::preparation(
        "schema = 1\n[product]\nname = \"acme\"\nrepository = \"example-org/acme\"\n[release]\nangles = []\n",
    )
    .unwrap();
    assert_eq!(with_file.changelog_sections, default_changelog_sections());
    run(
        &with_file,
        &Options {
            repository: None,
            ..r.options()
        },
    )
    .expect("product.repository serves when the file carries it");
}
