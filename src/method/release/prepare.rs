//! What `release prepare` computes (`docs/reference/05-release-cycle.md` §1), as
//! pure functions: the next version from the commits of the range, the
//! CHANGELOG section and release notes in release-please's format, and the
//! version written into the version files. The range is Git's, read along
//! the graph by the caller (`<last release tag>..HEAD`, or the whole history
//! for a first release), never by date: a commit merged into the default
//! branch while a cycle was open, which the merge-back brings into the next
//! range, is in the next release's notes.

use std::sync::LazyLock;

use regex::Regex;

use super::shape::Version;
use crate::config::{ChangelogSection, VersionFile};

/// A commit of the range, as `git log` gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub sha: String,
    pub subject: String,
    pub body: String,
}

/// A Conventional Commit, read from a subject and a body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conventional {
    pub kind: String,
    pub scope: Option<String>,
    pub description: String,
    pub breaking: bool,
    /// The text of its `BREAKING CHANGE:` footer, when it has one.
    pub breaking_note: Option<String>,
}

static SUBJECT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?P<type>[A-Za-z]+)(?:\((?P<scope>[^()]*)\))?(?P<bang>!)?: +(?P<desc>\S.*)$")
        .unwrap()
});
static BREAKING_FOOTER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^BREAKING[ -]CHANGE: *(?P<note>.*)$").unwrap());
static ISSUE_REF: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?P<before>^|[\s(])#(?P<n>\d+)\b").unwrap());
const SEMVER_TEXT: &str = r"\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?";

/// The Conventional Commit a commit is, or `None` (a merge, a free text).
pub fn parse(subject: &str, body: &str) -> Option<Conventional> {
    let c = SUBJECT.captures(subject.trim())?;
    // The footer's text runs to the next blank line.
    let lines: Vec<&str> = body.lines().collect();
    let footer = lines.iter().enumerate().find_map(|(i, line)| {
        let f = BREAKING_FOOTER.captures(line.trim_end())?;
        let mut note = vec![f["note"].trim().to_string()];
        note.extend(
            lines[i + 1..]
                .iter()
                .take_while(|l| !l.trim().is_empty())
                .map(|l| l.trim().to_string()),
        );
        Some(note.join(" ").trim().to_string())
    });
    let scope = c
        .name("scope")
        .map(|m| m.as_str().trim().to_string())
        .filter(|s| !s.is_empty());
    Some(Conventional {
        kind: c["type"].to_ascii_lowercase(),
        scope,
        description: c["desc"].trim().to_string(),
        breaking: c.name("bang").is_some() || footer.is_some(),
        breaking_note: footer.filter(|n| !n.is_empty()),
    })
}

/// How far the version moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Bump {
    Patch,
    Minor,
    Major,
}

/// What the range asks of the version: a breaking change a major, a `feat`
/// a minor, any other change the CHANGELOG lists a patch. `None` when the
/// range holds nothing to release (no Conventional Commit of a listed type,
/// no breaking change).
pub fn bump(commits: &[Commit], sections: &[ChangelogSection]) -> Option<Bump> {
    commits
        .iter()
        .filter_map(|c| parse(&c.subject, &c.body))
        .filter_map(|c| {
            if c.breaking {
                Some(Bump::Major)
            } else if c.kind == "feat" {
                Some(Bump::Minor)
            } else if sections.iter().any(|s| s.kind == c.kind) {
                Some(Bump::Patch)
            } else {
                None
            }
        })
        .max()
}

/// The next version after `previous` (`0.0.0` for a first release). Before
/// 1.0 a breaking change or a `feat` bumps the minor and anything else the
/// patch; from 1.0, semantic versioning.
pub fn next_version(previous: Option<Version>, bump: Bump) -> Version {
    let p = previous.unwrap_or(Version {
        major: 0,
        minor: 0,
        patch: 0,
    });
    let minor = Version {
        major: p.major,
        minor: p.minor + 1,
        patch: 0,
    };
    let patch = Version {
        patch: p.patch + 1,
        ..p
    };
    match (p.major, bump) {
        (0, Bump::Major | Bump::Minor) => minor,
        (_, Bump::Major) => Version {
            major: p.major + 1,
            minor: 0,
            patch: 0,
        },
        (_, Bump::Minor) => minor,
        (_, Bump::Patch) => patch,
    }
}

/// Where the links of the notes point: `https://github.com/<owner>/<repo>`.
pub fn repository_url(repository: &str) -> String {
    format!("https://github.com/{repository}")
}

