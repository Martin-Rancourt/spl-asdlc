//! The method's protocols, compiled into the binary, that the bridge embeds
//! in every item it serves to an agent (BR-ASDLC-015): an agent never
//! depends on reading the method's repository to know what to do. An item
//! carries the protocol of the bridge version that created it, and keeps it:
//! an item found again by its marker is never rewritten (but for its marker
//! written back, `crate::tick::identity`).
//!
//! A protocol is Markdown with its own headings. It is embedded under the
//! item's `## Protocol` section with every heading demoted below it, so the
//! contract's sections (`## Acceptance criteria`, `## Business rules`) stay
//! the item's own.

use crate::method::markdown;

/// `docs/agents/review-pr.md`: review a PR from one angle.
pub const REVIEW_PR: &str = include_str!("../../../docs/agents/review-pr.md");
/// `docs/agents/review-release.md`: review a release candidate from one angle.
pub const REVIEW_RELEASE: &str = include_str!("../../../docs/agents/review-release.md");
/// `docs/agents/consolidate.md`: consolidate a PR review or a release QA.
pub const CONSOLIDATE: &str = include_str!("../../../docs/agents/consolidate.md");
/// `docs/reference/06-qa-report.md`: the report every release angle ends with.
const QA_REPORT: &str = include_str!("../../../docs/reference/06-qa-report.md");

/// The method's notes of each angle, `docs/agents/angles/<key>.md`, by key.
pub const ANGLES: &[(&str, &str)] = &[
    (
        "design",
        include_str!("../../../docs/agents/angles/design.md"),
    ),
    (
        "maintainability",
        include_str!("../../../docs/agents/angles/maintainability.md"),
    ),
    (
        "product",
        include_str!("../../../docs/agents/angles/product.md"),
    ),
    (
        "security",
        include_str!("../../../docs/agents/angles/security.md"),
    ),
];

/// The size an item's description is held under. Neither adapter states a
/// tracker limit, so this is a conservative bound below the descriptions
/// either tracker keeps; a test holds the longest item under it, and a
/// protocol that outgrows it is trimmed in its file, never cut here.
pub const ITEM_LIMIT: usize = 60_000;

/// The version of the bridge whose protocols an item carries.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The method's notes of an angle, `None` for an angle the method does not
/// know (a project's own).
pub fn angle_notes(key: &str) -> Option<&'static str> {
    ANGLES.iter().find(|(k, _)| *k == key).map(|(_, t)| *t)
}

/// The kind of job a protocol section is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Job {
    /// An angle of a PR review.
    PrAngle,
    /// An angle of a release candidate's QA.
    ReleaseAngle,
    /// The consolidation of a PR review.
    PrConsolidation,
    /// The consolidation of a release candidate's QA.
    ReleaseConsolidation,
}

/// The `## Protocol` section of an item: the protocol of its job, the
/// method's notes of its angle (or a line saying there are none), and the
/// report's schema for a release job.
pub fn section(job: Job, angle: Option<&str>) -> String {
    let (base, path) = match job {
        Job::PrAngle => (REVIEW_PR, "docs/agents/review-pr.md"),
        Job::ReleaseAngle => (REVIEW_RELEASE, "docs/agents/review-release.md"),
        Job::PrConsolidation | Job::ReleaseConsolidation => {
            (CONSOLIDATE, "docs/agents/consolidate.md")
        }
    };
    let mut sources = vec![format!("`{path}`")];
    let mut parts = vec![demote(base, 2)];
    if let Some(key) = angle {
        match angle_notes(key) {
            Some(notes) => {
                sources.push(format!("`docs/agents/angles/{key}.md`"));
                parts.push(demote(notes, 2));
            }
            None => parts.push(format!(
                "### Angle: {key}\n\nThe method has no notes for the `{key}` angle (an angle of the \
                 project's own): review under the protocol above, from what the angle's name says, \
                 with the project's notes when the context of this item links them."
            )),
        }
    }
    if matches!(job, Job::ReleaseAngle | Job::ReleaseConsolidation) {
        sources.push("the schema and rules of `docs/reference/06-qa-report.md`".to_string());
        parts.push(format!("### The angle report\n\n{}", report_schema()));
    }
    format!(
        "## Protocol\n\nEmbedded by spl-asdlc {VERSION} when it created this item, and frozen with it: {}. \
         It is the whole protocol of this job: no file of the method's repository is needed. {}\n\n{}\n",
        sources.join(", "),
        super::KEEP_MARKER,
        parts
            .iter()
            .map(|p| p.trim())
            .collect::<Vec<_>>()
            .join("\n\n")
    )
}

