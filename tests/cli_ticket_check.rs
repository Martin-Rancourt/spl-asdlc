//! `spl-asdlc ticket check` on a description file: the refinement gate as a
//! user runs it.

use std::path::PathBuf;
use std::process::Command;

fn write_temp(name: &str, text: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("spl-asdlc-ticket-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    path
}

fn check(path: &std::path::Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_spl-asdlc"))
        .args(["ticket", "check", "--description"])
        .arg(path)
        .output()
        .unwrap()
}

#[test]
fn a_complete_contract_exits_zero() {
    let p = write_temp(
        "complete.md",
        "## Goal\nIt works.\n\n## Acceptance criteria\n- [ ] it works\n\n## Business rules\nNo new rule.\n",
    );
    let out = check(&p);
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).contains("the contract is complete"));
}

#[test]
fn an_incomplete_contract_exits_one_and_lists_what_is_missing() {
    let p = write_temp("incomplete.md", "## Goal\nIt works.\n");
    let out = check(&p);
    assert_eq!(out.status.code(), Some(1));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("no `## Acceptance criteria` section"),
        "{text}"
    );
    assert!(text.contains("no `## Business rules` section"), "{text}");
}

#[test]
fn an_unreadable_file_or_no_ticket_exits_two() {
    let out = check(std::path::Path::new("/nonexistent/description.md"));
    assert_eq!(out.status.code(), Some(2));
    let none = Command::new(env!("CARGO_BIN_EXE_spl-asdlc"))
        .args(["ticket", "check"])
        .output()
        .unwrap();
    assert_eq!(none.status.code(), Some(2));
}