/// `#12` → `[#12](<repo>/issues/12)`, as release-please links a PR.
fn link_issues(text: &str, repo_url: &str) -> String {
    ISSUE_REF
        .replace_all(text, |c: &regex::Captures| {
            format!("{}[#{n}]({repo_url}/issues/{n})", &c["before"], n = &c["n"])
        })
        .into_owned()
}

fn entry(scope: Option<&str>, text: &str, sha: &str, repo_url: &str) -> String {
    let short: String = sha.chars().take(7).collect();
    let scope = scope.map(|s| format!("**{s}:** ")).unwrap_or_default();
    format!(
        "* {scope}{} ([{short}]({repo_url}/commit/{sha}))",
        link_issues(text, repo_url)
    )
}

/// The CHANGELOG section of `version`, in release-please's format: its
/// heading (a compare link from `previous_tag`, none for a first release),
/// the breaking changes first, then one section per configured type in the
/// configured order, each entry sorted by scope then text, with its PR and
/// commit links. It is also the text of the release notes.
pub fn section(
    version: Version,
    previous_tag: Option<&str>,
    tag: &str,
    date: &str,
    commits: &[Commit],
    sections: &[ChangelogSection],
    repo_url: &str,
) -> String {
    let heading = match previous_tag {
        Some(prev) => format!("## [{version}]({repo_url}/compare/{prev}...{tag}) ({date})"),
        None => format!("## {version} ({date})"),
    };
    let parsed: Vec<(&Commit, Conventional)> = commits
        .iter()
        .filter_map(|c| parse(&c.subject, &c.body).map(|p| (c, p)))
        .collect();
    let sorted = |mut list: Vec<(String, String, String)>| {
        list.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
        list.into_iter()
            .map(|(scope, text, sha)| {
                entry(
                    (!scope.is_empty()).then_some(scope.as_str()),
                    &text,
                    &sha,
                    repo_url,
                )
            })
            .collect::<Vec<_>>()
    };
    let mut out = heading;
    let mut push = |title: &str, entries: Vec<String>| {
        if !entries.is_empty() {
            out.push_str(&format!("\n\n\n### {title}\n\n{}", entries.join("\n")));
        }
    };
    push(
        "⚠ BREAKING CHANGES",
        sorted(
            parsed
                .iter()
                .filter(|(_, p)| p.breaking)
                .map(|(c, p)| {
                    (
                        p.scope.clone().unwrap_or_default(),
                        p.breaking_note.clone().unwrap_or(p.description.clone()),
                        c.sha.clone(),
                    )
                })
                .collect(),
        ),
    );
    for s in sections {
        push(
            &s.section,
            sorted(
                parsed
                    .iter()
                    .filter(|(_, p)| p.kind == s.kind)
                    .map(|(c, p)| {
                        (
                            p.scope.clone().unwrap_or_default(),
                            p.description.clone(),
                            c.sha.clone(),
                        )
                    })
                    .collect(),
            ),
        );
    }
    out.push('\n');
    out
}

/// Whether `line` is the CHANGELOG heading of `version` (`## [X.Y.Z](…)`
/// or `## X.Y.Z (…)`).
fn heads(line: &str, version: Version) -> bool {
    let v = version.to_string();
    line.strip_prefix("## ").is_some_and(|rest| {
        let rest = rest.strip_prefix('[').unwrap_or(rest);
        rest.strip_prefix(v.as_str())
            .is_some_and(|after| after.is_empty() || after.starts_with([']', ' ']))
    })
}

/// The CHANGELOG with `section` as its newest entry: under the title, before
/// the previous newest; a section of the same version already there is
/// replaced, so that preparing twice writes the same file.
pub fn insert(changelog: Option<&str>, version: Version, section: &str) -> String {
    let text = changelog.unwrap_or("").replace("\r\n", "\n");
    if text.trim().is_empty() {
        return format!("# Changelog\n\n{section}");
    }
    let lines: Vec<&str> = text.lines().collect();
    let heading = |l: &&str| l.starts_with("## ");
    let mut out: Vec<String> = Vec::new();
    let joined = |l: &[&str]| l.iter().map(|x| format!("{x}\n")).collect::<String>();
    if let Some(at) = lines.iter().position(|l| heads(l, version)) {
        let end = lines[at + 1..]
            .iter()
            .position(heading)
            .map_or(lines.len(), |i| at + 1 + i);
        out.push(joined(&lines[..at]));
        out.push(section.to_string());
        if end < lines.len() {
            out.push("\n".to_string());
            out.push(joined(&lines[end..]));
        }
    } else if let Some(at) = lines.iter().position(heading) {
        out.push(joined(&lines[..at]));
        out.push(section.to_string());
        out.push("\n".to_string());
        out.push(joined(&lines[at..]));
    } else {
        let mut head = joined(&lines);
        while head.ends_with("\n\n") {
            head.pop();
        }
        out.push(head);
        out.push("\n".to_string());
        out.push(section.to_string());
    }
    out.concat()
}

