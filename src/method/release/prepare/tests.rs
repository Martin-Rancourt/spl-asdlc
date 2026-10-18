//! The version, the CHANGELOG section and the version files of
//! `release prepare` (spec 05 §1), as pure functions.

use super::*;
use crate::config::default_changelog_sections;

fn v(s: &str) -> Version {
    Version::parse(s).unwrap()
}

fn commit(sha: &str, subject: &str, body: &str) -> Commit {
    Commit {
        sha: sha.into(),
        subject: subject.into(),
        body: body.into(),
    }
}

fn bump_of(subjects: &[(&str, &str)]) -> Option<Bump> {
    let commits: Vec<Commit> = subjects
        .iter()
        .enumerate()
        .map(|(i, (s, b))| commit(&format!("{i:040}"), s, b))
        .collect();
    bump(&commits, &default_changelog_sections())
}

#[test]
fn a_conventional_commit_is_read_with_its_scope_and_breaking_marks() {
    let c = parse("feat(config)!: drop the old key (#12)", "").unwrap();
    assert_eq!(c.kind, "feat");
    assert_eq!(c.scope.as_deref(), Some("config"));
    assert_eq!(c.description, "drop the old key (#12)");
    assert!(c.breaking);
    assert_eq!(c.breaking_note, None);
    let c = parse(
        "fix: a fix",
        "Some text.\n\nBREAKING CHANGE: the flag is gone,\nuse the other one.\n\nRefs: x",
    )
    .unwrap();
    assert!(c.breaking);
    assert_eq!(
        c.breaking_note.as_deref(),
        Some("the flag is gone, use the other one.")
    );
    assert!(parse("fix: x", "BREAKING-CHANGE: also").unwrap().breaking);
    assert!(
        !parse("fix: x", "not BREAKING CHANGE: in prose")
            .unwrap()
            .breaking
    );
    assert_eq!(parse("Merge pull request #3 from x/y", ""), None);
    assert_eq!(parse("feat:no space", ""), None);
    assert_eq!(parse("update things", ""), None);
}

#[test]
fn the_bump_is_the_strongest_commit_of_the_range() {
    assert_eq!(
        bump_of(&[("fix: a", ""), ("docs: b", "")]),
        Some(Bump::Patch)
    );
    assert_eq!(
        bump_of(&[("fix: a", ""), ("feat: b", "")]),
        Some(Bump::Minor)
    );
    assert_eq!(
        bump_of(&[("feat: a", ""), ("fix!: b", "")]),
        Some(Bump::Major)
    );
    assert_eq!(
        bump_of(&[("chore: a", "BREAKING CHANGE: gone")]),
        Some(Bump::Major)
    );
    assert_eq!(
        bump_of(&[("chore: a", "")]),
        Some(Bump::Patch),
        "a listed type"
    );
    assert_eq!(
        bump_of(&[("wip: a", ""), ("Merge branch x", ""), ("free text", "")]),
        None,
        "nothing the CHANGELOG lists is nothing to release"
    );
}

#[test]
fn before_1_0_a_feat_or_a_breaking_change_bumps_the_minor_and_the_rest_the_patch() {
    let p = Some(v("0.3.4"));
    assert_eq!(next_version(p, Bump::Major), v("0.4.0"));
    assert_eq!(next_version(p, Bump::Minor), v("0.4.0"));
    assert_eq!(next_version(p, Bump::Patch), v("0.3.5"));
}

#[test]
fn from_1_0_the_bump_is_semantic_versioning() {
    let p = Some(v("1.4.2"));
    assert_eq!(next_version(p, Bump::Major), v("2.0.0"));
    assert_eq!(next_version(p, Bump::Minor), v("1.5.0"));
    assert_eq!(next_version(p, Bump::Patch), v("1.4.3"));
}

#[test]
fn a_first_release_is_bumped_from_0_0_0() {
    assert_eq!(next_version(None, Bump::Minor), v("0.1.0"));
    assert_eq!(next_version(None, Bump::Major), v("0.1.0"));
    assert_eq!(next_version(None, Bump::Patch), v("0.0.1"));
}

const REPO: &str = "https://github.com/example-org/acme";

