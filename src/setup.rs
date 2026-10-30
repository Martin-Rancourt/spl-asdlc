//! The files of a project's setup PR, from a host's answers
//! (`docs/setup/README.md`): the configuration's shape, the workflow
//! callers pinned at the minor line of the embedded version, the process
//! page of that version and the agent file. The host opens the PR with its
//! own GitHub identity: the workflow files need "Workflows: write", which the
//! bridge's App does not hold (`docs/setup/github-app.md`). The App, its
//! secrets and the rulesets stay a guided step of the person; `doctor`
//! checks them.
//!
//! The configuration is the light one (no PR review, no release QA angle)
//! unless a layer is asked for, and carries no identity: the host's
//! instance overlay holds the repository, the tracker's team and project,
//! the owner and the coordinators (spec 08, "The instance overlay").

use serde::Serialize;

use crate::config::{DEFAULT_PATH, TrackerKind, is_angle_key, is_product_name};
use crate::embed::{VERSION, minor_line};
use crate::github::RepoSlug;

/// What the host asks the person before the setup PR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answers {
    /// The product's name (`product.name`): lower-case letters, digits and
    /// hyphens; it names the tags (`<name>-v1.2.0`).
    pub product: String,
    /// The project's repository, `<owner>/<repo>`.
    pub repository: String,
    pub tracker: TrackerKind,
    /// The PR review layer: its angles, each asked by its ticket label or
    /// brought by its paths;
    /// empty for no PR review.
    pub review_angles: Vec<ReviewAngle>,
    /// The release QA layer: the keys of the release's angles; empty for
    /// none, the GO resting on the release CI.
    pub release_angles: Vec<String>,
    /// The project's test command, run on every PR and as the gate's suite.
    pub test_command: String,
    /// The slug of the bridge's GitHub App, the one publisher of a GO.
    pub app_slug: String,
    /// The method's repository the callers name; `None` for this crate's.
    pub method_repository: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewAngle {
    pub key: String,
    /// Empty: only on a ticket labelled `angle/<key>`; else also on a PR
    /// that touches one of them (`*` for every PR).
    pub paths: Vec<String>,
}

/// The setup PR: its branch, title and body, and its files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SetupPr {
    pub branch: String,
    pub title: String,
    pub body: String,
    pub files: Vec<SetupFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SetupFile {
    /// From the repository's root.
    pub path: String,
    pub content: String,
}

/// The method's repository of this crate (`Cargo.toml`'s `repository`).
pub fn method_repository() -> String {
    env!("CARGO_PKG_REPOSITORY")
        .trim_start_matches("https://github.com/")
        .trim_end_matches('/')
        .to_string()
}

const PROCESS: &str = include_str!("../docs/agents/process.md");
const AGENTS: &str = include_str!("../docs/setup/templates/AGENTS.md");
const EXAMPLES: [(&str, &str); 5] = [
    ("pr.yml", include_str!("../docs/setup/examples/pr.yml")),
    (
        "release-cut.yml",
        include_str!("../docs/setup/examples/release-cut.yml"),
    ),
    (
        "release-gate.yml",
        include_str!("../docs/setup/examples/release-gate.yml"),
    ),
    (
        "release-gate-auto-rerun.yml",
        include_str!("../docs/setup/examples/release-gate-auto-rerun.yml"),
    ),
    (
        "release-publish.yml",
        include_str!("../docs/setup/examples/release-publish.yml"),
    ),
];

