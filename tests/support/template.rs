//! The configuration template's layers, added as its comments say: the
//! template is light as written, and each function below makes the edit its
//! "Layers" section tells a project to make. Shared by the setup-file tests
//! and the workflow example tests, which check the example callers against
//! the full configuration.

#![allow(dead_code)]

use std::path::PathBuf;

pub const PATH: &str = "docs/setup/templates/spl-asdlc.toml";

/// The template as written: the light configuration.
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

/// Layer 1: one PR-review angle.
pub fn pr_review(text: &str) -> String {
    let t = remove_empty_angles(text, "layer 1");
    let t = uncomment_line(&t, "# coordinators =", "layers 1 and 4");
    uncomment_block(&t, "# [[review.angles]]", "")
}

/// Layer 2: the release cycle with its gate, and no QA angle.
pub fn release_cycle(text: &str) -> String {
    uncomment_block(text, "# [release]", "")
}

/// Layer 3: release QA angles, on top of layer 2.
pub fn release_angles(text: &str) -> String {
    let t = remove_empty_angles(text, "layer 3");
    uncomment_block(&t, "# coordinators =", "angle PR")
}

/// Layer 4: sensitive paths, with the coordinators they need.
pub fn sensitive_paths(text: &str) -> String {
    let t = uncomment_line(text, "# sensitive_paths =", "");
    let commented = t
        .lines()
        .any(|l| l.starts_with("# coordinators =") && l.contains("layers 1 and 4"));
    if commented {
        uncomment_line(&t, "# coordinators =", "layers 1 and 4")
    } else {
        t
    }
}

/// Every layer: the full configuration.
pub fn full() -> String {
    sensitive_paths(&release_angles(&release_cycle(&pr_review(&light()))))
}