/// The format of release-please's CHANGELOG, as this repository's own
/// CHANGELOG.md shows it: so the history stays one file.
#[test]
fn the_section_keeps_release_pleases_format_with_pr_and_commit_links() {
    let sha = |c: char| c.to_string().repeat(40);
    let commits = vec![
        commit(&sha('a'), "fix: trial fixes, in one PR (#50)", ""),
        commit(&sha('b'), "feat(config): the light setup (#54)", ""),
        commit(&sha('c'), "feat(ci): a direct commit", ""),
        commit(&sha('d'), "docs(setup): a template (#55)", ""),
        commit(&sha('e'), "Merge pull request #53 from x/release", ""),
        commit(&sha('f'), "wip: never listed", ""),
    ];
    let text = section(
        v("0.3.0"),
        Some("v0.2.0"),
        "v0.3.0",
        "2026-10-11",
        &commits,
        &default_changelog_sections(),
        REPO,
    );
    assert_eq!(
        text,
        format!(
            "## [0.3.0]({REPO}/compare/v0.2.0...v0.3.0) (2026-10-11)\n\
             \n\
             \n\
             ### Features\n\
             \n\
             * **ci:** a direct commit ([ccccccc]({REPO}/commit/{c}))\n\
             * **config:** the light setup ([#54]({REPO}/issues/54)) ([bbbbbbb]({REPO}/commit/{b}))\n\
             \n\
             \n\
             ### Bug Fixes\n\
             \n\
             * trial fixes, in one PR ([#50]({REPO}/issues/50)) ([aaaaaaa]({REPO}/commit/{a}))\n\
             \n\
             \n\
             ### Documentation\n\
             \n\
             * **setup:** a template ([#55]({REPO}/issues/55)) ([ddddddd]({REPO}/commit/{d}))\n",
            a = sha('a'),
            b = sha('b'),
            c = sha('c'),
            d = sha('d'),
        )
    );
}

#[test]
fn a_breaking_change_is_listed_first_and_in_its_own_section() {
    let commits = vec![
        commit("1111111aaaa", "feat!: drop the v1 API", ""),
        commit(
            "2222222bbbb",
            "fix(cli): rename a flag",
            "BREAKING CHANGE: `--old` is now `--new`",
        ),
    ];
    let text = section(
        v("2.0.0"),
        Some("v1.0.0"),
        "v2.0.0",
        "2026-10-12",
        &commits,
        &default_changelog_sections(),
        REPO,
    );
    let breaking = text.find("### ⚠ BREAKING CHANGES").expect("{text}");
    let features = text.find("### Features").expect("{text}");
    assert!(breaking < features, "{text}");
    assert!(text.contains("* drop the v1 API ([1111111]"), "{text}");
    assert!(
        text.contains("* **cli:** `--old` is now `--new` ([2222222]"),
        "{text}"
    );
    assert!(
        text.contains("### Bug Fixes\n\n* **cli:** rename a flag"),
        "{text}"
    );
}

#[test]
fn a_first_release_heading_has_no_compare_link_and_sections_follow_the_configuration() {
    let sections = vec![crate::config::ChangelogSection {
        kind: "fix".into(),
        section: "Fixes".into(),
    }];
    let text = section(
        v("0.1.0"),
        None,
        "acme-v0.1.0",
        "2026-10-12",
        &[
            commit("1a2b3c4d5e", "feat: first", ""),
            commit("abcdef0123", "fix: second", ""),
        ],
        &sections,
        REPO,
    );
    assert!(
        text.starts_with("## 0.1.0 (2026-10-12)\n\n\n### Fixes\n\n* second"),
        "{text}"
    );
    assert!(
        !text.contains("first"),
        "a type no section lists is left out"
    );
}

#[test]
fn the_section_goes_under_the_title_before_the_previous_one_and_replaces_itself() {
    let new = "## [1.1.0](x) (2026-10-12)\n\n\n### Features\n\n* b\n";
    let old = "# Changelog\n\n## [1.0.0](x) (2026-10-01)\n\n\n### Features\n\n* a\n";
    let once = insert(Some(old), v("1.1.0"), new);
    assert_eq!(
        once,
        "# Changelog\n\n## [1.1.0](x) (2026-10-12)\n\n\n### Features\n\n* b\n\n## [1.0.0](x) (2026-10-01)\n\n\n### Features\n\n* a\n"
    );
    let replaced = insert(
        Some(&once),
        v("1.1.0"),
        "## [1.1.0](x) (2026-10-13)\n\n\n### Bug Fixes\n\n* c\n",
    );
    assert!(replaced.contains("2026-10-13"), "{replaced}");
    assert!(!replaced.contains("2026-10-12"), "{replaced}");
    assert!(replaced.contains("## [1.0.0](x)"), "{replaced}");
    assert_eq!(
        insert(None, v("0.1.0"), "## 0.1.0 (d)\n"),
        "# Changelog\n\n## 0.1.0 (d)\n"
    );
    assert_eq!(
        insert(Some("# Changelog\n"), v("0.1.0"), "## 0.1.0 (d)\n"),
        "# Changelog\n\n## 0.1.0 (d)\n"
    );
}

