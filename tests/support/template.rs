//! The configuration template's layers, added as its comments say: the
//! template is light as written (the release cycle with no QA angle, no PR
//! review), and each function below makes the edit its comments tell a
//! project to make for a layer. Shared by the setup-file tests
//! and the workflow example tests, which check the example callers against
//! the full configuration.

#![allow(dead_code)]

use std::path::PathBuf;

pub const PATH: &str = "docs/setup/templates/config.toml";

/// The template as written: the configuration with no optional step.
pub fn light() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(PATH);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{PATH}: {e}"))
}

/// The one line that starts with `start` and contains `marker`.
fn find(lines: &[String], start: &str, marker: &str) -> Option<usize> {
    let at: Vec<usize> = (0..lines.len())
        .filter(|&i| lines[i].starts_with(start) && lines[i].contains(marker))
        .collect();
    assert!(
        at.len() <= 1,
        "{start:?} … {marker:?} more than once in {PATH}"
    );
    at.first().copied()
}

fn edit(text: &str, f: impl FnOnce(&mut Vec<String>)) -> String {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    f(&mut lines);
    lines.join("\n") + "\n"
}

/// Uncomment the block of `# ` lines that starts at the one line that starts
/// with `start` and contains `marker`.
fn uncomment_block(text: &str, start: &str, marker: &str) -> String {
    edit(text, |lines| {
        let mut i = find(lines, start, marker).unwrap_or_else(|| panic!("no {start:?}"));
        while i < lines.len() && lines[i].starts_with("# ") {
            lines[i] = lines[i][2..].to_string();
            i += 1;
        }
    })
}

/// Uncomment the one line that starts with `start` and contains `marker`.
fn uncomment_line(text: &str, start: &str, marker: &str) -> String {
    edit(text, |lines| {
        let i = find(lines, start, marker).unwrap_or_else(|| panic!("no {start:?}"));
        lines[i] = lines[i][2..].to_string();
    })
}

/// Remove the one line `angles = []` whose comment contains `marker`.
fn remove_empty_angles(text: &str, marker: &str) -> String {
    edit(text, |lines| {
        let i = find(lines, "angles = []", marker).expect("angles = []");
        lines.remove(i);
    })
}

/// Layer 1: PR review, an angle on every PR and one on its paths.
pub fn pr_review(text: &str) -> String {
    let t = remove_empty_angles(text, "layer 1");
    let t = uncomment_line(&t, "# coordinators =", "layer 1");
    uncomment_block(&t, "# [[review.angles]]", "every PR")
}

/// Layer 2: release QA angles.
pub fn release_angles(text: &str) -> String {
    let t = remove_empty_angles(text, "layer 2");
    uncomment_block(&t, "# coordinators =", "angle PR")
}

/// Every layer: the full configuration.
pub fn full() -> String {
    release_angles(&pr_review(&light()))
}
