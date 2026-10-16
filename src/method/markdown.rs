//! The one reader of ticket descriptions every guard uses (REQ-MERGE-030,
//! REQ-MERGE-032, REQ-MERGE-067): fenced blocks, checkboxes, headings and
//! sections. One reading, never two (`docs/reference/01-ticket-contract.md`).

use std::sync::LazyLock;

use regex::Regex;

static FENCE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[ \t]{0,3}(?:`{3,}|~{3,})").unwrap());
static CHECKBOX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[ \t]*(?:[-*+]|\d+[.)])[ \t]+\[([ xX])\].*$").unwrap());
static HEADING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^ {0,3}(#{1,6})(?:[ \t]+(.*?))?[ \t]*$").unwrap());

/// The lines of `text` with fenced code blocks blanked (line count kept).
///
/// A fence line opens a block and the next fence line closes it, whatever
/// its character or length (deliberately forgiving). A fence that is never
/// closed is not a block: blanking to the end would hide every criterion
/// below a stray fence.
pub fn without_fences(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
    let mut open: Option<usize> = None;
    for (i, line) in lines.iter().enumerate() {
        if FENCE.is_match(line) {
            match open {
                None => open = Some(i),
                Some(start) => {
                    for l in out.iter_mut().take(i + 1).skip(start) {
                        l.clear();
                    }
                    open = None;
                }
            }
        }
    }
    out
}

/// A checkbox line, verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkbox {
    pub line: String,
    pub checked: bool,
    /// 0-based line number in the description.
    pub line_no: usize,
}

fn checkbox_of(line: &str, line_no: usize) -> Option<Checkbox> {
    let caps = CHECKBOX.captures(line)?;
    Some(Checkbox {
        line: line.to_string(),
        checked: &caps[1] != " ",
        line_no,
    })
}

/// Every checkbox of a description, under any heading or none, outside
/// fenced blocks.
pub fn checkboxes(text: &str) -> Vec<Checkbox> {
    without_fences(text)
        .iter()
        .enumerate()
        .filter_map(|(i, l)| checkbox_of(l, i))
        .collect()
}

/// `(checked, open)` checkbox lines, verbatim.
pub fn checklist(text: &str) -> (Vec<String>, Vec<String>) {
    let mut checked = Vec::new();
    let mut open = Vec::new();
    for b in checkboxes(text) {
        if b.checked {
            checked.push(b.line);
        } else {
            open.push(b.line);
        }
    }
    (checked, open)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    pub level: usize,
    pub text: String,
    pub line_no: usize,
}

/// Parse one line as an ATX heading (up to three leading spaces, one to six
/// `#`, then a space, a tab or the end of the line).
pub fn heading_of(line: &str, line_no: usize) -> Option<Heading> {
    let caps = HEADING.captures(line)?;
    let level = caps[1].len();
    let mut text = caps
        .get(2)
        .map(|m| m.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    // A closing sequence of `#` preceded by a space is not part of the text.
    if let Some(stripped) = text.strip_suffix('#') {
        let trimmed = stripped.trim_end_matches('#');
        if trimmed.is_empty() || trimmed.ends_with([' ', '\t']) {
            text = trimmed.trim_end().to_string();
        }
    }
    Some(Heading {
        level,
        text,
        line_no,
    })
}

/// Headings of fence-blanked lines.
pub fn headings(lines: &[String]) -> Vec<Heading> {
    lines
        .iter()
        .enumerate()
        .filter_map(|(i, l)| heading_of(l, i))
        .collect()
}

/// A section: its heading and the lines up to the next heading of the same
/// or a shallower level (sub-headings belong to it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub heading: Heading,
    /// First body line (the line after the heading).
    pub start: usize,
    /// One past the last body line.
    pub end: usize,
    /// Body lines, fence-blanked.
    pub lines: Vec<String>,
}

impl Section {
    pub fn body(&self) -> String {
        self.lines.join("\n")
    }
}

/// Every section whose heading text satisfies `is_heading`, in order.
pub fn sections(text: &str, is_heading: impl Fn(&str) -> bool) -> Vec<Section> {
    let lines = without_fences(text);
    sections_of(&lines, is_heading)
}

pub fn sections_of(lines: &[String], is_heading: impl Fn(&str) -> bool) -> Vec<Section> {
    let hs = headings(lines);
    let mut out = Vec::new();
    for (k, h) in hs.iter().enumerate() {
        if !is_heading(&h.text) {
            continue;
        }
        let end = hs[k + 1..]
            .iter()
            .find(|n| n.level <= h.level)
            .map(|n| n.line_no)
            .unwrap_or(lines.len());
        out.push(Section {
            heading: h.clone(),
            start: h.line_no + 1,
            end,
            lines: lines[h.line_no + 1..end].to_vec(),
        });
    }
    out
}

/// Checkboxes inside sections whose heading satisfies `is_heading`, each
/// line counted once even when sections nest.
pub fn checkboxes_in_sections(text: &str, is_heading: impl Fn(&str) -> bool) -> Vec<Checkbox> {
    let lines = without_fences(text);
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for s in sections_of(&lines, is_heading) {
        for (i, line) in lines.iter().enumerate().take(s.end).skip(s.start) {
            if seen.insert(i) {
                if let Some(b) = checkbox_of(line, i) {
                    out.push(b);
                }
            }
        }
    }
    out
}

/// The first paragraph of a block of text: blank lines split paragraphs;
/// lines that are headings, comments, fences, tables or trailers are
/// dropped; the paragraph is joined with spaces and cut at `max` characters
/// on a word boundary, with ` …` (REQ-REL-093).
pub fn first_paragraph(text: &str, max: usize) -> String {
    let lines = without_fences(text);
    let mut paragraph: Vec<String> = Vec::new();
    for raw in &lines {
        let line = raw.trim();
        if line.is_empty() {
            if !paragraph.is_empty() {
                break;
            }
            continue;
        }
        let lower = line.to_lowercase();
        let noise = line.starts_with('#')
            || line.starts_with("<!--")
            || line.starts_with('\u{1F916}')
            || line.starts_with("```")
            || line.starts_with("~~~")
            || line.starts_with('|')
            || lower.starts_with("agent:")
            || lower.starts_with("co-authored-by:")
            || lower.starts_with("signed-off-by:");
        if noise {
            continue;
        }
        paragraph.push(line.to_string());
    }
    let joined = paragraph.join(" ");
    clip_words(&joined, max)
}

/// Cut on a word boundary at `max` characters, strip trailing ` ,;:` and
/// append ` …`.
pub fn clip_words(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let cut: String = text.chars().take(max).collect();
    let cut = match cut.rfind(' ') {
        Some(i) if i > 0 => cut[..i].to_string(),
        _ => cut,
    };
    format!("{} …", cut.trim_end_matches([' ', ',', ';', ':']))
}

/// Whether two texts are the same markdown once a tracker stored one of
/// them. A tracker rewrites what it keeps (Linear turns `- ` and `+ ` list
/// bullets into `* `, changes the spacing after a bullet, indents the lines
/// it nests, adds or drops blank lines, trims trailing spaces), so a
/// description differing only in that is not written again: the write
/// would change nothing the tracker keeps and come back restyled.
pub fn same_stored(a: &str, b: &str) -> bool {
    fn lines(text: &str) -> impl Iterator<Item = String> + '_ {
        text.lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(|l| match l.strip_prefix(['-', '+', '*']) {
                Some(item) if item.starts_with([' ', '\t']) => {
                    format!("* {}", item.trim_start())
                }
                _ => l.to_string(),
            })
    }
    lines(a).eq(lines(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_text_restyled_by_the_tracker_is_the_same_text() {
        let written = "## Release note\n- **ABC-1** — one\n+ two\n\n\n---\n";
        let stored = "## Release note\n\n* **ABC-1** — one  \n\n* two\n\n---";
        assert!(same_stored(written, stored));
        assert!(!same_stored(
            written,
            "## Release note\n* **ABC-1** — one\n"
        ));
        assert!(!same_stored("-not a bullet", "*not a bullet"));
        assert!(!same_stored("- one", "* two"));
        assert!(!same_stored("**bold**", "* *bold**"));
    }

    #[test]
    fn a_text_whose_lines_the_tracker_indented_or_respaced_is_the_same_text() {
        let written = "- **ABC-1** — one\n- **ABC-2** — two\n`spl-asdlc:release-note:end`\n";
        let stored = "*   **ABC-1** — one\n\n* **ABC-2** — two\n  `spl-asdlc:release-note:end`";
        assert!(same_stored(written, stored));
        assert!(!same_stored(
            written,
            "* **ABC-1** — one\n* **ABC-2** — two\n"
        ));
    }

    #[test]
    fn completion_checklist_items_reads_a_box_under_any_heading() {
        let text = "## Goal\nx\n- [ ] under goal\n## Notes\n- [x] under notes\n- [ ] no heading above? still counts\n";
        let (checked, open) = checklist(text);
        assert_eq!(checked, vec!["- [x] under notes"]);
        assert_eq!(open.len(), 2);
        assert_eq!(checklist("- [ ] alone\n").1, vec!["- [ ] alone"]);
    }

    #[test]
    fn completion_checklist_items_reads_every_markdown_bullet_spelling() {
        let text = "- [ ] dash\n* [x] star\n+ [X] plus\n1. [ ] dot\n2) [ ] paren\n-   [ ] spaces\n\t- [ ] tab indent\n-[ ] no space is not a box\n- [] empty is not a box\n";
        let (checked, open) = checklist(text);
        assert_eq!(checked, vec!["* [x] star", "+ [X] plus"]);
        assert_eq!(
            open,
            vec![
                "- [ ] dash",
                "1. [ ] dot",
                "2) [ ] paren",
                "-   [ ] spaces",
                "\t- [ ] tab indent"
            ]
        );
    }

    #[test]
    fn completion_checklist_items_ignores_a_box_inside_a_fenced_block() {
        let text = "- [ ] real\n```markdown\n- [ ] quoted\n```\n~~~\n- [x] quoted too\n~~~\n- [x] real too\n";
        let (checked, open) = checklist(text);
        assert_eq!(open, vec!["- [ ] real"]);
        assert_eq!(checked, vec!["- [x] real too"]);
    }

    #[test]
    fn an_unclosed_fence_hides_nothing() {
        let text = "- [ ] before\n```\n- [ ] after a stray fence\n";
        assert_eq!(checklist(text).1.len(), 2);
    }

    #[test]
    fn a_fence_closes_with_any_fence_line() {
        let text = "```\n- [ ] in\n~~~~~\n- [ ] out\n";
        assert_eq!(checklist(text).1, vec!["- [ ] out"]);
        let indented = "    ```\n- [ ] four spaces is not a fence\n";
        assert_eq!(checklist(indented).1.len(), 1);
    }

    #[test]
    fn fenced_blocks_keep_the_line_count() {
        let text = "a\n```\nb\n```\nc";
        let lines = without_fences(text);
        assert_eq!(lines, vec!["a", "", "", "", "c"]);
    }

    #[test]
    fn headings_need_a_space_and_at_most_three_leading_spaces() {
        assert_eq!(heading_of("## Goal", 0).unwrap().text, "Goal");
        assert_eq!(heading_of("   ### Goal  ", 0).unwrap().level, 3);
        assert_eq!(heading_of("##\tGoal", 0).unwrap().text, "Goal");
        assert_eq!(heading_of("##", 0).unwrap().text, "");
        assert_eq!(heading_of("## Goal ##", 0).unwrap().text, "Goal");
        assert_eq!(heading_of("## C#", 0).unwrap().text, "C#");
        assert!(heading_of("##Goal", 0).is_none());
        assert!(heading_of("    ## Goal", 0).is_none());
        assert!(heading_of("####### seven", 0).is_none());
        assert!(heading_of("**Goal**", 0).is_none());
    }

    #[test]
    fn a_section_runs_to_the_next_heading_of_its_level_and_keeps_its_sub_headings() {
        let text = "## A\none\n### A.1\ntwo\n## B\nthree\n# C\n";
        let s = sections(text, |t| t == "A");
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].lines, vec!["one", "### A.1", "two"]);
        let b = sections(text, |t| t == "B");
        assert_eq!(b[0].lines, vec!["three"]);
    }

    #[test]
    fn a_heading_inside_a_fence_is_not_a_section() {
        let text = "```\n## Post deploy\n- [ ] x\n```\n";
        assert!(sections(text, |t| t == "Post deploy").is_empty());
    }

    #[test]
    fn nested_matching_sections_count_each_box_once() {
        let text = "# Post deploy\n- [ ] a\n## Post deploy\n- [ ] b\n";
        let boxes = checkboxes_in_sections(text, |t| t == "Post deploy");
        assert_eq!(boxes.len(), 2);
    }

    #[test]
    fn the_first_paragraph_skips_noise_and_is_cut_on_a_word() {
        let text =
            "## Summary\n<!-- hidden -->\n| a | b |\nFirst line\nsecond line.\n\nNext paragraph.";
        assert_eq!(first_paragraph(text, 280), "First line second line.");
        let long = "word ".repeat(100);
        let cut = first_paragraph(&long, 20);
        assert!(cut.ends_with(" …"), "{cut}");
        assert!(cut.chars().count() <= 22);
        assert_eq!(
            first_paragraph("Co-Authored-By: x\nSigned-off-by: y\nReal", 280),
            "Real"
        );
    }
}
