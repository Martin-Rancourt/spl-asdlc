//! The version of the method a project is pinned on (BR-ASDLC-032): the ref
//! its workflow callers name, `<owner>/<repo>/.github/workflows/asdlc-<unit>.yml@<ref>`.
//! The angle and protocol texts the bridge copies into the project's items
//! are read from that repository at that ref, so one update of the callers
//! brings the new workflows and the new texts together. Pure: the caller
//! reads the workflow files.

use std::fmt;

use crate::github::RepoSlug;
use crate::method::workflow_callers::reusable_callers;

/// The directory of a repository's workflows.
pub const WORKFLOWS: &str = ".github/workflows";

/// Where the method's texts of a project are read: the method's repository
/// and the ref its workflow callers name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pin {
    pub repo: RepoSlug,
    pub rev: String,
}

impl fmt::Display for Pin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.repo, self.rev)
    }
}

/// Why no pin could be told from the workflow files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PinError {
    /// No job calls a reusable workflow `asdlc-*.yml` of another repository.
    NoCaller,
    /// The callers name more than one version: each `<pin> (<files>)`.
    Several(Vec<String>),
}

impl fmt::Display for PinError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PinError::NoCaller => write!(
                f,
                "no workflow under {WORKFLOWS} calls a reusable workflow of the method \
                 (`<owner>/spl-asdlc/{WORKFLOWS}/asdlc-<unit>.yml@<ref>`): the version of the \
                 method the project is pinned on cannot be told"
            ),
            PinError::Several(pins) => write!(
                f,
                "the workflow callers pin the method on more than one version ({}): pin every \
                 caller on the same one",
                pins.join(", ")
            ),
        }
    }
}

/// The pin of a project from its workflow files, `(file name, text)`: the
/// one `<owner>/<repo>@<ref>` every job calling a reusable workflow
/// `asdlc-*.yml` of another repository names. A local call
/// (`./.github/workflows/…`) and a call of any other workflow are left out.
/// Fail closed: no such caller, or callers on two versions, tells no pin.
pub fn resolve(workflows: &[(String, String)]) -> Result<Pin, PinError> {
    let mut found: Vec<(Pin, Vec<String>)> = Vec::new();
    for (file, text) in workflows {
        for caller in reusable_callers(text) {
            let Some(pin) = pin_of(&caller.uses) else {
                continue;
            };
            match found.iter_mut().find(|(p, _)| *p == pin) {
                Some((_, files)) if files.contains(file) => {}
                Some((_, files)) => files.push(file.clone()),
                None => found.push((pin, vec![file.clone()])),
            }
        }
    }
    match found.len() {
        0 => Err(PinError::NoCaller),
        1 => Ok(found.remove(0).0),
        _ => Err(PinError::Several(
            found
                .iter()
                .map(|(p, files)| format!("{p} in {}", files.join(", ")))
                .collect(),
        )),
    }
}

/// The pin a `uses` names, when it calls a reusable workflow `asdlc-*.yml`
/// of another repository at a ref.
fn pin_of(uses: &str) -> Option<Pin> {
    let (path, rev) = uses.rsplit_once('@')?;
    let mut parts = path.splitn(3, '/');
    let (owner, name, rest) = (parts.next()?, parts.next()?, parts.next()?);
    let file = rest.strip_prefix(".github/workflows/")?;
    if file.contains('/') || !file.starts_with("asdlc-") || rev.is_empty() || rev.contains("${{") {
        return None;
    }
    Some(Pin {
        repo: RepoSlug::parse(&format!("{owner}/{name}"))?,
        rev: rev.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wf(name: &str, text: &str) -> (String, String) {
        (name.to_string(), text.to_string())
    }

    fn caller(unit: &str, at: &str) -> String {
        format!(
            "on: push\njobs:\n  {unit}:\n    uses: acme/spl-asdlc/.github/workflows/asdlc-{unit}.yml@{at}\n"
        )
    }

    #[test]
    fn the_pin_is_the_one_ref_every_caller_of_the_method_names() {
        let pin = resolve(&[
            wf("pr.yml", &caller("pr-guards", "v1")),
            wf("pr-title.yml", &caller("pr-title", "v1")),
            wf(
                "ci.yml",
                "on: push\njobs:\n  local:\n    uses: ./.github/workflows/asdlc-own.yml\n  other:\n    uses: acme/tools/.github/workflows/lint.yml@main\n",
            ),
        ])
        .unwrap();
        assert_eq!(pin.to_string(), "acme/spl-asdlc@v1");
        assert_eq!(pin.rev, "v1");
    }

    #[test]
    fn an_exact_tag_or_a_sha_is_a_pin_too() {
        let pin = resolve(&[wf("cut.yml", &caller("release-cut", "v1.3.0"))]).unwrap();
        assert_eq!(pin.rev, "v1.3.0");
        let sha = "0123456789abcdef0123456789abcdef01234567";
        let pin = resolve(&[wf("cut.yml", &caller("release-cut", sha))]).unwrap();
        assert_eq!(pin.rev, sha);
    }

    #[test]
    fn no_caller_or_two_versions_tell_no_pin() {
        assert_eq!(
            resolve(&[wf("ci.yml", "on: push\njobs:\n  t:\n    runs-on: x\n")]),
            Err(PinError::NoCaller)
        );
        assert_eq!(resolve(&[]), Err(PinError::NoCaller));
        let err = resolve(&[
            wf("pr.yml", &caller("pr-guards", "v1")),
            wf("cut.yml", &caller("release-cut", "v1.2.0")),
        ])
        .unwrap_err();
        assert!(matches!(err, PinError::Several(_)), "{err:?}");
        let said = err.to_string();
        assert!(said.contains("acme/spl-asdlc@v1 in pr.yml"), "{said}");
        assert!(said.contains("acme/spl-asdlc@v1.2.0 in cut.yml"), "{said}");
    }
}
