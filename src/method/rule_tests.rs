//! The tests a business rule cites (spec 02 §3, SR-010): `doctor` checks
//! that each one exists in the repository. Light and language-agnostic: it
//! reads the backticked tokens of a rule's `## Test` section, never runs a
//! test, and fails only on what it can prove absent.
//!
//! - A token that looks like a repository path is a file: it must exist.
//! - A test name (`a_snake_case_name`, or a nested JS name `a > b > c`) is
//!   looked for in the file named beside it: in the same paragraph or list
//!   item, or, for a list item, in the paragraph or item before it.
//! - A Rust module path (`a::b::name`) is looked for in the file its modules
//!   lead to (`src/a/b.rs`, `src/a/b/mod.rs`, `src/a.rs`, … then `tests/`);
//!   a `*` last segment only needs that file.
//! - A name is found when its last segment appears literally in the file. A
//!   name whose file cannot be located is counted, never a failure; any other
//!   token (a job, a task, a word) is ignored.

use std::collections::{BTreeSet, HashMap};
use std::sync::LazyLock;

use regex::Regex;

use super::business_rules::section_items;

static TOKEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"`([^`]+)`").unwrap());
static MODULE_PATH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(?:crate::)?[a-z_][a-z0-9_]*(?:::[a-z_][a-z0-9_]*)*::(?:[A-Za-z_][A-Za-z0-9_]*|\*)$",
    )
    .unwrap()
});
static SNAKE_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z][a-z0-9]*(?:_[a-z0-9]+)+$").unwrap());
static LIST_ITEM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(?:[-*+]|\d+[.)])\s").unwrap());
static PARAMETERS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[[^\]]*\]$").unwrap());

/// What a rule's Test section cites, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cited {
    /// A repository path: it must exist.
    File(String),
    /// A test name, and the file it is looked for in, when one is named
    /// beside it.
    Name { name: String, file: Option<String> },
    /// A Rust module path; `file` is the one named beside it, if any, tried
    /// when its modules lead nowhere.
    Module { path: String, file: Option<String> },
}

/// A backticked token, read.
enum Token {
    File(String),
    Name(String),
    Module(String),
    /// `path::name`, as pytest names a test.
    FileName(String, String),
}

/// Whether a token is a repository path: relative, no space, a `/`, and a
/// last segment with an extension or under `scripts/`.
fn is_path(t: &str) -> bool {
    let t = t.strip_prefix("./").unwrap_or(t);
    if t.is_empty()
        || t.starts_with('/')
        || t.contains("://")
        || t.contains(char::is_whitespace)
        || t.contains(['*', '`', '?', '<', '>', '{', '}', '$'])
        || !t.contains('/')
        || t.ends_with('/')
    {
        return false;
    }
    let last = t.rsplit('/').next().unwrap_or(t);
    let extension = last
        .rsplit_once('.')
        .is_some_and(|(_, ext)| !ext.is_empty() && ext.chars().all(|c| c.is_ascii_alphanumeric()));
    extension || t.starts_with("scripts/")
}

fn normalize_path(t: &str) -> String {
    t.strip_prefix("./").unwrap_or(t).to_string()
}

fn token(raw: &str) -> Option<Token> {
    let t = raw.trim();
    if is_path(t) {
        return Some(Token::File(normalize_path(t)));
    }
    if let Some((path, rest)) = t.split_once("::") {
        if is_path(path) && !rest.trim().is_empty() {
            return Some(Token::FileName(
                normalize_path(path),
                rest.trim().to_string(),
            ));
        }
    }
    if MODULE_PATH.is_match(t) {
        return Some(Token::Module(t.to_string()));
    }
    if SNAKE_NAME.is_match(t) {
        return Some(Token::Name(t.to_string()));
    }
    if t.contains(" > ") && !last_segment(t).is_empty() {
        return Some(Token::Name(t.to_string()));
    }
    None
}