/// The setup PR of a project, or why the answers cannot make one.
pub fn files(answers: &Answers) -> Result<SetupPr, String> {
    let a = answers;
    if !is_product_name(&a.product) {
        return Err(format!(
            "product \"{}\" must be lower-case letters, digits and hyphens",
            a.product
        ));
    }
    if RepoSlug::parse(&a.repository).is_none() {
        return Err(format!(
            "repository \"{}\" is not <owner>/<repo>",
            a.repository
        ));
    }
    let method = a
        .method_repository
        .clone()
        .unwrap_or_else(method_repository);
    if RepoSlug::parse(&method).is_none() {
        return Err(format!(
            "method repository \"{method}\" is not <owner>/<repo>"
        ));
    }
    for key in a
        .review_angles
        .iter()
        .map(|r| &r.key)
        .chain(&a.release_angles)
    {
        if !is_angle_key(key) {
            return Err(format!(
                "angle \"{key}\" must be lower-case letters, digits and hyphens"
            ));
        }
    }
    let command = a.test_command.trim();
    if command.is_empty() || command.contains('\n') {
        return Err("the test command must be one non-empty line".into());
    }
    if a.app_slug.is_empty()
        || !a
            .app_slug
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err(format!("\"{}\" is not a GitHub App slug", a.app_slug));
    }
    let line = minor_line(VERSION);
    let mut files = vec![SetupFile {
        path: DEFAULT_PATH.to_string(),
        content: config(a),
    }];
    for (name, example) in EXAMPLES {
        files.push(SetupFile {
            path: format!(".github/workflows/{name}"),
            content: caller(name, example, a, &method, &line, command),
        });
    }
    files.push(SetupFile {
        path: ".spl-asdlc/process.md".to_string(),
        content: PROCESS.to_string(),
    });
    files.push(SetupFile {
        path: "AGENTS.md".to_string(),
        content: AGENTS.replacen(
            "# Working on acme",
            &format!("# Working on {}", a.product),
            1,
        ),
    });
    let layers = match (a.review_angles.is_empty(), a.release_angles.is_empty()) {
        (true, true) => "the light configuration: no PR review, no release QA angle".to_string(),
        (false, true) => "PR review, no release QA angle".to_string(),
        (true, false) => "no PR review, release QA angles".to_string(),
        (false, false) => "PR review and release QA angles".to_string(),
    };
    let body = format!(
        "Sets up SPL-ASDLC for this repository, at {line} of `{method}` (spl-asdlc {VERSION}): {layers}.\n\n\
         - `.spl-asdlc/config.toml`: the method's shape; the identities (repository, tracker, owner{coordinators}) stay in the host's instance overlay.\n\
         - `.github/workflows/`: the workflow callers, pinned at `@{line}`; the cut runs the published binary of {line}.\n\
         - `.spl-asdlc/process.md`: the process page of spl-asdlc {VERSION}, never edited here.\n\
         - `AGENTS.md`: points the agents to the process page; the project's own instructions go under \"This project\".\n\n\
         Before the method runs, the person gives the repository the App's secrets and the rulesets (`docs/setup/github-app.md`, `docs/setup/repository.md`); `doctor` checks each step.\n",
        coordinators = if a.review_angles.is_empty() && a.release_angles.is_empty() {
            ""
        } else {
            ", coordinators"
        },
    );
    Ok(SetupPr {
        branch: "spl-asdlc/setup".to_string(),
        title: format!("chore: set up SPL-ASDLC {line}"),
        body,
        files,
    })
}

