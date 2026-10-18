//! The identity of the items the bridge creates: their marker
//! (REQ-MERGE-068), kept twice. Once in the item's description, where every
//! lookup reads it, and once in a comment the bridge posts right after the
//! creation, which agents do not edit. An agent that rewrites the description
//! and drops the marker line (to tick a box, say) does not make the item
//! invisible, so it is never created a second time (SID-1170).
//!
//! The comments are read only when the description lookup found nothing,
//! and only for the items whose description carries no marker of the
//! bridge's at all: the cost of a lookup does not grow with the items. An
//! item found by its comment gets the marker back in its description, once,
//! with a comment saying so; the next lookup finds it by its description.

use serde_json::{Value, json};

use crate::action_log::ActionLog;
use crate::method::texts;
use crate::tracker::{Item, NewItem};

use super::merge::for_tracker;
use super::{Report, Tick};

/// Create the item the bridge's `new` describes, then post its identity
/// comment (the marker of its description). A comment that fails is
/// reported; the item stays found by its description.
pub fn create(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    object: &str,
    action: &str,
    inputs: Value,
    new: &NewItem,
) -> Option<Item> {
    let item = t.act(log, report, object, action, inputs, || {
        t.tracker.create_item(new).map_err(|e| e.to_string())
    })?;
    if let Some(marker) = texts::identity_marker(&new.description) {
        let body = for_tracker(t, &texts::identity_comment(&marker));
        t.act(
            log,
            report,
            &item.key,
            "comment identity",
            json!({"marker": texts::token(&marker)}),
            || {
                t.tracker
                    .comment(&item.key, &body)
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            },
        );
    }
    Some(item)
}

/// The items among `items` that are the bridge's item of `marker`: those
/// `described` (by their description, as the caller reads it), or, when none
/// is, those whose description carries no marker of the bridge's and whose
/// identity comment carries `marker`, each with its marker written back.
/// Comments that cannot be read are an error: the caller then creates
/// nothing.
pub fn find(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    items: &[Item],
    marker: &str,
    described: impl Fn(&Item) -> bool,
) -> Result<Vec<Item>, String> {
    let found: Vec<Item> = items.iter().filter(|i| described(i)).cloned().collect();
    if !found.is_empty() {
        return Ok(found);
    }
    let mut out = Vec::new();
    for item in items
        .iter()
        .filter(|i| texts::identity_marker(&i.description).is_none())
    {
        if said(t, &item.key, marker)? {
            out.push(restore(t, log, report, item, marker));
        }
    }
    Ok(out)
}

/// Whether `item` is the bridge's item of `marker`, `described` by its
/// description or, when its description carries no marker of the bridge's,
/// by its identity comment (its marker then written back).
pub fn is(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    item: &Item,
    marker: &str,
    described: bool,
) -> Result<bool, String> {
    if described {
        return Ok(true);
    }
    if texts::identity_marker(&item.description).is_some() || !said(t, &item.key, marker)? {
        return Ok(false);
    }
    restore(t, log, report, item, marker);
    Ok(true)
}

/// Whether one of the bridge's comments on the item carries the marker.
fn said(t: &Tick, key: &str, marker: &str) -> Result<bool, String> {
    let comments = t
        .tracker
        .comments(key)
        .map_err(|e| format!("the comments of {key}: {e}"))?;
    Ok(comments
        .iter()
        .any(|c| c.by_bridge && texts::has_marker(&c.body, marker)))
}

/// The marker written back at the top of the description, then a comment
/// saying so; the item as it now reads (unchanged when the write failed: the
/// next tick finds it by its comment again and retries). An archived item is
/// never written to again: it stays found by its comment.
fn restore(t: &Tick, log: &mut ActionLog, report: &mut Report, item: &Item, marker: &str) -> Item {
    if item.archived {
        return item.clone();
    }
    let description = format!(
        "{}\n\n{}",
        for_tracker(t, marker),
        item.description.trim_start()
    );
    let written = t.act(
        log,
        report,
        &item.key,
        "restore marker",
        json!({"marker": texts::token(marker)}),
        || {
            t.tracker
                .update_description(&item.key, &description)
                .map_err(|e| e.to_string())
        },
    );
    if written.is_none() {
        return item.clone();
    }
    let body = for_tracker(t, &texts::marker_restored_comment());
    t.act(
        log,
        report,
        &item.key,
        "comment marker restored",
        json!({}),
        || {
            t.tracker
                .comment(&item.key, &body)
                .map(|_| ())
                .map_err(|e| e.to_string())
        },
    );
    Item {
        description,
        ..item.clone()
    }
}