/// The citations of a rule's Test section, in order. A name takes the file
/// named nearest before it in its paragraph or list item, else the first
/// one after it there; a list item that names no file takes the last file
/// of the paragraph or item before it.
pub fn citations(section: &str) -> Vec<Cited> {
    let mut out = Vec::new();
    let mut carried: Option<String> = None;
    for item in section_items(section) {
        let tokens: Vec<Token> = TOKEN
            .captures_iter(&item)
            .filter_map(|c| token(&c[1]))
            .collect();
        let files: Vec<(usize, &str)> = tokens
            .iter()
            .enumerate()
            .filter_map(|(i, t)| match t {
                Token::File(p) | Token::FileName(p, _) => Some((i, p.as_str())),
                _ => None,
            })
            .collect();
        let is_item = LIST_ITEM.is_match(&item);
        let beside = |i: usize| -> Option<String> {
            files
                .iter()
                .rev()
                .find(|(j, _)| *j < i)
                .or_else(|| files.iter().find(|(j, _)| *j > i))
                .map(|(_, p)| p.to_string())
                .or_else(|| if is_item { carried.clone() } else { None })
        };
        for (i, t) in tokens.iter().enumerate() {
            match t {
                Token::File(p) => out.push(Cited::File(p.clone())),
                Token::FileName(p, name) => {
                    out.push(Cited::File(p.clone()));
                    out.push(Cited::Name {
                        name: name.clone(),
                        file: Some(p.clone()),
                    });
                }
                Token::Name(name) => out.push(Cited::Name {
                    name: name.clone(),
                    file: beside(i),
                }),
                Token::Module(path) => out.push(Cited::Module {
                    path: path.clone(),
                    file: beside(i),
                }),
            }
        }
        if let Some((_, last)) = files.last() {
            carried = Some(last.to_string());
        } else if !is_item {
            carried = None;
        }
    }
    out
}

/// What a name is looked for by: after the last ` > ` (a nested JS name) or
/// the last `::`, without a trailing `[parameters]`.
fn last_segment(name: &str) -> &str {
    let last = name
        .rsplit_once(" > ")
        .map(|(_, l)| l)
        .or_else(|| name.rsplit_once("::").map(|(_, l)| l))
        .unwrap_or(name)
        .trim();
    PARAMETERS
        .find(last)
        .map_or(last, |m| last[..m.start()].trim_end())
}

/// The files a Rust module path's modules may live in, the most specific
/// first: `src/a/b.rs`, `src/a/b/mod.rs`, `src/a.rs`, `src/a/mod.rs`, then
/// the same under `tests/` and an integration test's `tests/a/main.rs`.
pub fn module_files(path: &str) -> Vec<String> {
    let path = path.strip_prefix("crate::").unwrap_or(path);
    let segments: Vec<&str> = path.split("::").collect();
    let modules = &segments[..segments.len().saturating_sub(1)];
    let mut out = Vec::new();
    for base in ["src", "tests"] {
        for k in (1..=modules.len()).rev() {
            let p = modules[..k].join("/");
            out.push(format!("{base}/{p}.rs"));
            out.push(format!("{base}/{p}/mod.rs"));
            if base == "tests" && k == 1 {
                out.push(format!("{base}/{p}/main.rs"));
            }
        }
    }
    out
}

/// Whether `text` holds `needle` literally, or once quotes escaped in a
/// string literal (`\'`, `\"`) are read as written.
fn holds(text: &str, needle: &str) -> bool {
    text.contains(needle)
        || text
            .replace("\\'", "'")
            .replace("\\\"", "\"")
            .replace("\\`", "`")
            .contains(needle)
}

/// The counts and the problems of every rule judged.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tally {
    pub found: usize,
    /// Every file found: cited, or where a name was found.
    pub files: BTreeSet<String>,
    pub not_located: usize,
    /// `<ID>: <what is absent>`, in order, each once.
    pub problems: Vec<String>,
}

impl Tally {
    fn problem(&mut self, p: String) {
        if !self.problems.contains(&p) {
            self.problems.push(p);
        }
    }
}

/// The judge of the rules' citations. `read` returns a file's text, `None`
/// when it is absent, an error when it cannot be read; each file is read
/// once.
pub struct Judge<R> {
    read: R,
    cache: HashMap<String, Option<String>>,
}

impl<R: FnMut(&str) -> Result<Option<String>, String>> Judge<R> {
    pub fn new(read: R) -> Self {
        Judge {
            read,
            cache: HashMap::new(),
        }
    }

    fn text(&mut self, path: &str) -> Result<Option<&str>, String> {
        if !self.cache.contains_key(path) {
            let text = (self.read)(path)?;
            self.cache.insert(path.to_string(), text);
        }
        Ok(self.cache[path].as_deref())
    }