/// The repository's configuration: the method's shape only.
fn config(a: &Answers) -> String {
    let product = &a.product;
    let mut out = format!(
        "# .spl-asdlc/config.toml: the configuration of this repository\n\
         # (docs/reference/08-configuration.md), set up with spl-asdlc {VERSION}.\n\
         # The method's shape only: the identities of the installation (the\n\
         # repository, the tracker's team and project, the owner, the\n\
         # coordinators) are in the instance overlay of the host that runs the\n\
         # bridge. Each optional layer is an edit of this file\n\
         # (docs/setup/README.md, \"Optional steps\").\n\
         \n\
         schema = 1\n\
         \n\
         [product]\n\
         name = \"{product}\"\n\
         \n\
         [tracker]\n\
         kind = \"{}\"\n",
        a.tracker
    );
    if a.tracker == TrackerKind::Livela {
        out.push_str(
            "# A PR names its item as livela://item/<id> in its body: the bridge links it.\n\
             link_fallback = \"body-url\"\n",
        );
    }
    out.push_str("\n[review]\nrequire_up_to_date = false\n");
    if a.review_angles.is_empty() {
        out.push_str(
            "angles = []                            # no PR review: a PR merges on its CI\n",
        );
    }
    for angle in &a.review_angles {
        out.push_str(&format!("\n[[review.angles]]\nkey = \"{}\"\n", angle.key));
        if !angle.paths.is_empty() {
            out.push_str(&format!(
                "paths = [{}]\n",
                angle
                    .paths
                    .iter()
                    .map(|p| toml_string(p))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    out.push_str(&format!(
        "\n[release]\n\
         candidate_tag = \"{product}-v{{version}}-rc.{{rc}}\"\n\
         release_tag = \"{product}-v{{version}}\"\n"
    ));
    if a.release_angles.is_empty() {
        out.push_str("angles = []                            # no QA angle: the GO rests on the release CI\n");
    }
    for key in &a.release_angles {
        out.push_str(&format!("\n[[release.angles]]\nkey = \"{key}\"\n"));
    }
    out
}

fn toml_string(s: &str) -> String {
    toml::Value::String(s.to_string()).to_string()
}

/// `from` replaced by `to` in `text`, where the example has it: a change of
/// the example that drops it fails the crate's tests, never a project's CI.
fn edit(text: &mut String, from: &str, to: &str) {
    assert!(text.contains(from), "the example lacks {from:?}");
    *text = text.replace(from, to);
}

/// An example caller made the project's: the method's repository and the
/// minor line, the product's tag patterns, the test command, and the
/// layers chosen.
fn caller(
    name: &str,
    example: &str,
    a: &Answers,
    method: &str,
    line: &str,
    command: &str,
) -> String {
    // The example's header says how to copy it: the generated file's own
    // line replaces it.
    let (head, rest) = example.split_once("\n\n").expect("the example's name line");
    let body = match rest.find("\n\n") {
        Some(i) if rest.starts_with('#') => &rest[i + 2..],
        _ => rest,
    };
    let mut text = format!(
        "{head}\n\n# Set up by spl-asdlc {VERSION} from docs/setup/examples/{name}, pinned at {line}.\n\n{body}"
    );
    edit(&mut text, "<owner>/spl-asdlc", method);
    edit(&mut text, ".yml@v1\n", &format!(".yml@{line}\n"));
    match name {
        "pr.yml" => {
            edit(&mut text, "@v1\n", &format!("@{line}\n"));
            edit(
                &mut text,
                "guard-commands: '[{\"name\": \"lint\", \"run\": \"make lint\"}]'",
                "guard-commands: '[]'",
            );
            edit(
                &mut text,
                "        run: make test\n",
                &format!("        run: |\n          {command}\n"),
            );
        }
        "release-cut.yml" => {
            edit(&mut text, "acme-v", &format!("{}-v", a.product));
            edit(
                &mut text,
                "spl-asdlc-version: latest",
                &format!("spl-asdlc-version: {line}"),
            );
        }
        "release-gate.yml" => {
            edit(&mut text, "acme-v", &format!("{}-v", a.product));
            // No build of its own before the suite: the test command is the
            // suite.
            let start = text
                .find("  # The project's own build on the exact commit")
                .expect("the example's preflight");
            let end = text.find("  gate:\n").expect("the example's gate");
            text.replace_range(start..end, "");
            edit(&mut text, "    needs: preflight\n    if: always()\n", "");
            edit(
                &mut text,
                "      upstream-result: ${{ needs.preflight.result }}\n",
                "",
            );
            let run =
                serde_json::to_string(&format!("{command} 2>&1 | tee \"$EVIDENCE_DIR/tests.log\""))
                    .expect("a string serialises");
            edit(
                &mut text,
                "[{\"id\": \"tests\", \"run\": \"make test 2>&1 | tee \\\"$EVIDENCE_DIR/tests.log\\\"\",",
                &format!("[{{\"id\": \"tests\", \"run\": {run},"),
            );
            edit(
                &mut text,
                "guard-commands: '[{\"name\": \"lint\", \"run\": \"make lint\"}]'",
                "guard-commands: '[]'",
            );
        }
        "release-publish.yml" => {
            edit(&mut text, "acme-v", &format!("{}-v", a.product));
            edit(
                &mut text,
                "qa-angles: '[\"security\", \"product\"]'",
                &format!(
                    "qa-angles: '{}'",
                    serde_json::to_string(&a.release_angles).expect("strings serialise")
                ),
            );
            edit(
                &mut text,
                "go-publisher-login: example-app",
                &format!("go-publisher-login: {}", a.app_slug),
            );
            edit(
                &mut text,
                "      - name: Publish\n        run: make publish\n      - name: Deploy\n        run: make deploy\n",
                "      - name: Publish\n        run: |\n          echo \"Nothing is published yet: this job is the project's publication and deployment.\"\n",
            );
        }
        _ => {}
    }
    text
}

#[cfg(test)]
mod tests;