#[test]
fn the_notes_of_a_version_are_read_back_from_the_changelog() {
    let text = "# Changelog\n\n## [1.1.0](x) (2026-10-12)\n\n\n### Features\n\n* b\n\n## 1.0.0 (2026-10-01)\n\n* a\n";
    assert_eq!(
        section_of(text, v("1.1.0")).as_deref(),
        Some("## [1.1.0](x) (2026-10-12)\n\n\n### Features\n\n* b")
    );
    assert_eq!(
        section_of(text, v("1.0.0")).as_deref(),
        Some("## 1.0.0 (2026-10-01)\n\n* a")
    );
    assert_eq!(section_of(text, v("1.2.0")), None);
    assert_eq!(
        section_of("## [1.1.0-rc.1](x)\n", v("1.1.0")),
        None,
        "another version that starts the same is not this one"
    );
}

#[test]
fn cargo_toml_and_its_lock_take_the_version_with_no_network() {
    let manifest = "[package]\nname = \"acme\"\nversion = \"0.3.0\" # the crate\nedition = \"2024\"\n\n[dependencies]\nserde = { version = \"1.0\" }\nregex = \"1.13\"\n";
    let (old, text) = set_cargo_toml(manifest, v("0.4.0")).unwrap();
    assert_eq!(old, "0.3.0");
    assert_eq!(
        text,
        manifest.replace(
            "version = \"0.3.0\" # the crate",
            "version = \"0.4.0\" # the crate"
        )
    );
    let workspace = "[workspace]\nmembers = [\"a\"]\n\n[workspace.package]\nversion = \"1.2.3\"\n";
    let (old, text) = set_cargo_toml(workspace, v("1.3.0")).unwrap();
    assert_eq!(old, "1.2.3");
    assert!(text.contains("[workspace.package]\nversion = \"1.3.0\"\n"));
    assert_eq!(
        set_cargo_toml("[workspace]\nmembers = [\"a\"]\n", v("1.0.0")),
        None,
        "a virtual workspace declares no version of its own"
    );

    let lock = "# auto-generated\nversion = 4\n\n[[package]]\nname = \"acme\"\nversion = \"0.3.0\"\ndependencies = [\n \"serde\",\n]\n\n[[package]]\nname = \"serde\"\nversion = \"0.3.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"abc\"\n\n[[package]]\nname = \"acme-macros\"\nversion = \"0.1.0\"\n";
    let text = set_cargo_lock(lock, "0.3.0", v("0.4.0"));
    assert_eq!(
        text,
        lock.replacen(
            "name = \"acme\"\nversion = \"0.3.0\"",
            "name = \"acme\"\nversion = \"0.4.0\"",
            1
        ),
        "only the local package at the old version moves; the registry's and the lock's own `version` stay"
    );
}

#[test]
fn a_version_file_takes_the_version_at_every_match_of_its_pattern() {
    let file = VersionFile {
        path: "web/package.json".into(),
        pattern: "\"version\": \"{version}\"".into(),
    };
    let text = "{\n  \"name\": \"acme\",\n  \"version\": \"1.2.3-beta.1\",\n  \"deps\": {\"x\": \"1.0.0\"}\n}\n";
    assert_eq!(
        set_pattern(text, &file, v("1.3.0")).unwrap(),
        text.replace("\"version\": \"1.2.3-beta.1\"", "\"version\": \"1.3.0\"")
    );
    let err = set_pattern("{}", &file, v("1.3.0")).unwrap_err();
    assert!(err.contains("nothing matches"), "{err}");
    let dollar = VersionFile {
        path: "VERSION".into(),
        pattern: "$v{version}".into(),
    };
    assert_eq!(
        set_pattern("$v0.1.0\n", &dollar, v("0.2.0")).unwrap(),
        "$v0.2.0\n",
        "the pattern is literal text"
    );
}