    /// Whether `name` is in `path`: `None` when the file is absent.
    fn holds(&mut self, path: &str, name: &str) -> Result<Option<bool>, String> {
        Ok(self.text(path)?.map(|t| holds(t, name)))
    }

    /// Judge one rule's Test section; `rev` is said in the problems. An
    /// error is a file that could not be read: nothing is concluded.
    pub fn rule(
        &mut self,
        id: &str,
        section: &str,
        rev: &str,
        tally: &mut Tally,
    ) -> Result<(), String> {
        for cited in citations(section) {
            match cited {
                Cited::File(path) => match self.text(&path)? {
                    Some(_) => {
                        tally.files.insert(path);
                    }
                    None => tally.problem(format!("{id}: `{path}` is absent at {rev}")),
                },
                Cited::Name { file: None, .. } => tally.not_located += 1,
                Cited::Name {
                    name,
                    file: Some(path),
                } => match self.holds(&path, last_segment(&name))? {
                    Some(true) => {
                        tally.found += 1;
                        tally.files.insert(path);
                    }
                    Some(false) => tally.problem(format!("{id}: `{name}` is not in `{path}`")),
                    // The file is cited, and said absent once.
                    None => {}
                },
                Cited::Module { path, file } => self.module(id, &path, file, tally)?,
            }
        }
        Ok(())
    }

    fn module(
        &mut self,
        id: &str,
        path: &str,
        beside: Option<String>,
        tally: &mut Tally,
    ) -> Result<(), String> {
        let name = last_segment(path).to_string();
        let wildcard = name == "*";
        let mut first_present = None;
        for candidate in module_files(path) {
            let hit = if wildcard {
                self.text(&candidate)?.map(|_| true)
            } else {
                self.holds(&candidate, &name)?
            };
            match hit {
                Some(true) => {
                    tally.found += 1;
                    tally.files.insert(candidate);
                    return Ok(());
                }
                Some(false) => {
                    first_present.get_or_insert(candidate);
                }
                None => {}
            }
        }
        if let Some(file) = beside {
            let hit = if wildcard {
                self.text(&file)?.map(|_| true)
            } else {
                self.holds(&file, &name)?
            };
            if hit == Some(true) {
                tally.found += 1;
                tally.files.insert(file);
                return Ok(());
            }
        }
        match first_present {
            Some(file) => tally.problem(format!("{id}: `{path}` is not in `{file}`")),
            None => tally.not_located += 1,
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tokens_of_a_test_section_are_files_names_and_module_paths_or_ignored() {
        let section = "`apps/web/__tests__/worker.test.ts`, in the\n`web#test` task of the `build-test` job:\n\n- `run > twice > refuses the second`\n\nAnd `tick::tests::a_case` (`tests/b.rs`), `c_case`; `tests/x.py::test_y[1]`, `scripts/check`.\n";
        let file = |p: &str| Some(p.to_string());
        assert_eq!(
            citations(section),
            vec![
                Cited::File("apps/web/__tests__/worker.test.ts".into()),
                Cited::Name {
                    name: "run > twice > refuses the second".into(),
                    file: file("apps/web/__tests__/worker.test.ts"),
                },
                Cited::Module {
                    path: "tick::tests::a_case".into(),
                    file: file("tests/b.rs"),
                },
                Cited::File("tests/b.rs".into()),
                Cited::Name {
                    name: "c_case".into(),
                    file: file("tests/b.rs"),
                },
                Cited::File("tests/x.py".into()),
                Cited::Name {
                    name: "test_y[1]".into(),
                    file: file("tests/x.py"),
                },
                Cited::File("scripts/check".into()),
            ]
        );
        assert_eq!(last_segment("a > b > the case"), "the case");
        assert_eq!(last_segment("a::b::*"), "*");
        assert_eq!(last_segment("test_y[1]"), "test_y");
        assert_eq!(
            module_files("workflows::pr_guards::a_case"),
            [
                "src/workflows/pr_guards.rs",
                "src/workflows/pr_guards/mod.rs",
                "src/workflows.rs",
                "src/workflows/mod.rs",
                "tests/workflows/pr_guards.rs",
                "tests/workflows/pr_guards/mod.rs",
                "tests/workflows.rs",
                "tests/workflows/mod.rs",
                "tests/workflows/main.rs",
            ]
        );
    }
}
