//! Path globs as the method reads them (REQ-MERGE-046, REQ-REL-012): shell
//! style, where `*` and `**` also cross `/`, `?` is one character and
//! `[seq]` a class (`[!seq]` its complement); or a directory prefix: the
//! pattern without its trailing `*` and `/` is a directory the path lies
//! under. Case-sensitive. A glob library whose `*` stops at `/` would drift
//! from the verified behaviour.
//!
//! Paths are repository paths, `/`-separated on every platform, as GitHub
//! gives them. A path read from the file system goes through [`repo_path`]
//! first: on Windows its separator is `\`, which no pattern matches.

/// A path relative to the repository root, in the `/` form patterns are
/// written in. Every `\` becomes `/`, whatever the platform: a repository
/// path never holds a literal `\`.
pub fn repo_path(relative: &std::path::Path) -> String {
    relative.to_string_lossy().replace('\\', "/")
}

/// Shell-style match of a whole path.
pub fn fnmatch(path: &str, pattern: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let s: Vec<char> = path.chars().collect();
    matches(&s, &p)
}

fn matches(s: &[char], p: &[char]) -> bool {
    // Iterative matcher with backtracking on the last `*`.
    let (mut si, mut pi) = (0usize, 0usize);
    let mut star: Option<(usize, usize)> = None;
    while si < s.len() {
        if pi < p.len() {
            match p[pi] {
                '*' => {
                    while pi < p.len() && p[pi] == '*' {
                        pi += 1;
                    }
                    star = Some((pi, si));
                    continue;
                }
                '?' => {
                    si += 1;
                    pi += 1;
                    continue;
                }
                '[' => {
                    if let Some((ok, next)) = class(&p[pi..], s[si]) {
                        if ok {
                            si += 1;
                            pi += next;
                            continue;
                        }
                    } else if s[si] == '[' {
                        // An unclosed `[` is a literal.
                        si += 1;
                        pi += 1;
                        continue;
                    }
                }
                c if c == s[si] => {
                    si += 1;
                    pi += 1;
                    continue;
                }
                _ => {}
            }
        }
        match star {
            Some((sp, ss)) => {
                pi = sp;
                si = ss + 1;
                star = Some((sp, ss + 1));
            }
            None => return false,
        }
    }
    p[pi..].iter().all(|c| *c == '*')
}

/// Match `c` against the class starting at `p[0] == '['`; returns whether
/// it matched and the class length, or `None` for an unclosed class.
fn class(p: &[char], c: char) -> Option<(bool, usize)> {
    let mut i = 1;
    let negate = matches!(p.get(i), Some('!'));
    if negate {
        i += 1;
    }
    let mut matched = false;
    let mut first = true;
    while i < p.len() {
        if p[i] == ']' && !first {
            return Some((matched != negate, i + 1));
        }
        first = false;
        if i + 2 < p.len() && p[i + 1] == '-' && p[i + 2] != ']' {
            if p[i] <= c && c <= p[i + 2] {
                matched = true;
            }
            i += 3;
        } else {
            if p[i] == c {
                matched = true;
            }
            i += 1;
        }
    }
    None
}

/// Whether `path` matches `pattern` by glob or directory prefix.
pub fn path_matches(path: &str, pattern: &str) -> bool {
    if pattern.is_empty() {
        return false;
    }
    if fnmatch(path, pattern) {
        return true;
    }
    let directory = pattern.trim_end_matches('*').trim_end_matches('/');
    !directory.is_empty() && path.starts_with(&format!("{directory}/"))
}

/// The paths matching any pattern, in path order, each once.
pub fn hits(paths: &[String], patterns: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for path in paths {
        if out.contains(path) {
            continue;
        }
        if patterns.iter().any(|p| path_matches(path, p)) {
            out.push(path.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn a_star_crosses_slashes() {
        assert!(fnmatch("src/a/b.rs", "src/*.rs"));
        assert!(fnmatch("src/a/b.rs", "**/b.rs"));
        assert!(fnmatch("a/b/c", "a/**"));
        assert!(fnmatch("anything/at/all", "**"));
        assert!(!fnmatch("src/a/b.ts", "src/*.rs"));
    }

    #[test]
    fn question_marks_and_classes() {
        assert!(fnmatch("a1", "a?"));
        assert!(!fnmatch("a12", "a?"));
        assert!(fnmatch("v2", "v[0-9]"));
        assert!(!fnmatch("vx", "v[0-9]"));
        assert!(fnmatch("vx", "v[!0-9]"));
        assert!(fnmatch("a[b", "a[b"), "an unclosed class is a literal");
    }

    #[test]
    fn a_directory_glob_matches_below_itself() {
        for pattern in ["a/**", "a/*", "a/", "a"] {
            assert!(path_matches("a/b/c.rs", pattern), "{pattern}");
        }
        assert!(!path_matches("ab/c.rs", "a/**"));
        assert!(
            !path_matches("README.md", "docs/**"),
            "an unrelated top-level file"
        );
    }

    #[test]
    fn the_bare_star_uses_only_the_glob_test() {
        assert!(path_matches("anything/below", "*"));
    }

    #[test]
    fn empty_patterns_are_ignored_and_hits_are_deduplicated_in_path_order() {
        let paths = v(&[
            "src/auth/a.rs",
            "docs/x.md",
            "src/auth/a.rs",
            "src/auth/b.rs",
        ]);
        assert_eq!(
            hits(&paths, &v(&["", "src/auth/**"])),
            v(&["src/auth/a.rs", "src/auth/b.rs"])
        );
        assert!(hits(&paths, &v(&[""])).is_empty());
    }

    #[test]
    fn a_windows_path_is_matched_in_its_slash_form() {
        // Built as a string, so the test means the same on every platform.
        let walked = std::path::Path::new(".github\\workflows\\ci.yml");
        assert_eq!(repo_path(walked), ".github/workflows/ci.yml");
        assert_eq!(
            repo_path(std::path::Path::new("src/github/auth.rs")),
            "src/github/auth.rs",
            "a path already in `/` form is unchanged"
        );
        let paths = vec![repo_path(walked)];
        assert_eq!(hits(&paths, &v(&[".github/workflows/ci.yml"])), paths);
    }

    #[test]
    fn matching_is_case_sensitive() {
        assert!(!path_matches("SRC/a.rs", "src/**"));
    }
}