/// The `## Schema` and `## Rules` sections of the report spec, demoted
/// under `### The angle report`.
fn report_schema() -> String {
    let lines: Vec<&str> = QA_REPORT.lines().collect();
    let wanted = |h: &str| h.eq_ignore_ascii_case("Schema") || h.eq_ignore_ascii_case("Rules");
    markdown::sections(QA_REPORT, wanted)
        .iter()
        .map(|s| {
            let text = lines[s.heading.line_no..s.end].join("\n");
            demote(text.trim(), 2)
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Every heading of `text` outside fenced blocks moved `by` levels down; a
/// heading that would pass level six becomes a bold line.
pub fn demote(text: &str, by: usize) -> String {
    let blanked = markdown::without_fences(text);
    text.lines()
        .zip(blanked.iter())
        .enumerate()
        .map(
            |(i, (line, seen))| match markdown::heading_of(seen, i).filter(|_| by > 0) {
                Some(h) if h.level + by <= 6 => format!("{} {}", "#".repeat(h.level + by), h.text),
                Some(h) => format!("**{}**", h.text),
                None => line.to_string(),
            },
        )
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::method::contract;

    #[test]
    fn every_angle_file_of_the_method_is_compiled_in() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/agents/angles");
        let mut on_disk: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| {
                let name = e.unwrap().file_name().into_string().unwrap();
                name.strip_suffix(".md").map(str::to_string)
            })
            .collect();
        on_disk.sort();
        let compiled: Vec<String> = ANGLES.iter().map(|(k, _)| k.to_string()).collect();
        assert_eq!(compiled, on_disk, "docs/agents/angles/ and ANGLES differ");
    }

    #[test]
    fn a_protocol_carries_no_marker_and_no_contract_heading() {
        let schema = report_schema();
        let texts = [REVIEW_PR, REVIEW_RELEASE, CONSOLIDATE, schema.as_str()]
            .into_iter()
            .map(str::to_string)
            .chain(ANGLES.iter().map(|(_, t)| t.to_string()))
            .collect::<Vec<_>>();
        for t in &texts {
            assert!(!t.contains("<!--"), "{t}");
            assert!(!t.contains("spl-asdlc:"), "{t}");
            assert!(markdown::checkboxes(t).is_empty(), "no box: {t}");
            for h in markdown::headings(&markdown::without_fences(t)) {
                assert!(
                    !contract::is_goal_heading(&h.text)
                        && !contract::is_acceptance_heading(&h.text)
                        && !contract::is_business_rules_heading(&h.text)
                        && !contract::is_post_deploy_heading(&h.text)
                        && !contract::is_release_note_heading(&h.text),
                    "{}",
                    h.text
                );
            }
        }
    }

    #[test]
    fn headings_are_demoted_outside_fences_only() {
        let text = "# Title\nx\n## Part\n```\n# not a heading\n```\n###### Deep";
        assert_eq!(
            demote(text, 2),
            "### Title\nx\n#### Part\n```\n# not a heading\n```\n**Deep**"
        );
    }

    #[test]
    fn the_report_schema_is_embedded_whole() {
        let s = report_schema();
        assert!(s.starts_with("#### Schema"), "{s}");
        assert!(s.contains("\"schema\": \"spl-asdlc/qa-report/2\""), "{s}");
        assert!(s.contains("#### Rules"), "{s}");
        assert!(s.contains("`checked` is never empty"), "{s}");
        assert!(!s.contains("## Prompts"), "{s}");
    }

    #[test]
    fn a_release_angle_names_its_sub_item_in_the_report_never_a_person() {
        let s = section(Job::ReleaseAngle, Some("security"));
        assert!(
            s.contains("\"item\": \"<the key of the angle sub-item"),
            "{s}"
        );
        assert!(!s.contains("\"reviewer\":"), "{s}");
        assert!(
            s.contains("Its `item` is your sub-item's key")
                && s.contains("never your name or anyone's"),
            "{s}"
        );
    }

    #[test]
    fn a_project_angle_gets_the_generic_protocol_and_says_the_method_has_no_notes() {
        let s = section(Job::PrAngle, Some("accessibility"));
        assert!(s.contains("### Review a PR from one angle"), "{s}");
        assert!(
            s.contains("The method has no notes for the `accessibility` angle"),
            "{s}"
        );
        assert!(!s.contains("docs/agents/angles/accessibility.md"), "{s}");
    }
}
