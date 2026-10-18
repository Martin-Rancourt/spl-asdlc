//! The method's texts a tick copies into the items it creates, read at the
//! version of spl-asdlc the project is pinned on (BR-ASDLC-032): the ref of
//! its workflow callers at the revision the work is on (a PR's base branch,
//! a candidate's tag), in the method's repository those callers name, with
//! the token the bridge holds for the project. Each pin and each file is
//! read at most once per tick ([`MethodTexts`]), and only when an item is to
//! be created. A text that cannot be read creates no item: the tick reports
//! it, says it once on the item the work belongs to, and tries again at the
//! next tick; it never copies the binary's own texts instead.

use std::cell::RefCell;
use std::collections::BTreeMap;

use serde_json::json;

use super::merge::{for_tracker, tracker_said};
use super::{Report, Tick};
use crate::action_log::ActionLog;
use crate::github::ReadError;
use crate::method::pin::{self, Pin, WORKFLOWS};
use crate::method::texts::{self, protocol};

/// A method's file as read: its text, `None` when that version has none.
type FileRead = Result<Option<String>, String>;

/// What one tick has read of the method's texts: the pin at each revision
/// of the project, and each file at each pin.
#[derive(Default)]
pub struct MethodTexts {
    pins: RefCell<BTreeMap<String, Result<Pin, String>>>,
    files: RefCell<BTreeMap<(String, String), FileRead>>,
}

impl MethodTexts {
    /// The pin of the project at `rev`, read once per tick.
    fn pin(&self, t: &Tick, rev: &str) -> Result<Pin, String> {
        if let Some(p) = self.pins.borrow().get(rev) {
            return p.clone();
        }
        let p = read_pin(t, rev);
        self.pins.borrow_mut().insert(rev.to_string(), p.clone());
        p
    }

    /// The method's file `path` at `pin`, `None` when that version has no
    /// such file, read once per tick.
    fn file(&self, t: &Tick, pin: &Pin, path: &str) -> Result<Option<String>, String> {
        let key = (pin.to_string(), path.to_string());
        if let Some(f) = self.files.borrow().get(&key) {
            return f.clone();
        }
        let f = match t.github.file_from(&t.repo, &pin.repo, path, &pin.rev) {
            Ok(text) => Ok(Some(text)),
            Err(ReadError::NotFound { .. }) => Ok(None),
            Err(e) => Err(e.to_string()),
        };
        self.files.borrow_mut().insert(key, f.clone());
        f
    }
}

/// The pin of the project at `rev`, from its workflow files.
fn read_pin(t: &Tick, rev: &str) -> Result<Pin, String> {
    let names = match t.github.dir(&t.repo, WORKFLOWS, rev) {
        Ok(names) => names,
        // No workflow at all: no caller.
        Err(ReadError::NotFound { .. }) => Vec::new(),
        Err(e) => return Err(format!("the workflow callers at {rev}: {e}")),
    };
    let mut files = Vec::new();
    for name in names
        .iter()
        .filter(|n| n.ends_with(".yml") || n.ends_with(".yaml"))
    {
        let text = t
            .github
            .file(&t.repo, &format!("{WORKFLOWS}/{name}"), rev)
            .map_err(|e| format!("the workflow caller {name} at {rev}: {e}"))?;
        files.push((name.clone(), text));
    }
    pin::resolve(&files).map_err(|e| format!("at {rev}, {e}"))
}

/// The texts an item of `job` (of the angle `angle`) carries: the method's
/// protocol of the job and the angle's text, at the project's pin at `rev`.
pub fn texts(
    t: &Tick,
    rev: &str,
    job: protocol::Job,
    angle: Option<&str>,
) -> Result<protocol::Texts, String> {
    let pin = t.method_texts.pin(t, rev)?;
    let protocol = t.method_texts.file(t, &pin, job.path())?.ok_or_else(|| {
        format!(
            "{} is not at {pin}: {} has no such ref, or the token of {} cannot read that repository \
             (the GitHub App's installation must include it while it is private, docs/setup/github-app.md)",
            job.path(),
            pin.repo,
            t.repo
        )
    })?;
    let text = match angle {
        Some(key) => t.method_texts.file(t, &pin, &protocol::angle_path(key))?,
        None => None,
    };
    // Absent at the pin: a project's own angle, or one the method retired
    // (this build's library says which).
    let retired = angle
        .filter(|_| text.is_none())
        .and_then(protocol::retired_in);
    Ok(protocol::Texts {
        at: pin.to_string(),
        protocol,
        angle: text,
        retired,
    })
}

/// The method's texts could not be read for `what` (the items of `scope`,
/// under the tracker item `key`): a failure of the tick, retried at the
/// next one, and said once on that item, by scope.
pub fn unreadable(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    key: &str,
    scope: &str,
    what: &str,
    error: &str,
) {
    t.unreadable(
        report,
        key,
        format!("the method's texts for {what}: {error}"),
    );
    let marker = texts::marker("method-texts", &[("scope", scope)]);
    match tracker_said(t, key, &marker) {
        Ok(false) => {}
        Ok(true) => return,
        Err(e) => {
            t.unreadable(report, key, format!("comments: {e}"));
            return;
        }
    }
    let body = for_tracker(
        t,
        &format!(
            "The bridge could not read the method's texts this project is pinned on, so it has not created {what} yet: \
             {error}. It never copies other texts instead; it tries again at every tick, and `spl-asdlc doctor` says \
             what is missing.\n\n{marker}"
        ),
    );
    t.act(
        log,
        report,
        key,
        "comment method-texts",
        json!({"scope": scope}),
        || {
            t.tracker
                .comment(key, &body)
                .map(|_| ())
                .map_err(|e| e.to_string())
        },
    );
}
