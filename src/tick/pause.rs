//! The pause switch (`docs/reference/07-the-bridge.md`, "Pausing a
//! project"): the repository variable `SPL_ASDLC_PAUSED`, which a person
//! sets in GitHub without access to the host that runs the bridge. Set to
//! `true`, the tick writes nothing for the repository but a single notice on
//! the release item of a cycle in flight; unset or anything else, it acts.
//!
//! A switch that cannot be read (the App lacks "Variables: read", GitHub
//! failed) is not a pause: the bridge acts as it did before the switch
//! existed, and says so in its report. Holding every project of an
//! installation that lacks the permission would turn an upgrade into a
//! stop nobody asked for; `doctor` names the missing permission.

use serde_json::json;

use crate::action_log::ActionLog;
use crate::github::{GitHub, Read, RepoSlug, Variable};
use crate::method::release::shape::Shape;
use crate::method::texts;

use super::merge::{for_tracker, tracker_said};
use super::{Report, Tick, release};

/// The repository variable that pauses the bridge for a repository.
pub const VARIABLE: &str = "SPL_ASDLC_PAUSED";

/// Where the switch stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Switch {
    /// Unset, or `false`.
    Off,
    /// Set to `true` (any case): the bridge writes nothing.
    On(Variable),
    /// Set to anything else: not a pause, said by `doctor`.
    Other(String),
    /// It could not be read: not a pause, said in the report.
    Unreadable(String),
}

/// Read the switch of `repo`.
pub fn switch(github: &dyn GitHub, repo: &RepoSlug) -> Switch {
    of(github.variable(repo, VARIABLE))
}

fn of(read: Read<Variable>) -> Switch {
    match read {
        Ok(v) if v.value.trim().eq_ignore_ascii_case("true") => Switch::On(v),
        Ok(v) if v.value.trim().is_empty() || v.value.trim().eq_ignore_ascii_case("false") => {
            Switch::Off
        }
        Ok(v) => Switch::Other(v.value),
        Err(e) if e.is_not_found() => Switch::Off,
        Err(e) => Switch::Unreadable(e.to_string()),
    }
}

/// A paused tick: the release item of a cycle in flight is told once per
/// setting of the switch (its marker carries when it was set); nothing else
/// is written. A read that fails leaves the notice to the next paused tick.
pub fn notice(t: &Tick, log: &mut ActionLog, report: &mut Report, variable: &Variable) {
    let shape = Shape::of(&t.config.product.name, &t.config.release);
    let Ok(Some((candidate, _))) = release::newest(t, &shape) else {
        return;
    };
    let Ok(Some(item)) = release::find_release_item(t, &shape, candidate.version) else {
        return;
    };
    if item.state.is_resolved() {
        return;
    }
    let since = variable.updated_at.as_deref().unwrap_or("unknown");
    let marker = texts::marker("paused", &[("since", since)]);
    match tracker_said(t, &item.key, &marker) {
        Ok(false) => {}
        _ => return,
    }
    let body = for_tracker(
        t,
        &format!(
            "The bridge is paused for {}: the repository variable `{VARIABLE}` is `true`. It reads as usual and writes nothing, here or on GitHub, until the variable is unset or `false`; the cycle of {} resumes then where it stands.\n\n{marker}",
            t.repo, candidate.tag
        ),
    );
    t.act(
        log,
        report,
        &item.key,
        "comment paused",
        json!({"variable": VARIABLE, "since": since}),
        || {
            t.tracker
                .comment(&item.key, &body)
                .map(|_| ())
                .map_err(|e| e.to_string())
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::ReadError;

    fn var(value: &str) -> Read<Variable> {
        Ok(Variable {
            name: VARIABLE.into(),
            value: value.into(),
            updated_at: None,
        })
    }

    #[test]
    fn only_true_pauses_and_an_unreadable_switch_is_no_pause() {
        assert!(matches!(of(var("true")), Switch::On(_)));
        assert!(matches!(of(var(" TRUE ")), Switch::On(_)));
        assert_eq!(of(var("false")), Switch::Off);
        assert_eq!(of(var("")), Switch::Off);
        assert_eq!(of(var("yes")), Switch::Other("yes".into()));
        assert_eq!(
            of(Err(ReadError::NotFound {
                what: "variable".into()
            })),
            Switch::Off
        );
        assert!(matches!(
            of(Err(ReadError::unreadable("variable", "HTTP 403"))),
            Switch::Unreadable(_)
        ));
    }
}