/// The section of `version` in a CHANGELOG, heading included, without its
/// trailing blank lines: the text of the GitHub release (the bridge's GO).
pub fn section_of(changelog: &str, version: Version) -> Option<String> {
    let lines: Vec<&str> = changelog.lines().collect();
    let at = lines.iter().position(|l| heads(l, version))?;
    let end = lines[at + 1..]
        .iter()
        .position(|l| l.starts_with("## "))
        .map_or(lines.len(), |i| at + 1 + i);
    let text = lines[at..end].join("\n");
    Some(text.trim_end().to_string())
}

/// The version `Cargo.toml` declares (`[package]`, else
/// `[workspace.package]`), and the file with `version` in its place; `None`
/// when it declares none (a virtual workspace whose members keep their own).
pub fn set_cargo_toml(text: &str, version: Version) -> Option<(String, String)> {
    static TABLE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^\s*\[\s*(?P<name>[^\[\]]+?)\s*\]\s*(#.*)?$").unwrap());
    static VERSION: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"^(?P<pre>\s*version\s*=\s*")(?P<v>[^"]*)(?P<post>".*)$"#).unwrap()
    });
    let mut table = String::new();
    let mut old: Option<String> = None;
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        let end = &line[body.len()..];
        if let Some(t) = TABLE.captures(body) {
            table = t["name"].to_string();
        }
        match VERSION.captures(body) {
            Some(c) if table == "package" || table == "workspace.package" => {
                old.get_or_insert_with(|| c["v"].to_string());
                out.push_str(&format!("{}{version}{}{end}", &c["pre"], &c["post"]));
            }
            _ => out.push_str(line),
        }
    }
    old.map(|o| (o, out))
}

/// `Cargo.lock` with every local package (no `source`, a member of the
/// workspace) at `old` moved to `new`; packages from a registry or git, and
/// members that keep their own version, are left as they are.
pub fn set_cargo_lock(text: &str, old: &str, new: Version) -> String {
    let mut out = String::with_capacity(text.len());
    let mut block: Vec<&str> = Vec::new();
    let flush = |block: &mut Vec<&str>, out: &mut String| {
        let local = !block.iter().any(|l| l.trim_start().starts_with("source "));
        let at_old = block
            .iter()
            .any(|l| l.trim_end() == format!("version = \"{old}\""));
        let package = block.first().is_some_and(|l| l.trim() == "[[package]]");
        for l in block.drain(..) {
            if package && local && at_old && l.trim_end() == format!("version = \"{old}\"") {
                let end = &l[l.trim_end().len()..];
                out.push_str(&format!("version = \"{new}\"{end}"));
            } else {
                out.push_str(l);
            }
        }
    };
    for line in text.split_inclusive('\n') {
        if line.trim() == "[[package]]" || line.trim_start().starts_with('[') {
            flush(&mut block, &mut out);
        }
        block.push(line);
    }
    flush(&mut block, &mut out);
    out
}

/// `text` with every match of a version file's pattern carrying `version`;
/// a pattern that matches nothing is an error (the file would silently keep
/// its old version).
pub fn set_pattern(text: &str, file: &VersionFile, version: Version) -> Result<String, String> {
    let (before, after) = file
        .pattern
        .split_once("{version}")
        .ok_or_else(|| format!("{}: the pattern holds no {{version}}", file.path))?;
    let re = Regex::new(&format!(
        "{}(?:{SEMVER_TEXT}){}",
        regex::escape(before),
        regex::escape(after)
    ))
    .map_err(|e| format!("{}: {e}", file.path))?;
    if !re.is_match(text) {
        return Err(format!(
            "{}: nothing matches the pattern `{}`",
            file.path, file.pattern
        ));
    }
    let with = format!("{before}{version}{after}");
    Ok(re.replace_all(text, regex::NoExpand(&with)).into_owned())
}

#[cfg(test)]
mod tests;
