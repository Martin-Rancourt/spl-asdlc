//! The Done gate (REQ-MERGE-064..067, REQ-REL-087..090): a ticket is done
//! only when every box of its contract — its own and its duplicates' — is
//! checked and at least one box was ever declared. The same gate serves
//! every door to Done, so no two doors drift into two ideas of "done enough".

use std::collections::BTreeSet;

use super::contract::is_post_deploy_heading;
use super::markdown;
use super::vocabulary::Status;

/// At most this many tickets in a duplicate walk; beyond, the contract is
/// unreadable (an unfinished walk never promotes).
pub const MAX_DUPLICATE_WALK: usize = 50;
/// One ticket of a contract: the ticket itself or one of its duplicates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractMember {
    pub id: String,
    /// Its title, named beside its id when its boxes are listed.
    pub title: String,
    pub description: String,
}

/// What a tracker answers about one ticket for the walk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateNode {
    pub title: String,
    pub description: String,
    /// Tickets linked by a `duplicate` relation, in either direction.
    pub duplicates: Vec<String>,
    /// The relation list was cut by pagination.
    pub relations_cut: bool,
}

/// The ticket and every ticket reached through `duplicate` relations, each
/// once, primary first; or why the walk could not be finished.
pub fn duplicate_closure(
    id: &str,
    node: DuplicateNode,
    read: &mut dyn FnMut(&str) -> Result<DuplicateNode, String>,
) -> (Vec<ContractMember>, Option<String>) {
    let mut members = Vec::new();
    let mut seen = BTreeSet::from([id.to_string()]);
    let mut queue = std::collections::VecDeque::from([(id.to_string(), node)]);
    while let Some((current, node)) = queue.pop_front() {
        if node.relations_cut {
            return (
                members,
                Some(format!(
                    "{current}'s duplicate relations were cut short by pagination"
                )),
            );
        }
        members.push(ContractMember {
            id: current.clone(),
            title: node.title,
            description: node.description,
        });
        for next in node.duplicates {
            if !seen.insert(next.clone()) {
                continue;
            }
            if seen.len() > MAX_DUPLICATE_WALK {
                return (
                    members,
                    Some(format!(
                        "more than {MAX_DUPLICATE_WALK} tickets in the duplicate graph"
                    )),
                );
            }
            match read(&next) {
                Ok(n) => queue.push_back((next, n)),
                Err(e) => return (members, Some(format!("{next} could not be read: {e}"))),
            }
        }
    }
    (members, None)
}

/// An open box and the ticket it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenBox {
    pub source: String,
    pub line: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DoneGate {
    /// Every open box of the contract, the primary ticket's first.
    pub open_items: Vec<OpenBox>,
    /// No member declares a single checkbox.
    pub no_criteria_declared: bool,
    /// The contract could not be read in full.
    pub unreadable_reason: Option<String>,
    /// The open boxes that sit in a `## Post deploy` section (a subset).
    pub post_release_items: Vec<OpenBox>,
    /// Every member read, `(id, title)`, the primary ticket first: the
    /// origins its open boxes are listed under.
    pub origins: Vec<(String, String)>,
}

impl DoneGate {
    pub fn ok(&self) -> bool {
        self.open_items.is_empty() && !self.no_criteria_declared && self.unreadable_reason.is_none()
    }

    /// Open boxes minus the post-deploy ones (one occurrence removed per
    /// post-deploy box, so a line written inside and outside the section is
    /// told apart).
    pub fn release_blocking_items(&self) -> Vec<OpenBox> {
        let mut post = self.post_release_items.clone();
        let mut out = Vec::new();
        for item in &self.open_items {
            if let Some(i) = post.iter().position(|p| p == item) {
                post.remove(i);
            } else {
                out.push(item.clone());
            }
        }
        out
    }

    /// Only post-deploy boxes are open, the contract is readable and declared.
    pub fn awaits_post_release(&self) -> bool {
        !self.post_release_items.is_empty()
            && self.release_blocking_items().is_empty()
            && self.unreadable_reason.is_none()
            && !self.no_criteria_declared
    }

    /// As [`awaits_post_release`](Self::awaits_post_release), and every open
    /// post-deploy box is the ticket's own: a duplicate's box cannot be
    /// ticked through this ticket.
    pub fn awaits_post_release_of(&self, id: &str) -> bool {
        self.awaits_post_release() && self.post_release_items.iter().all(|b| b.source == id)
    }
}

/// Judge a contract (REQ-MERGE-064). `unreadable` is the reason a walk or a
/// read failed; it fails the gate whatever the boxes say.
pub fn evaluate(members: &[ContractMember], unreadable: Option<String>) -> DoneGate {
    let mut gate = DoneGate {
        unreadable_reason: unreadable,
        ..DoneGate::default()
    };
    let mut any_box = false;
    for m in members {
        gate.origins.push((m.id.clone(), m.title.clone()));
        let boxes = markdown::checkboxes(&m.description);
        any_box |= !boxes.is_empty();
        for b in boxes.into_iter().filter(|b| !b.checked) {
            gate.open_items.push(OpenBox {
                source: m.id.clone(),
                line: b.line,
            });
        }
        for b in markdown::checkboxes_in_sections(&m.description, is_post_deploy_heading)
            .into_iter()
            .filter(|b| !b.checked)
        {
            gate.post_release_items.push(OpenBox {
                source: m.id.clone(),
                line: b.line,
            });
        }
    }
    gate.no_criteria_declared = !any_box && gate.unreadable_reason.is_none();
    gate
}

/// A ticket at closure that only waits for its own post-deploy boxes, and is
/// shipped (`To Deploy`) or already `Post Deploy` (a replay leaves it there).
pub fn waits_in_post_release(id: &str, status: Option<Status>, gate: &DoneGate) -> bool {
    gate.awaits_post_release_of(id) && matches!(status, Some(Status::ToDeploy | Status::PostDeploy))
}

/// What the closure does to one embarked ticket (REQ-REL-087).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClosureVerdict {
    PostDeploy,
    Reopen,
    Done,
}

pub fn closure_verdict(id: &str, status: Option<Status>, gate: &DoneGate) -> ClosureVerdict {
    if waits_in_post_release(id, status, gate) {
        ClosureVerdict::PostDeploy
    } else if !gate.ok() {
        ClosureVerdict::Reopen
    } else {
        ClosureVerdict::Done
    }
}

/// Why a ticket cannot pass `Done` (REQ-MERGE-066). `state_unchanged` adds
/// the sentence saying so, only when the caller really leaves the state.
pub fn refusal(id: &str, gate: &DoneGate, state_unchanged: bool) -> String {
    if let Some(reason) = &gate.unreadable_reason {
        let mut s = format!(
            "Cannot move to `Done` for now: the contract of {id} (itself and its duplicates) could not be read in full ({reason})."
        );
        if state_unchanged {
            s.push_str(" State unchanged — a read that fails never promotes.");
        }
        return s;
    }
    if gate.no_criteria_declared {
        let mut s = "Cannot move to `Done`: no criterion declared — neither the ticket nor its duplicates ever declared a checkbox, so there is no readable contract to verify.".to_string();
        if state_unchanged {
            s.push_str(" State unchanged.");
        }
        return s;
    }
    let n = gate.open_items.len();
    let mut from_dups: Vec<(String, usize)> = Vec::new();
    for b in gate.open_items.iter().filter(|b| b.source != id) {
        match from_dups.iter_mut().find(|(s, _)| *s == b.source) {
            Some((_, c)) => *c += 1,
            None => from_dups.push((b.source.clone(), 1)),
        }
    }
    from_dups.sort();
    let mut s = format!("Cannot move to `Done`: {n} criteria/boxes unchecked");
    if !from_dups.is_empty() {
        let parts = from_dups
            .iter()
            .map(|(src, c)| format!("{c} taken from {src}"))
            .collect::<Vec<_>>()
            .join(", ");
        s.push_str(&format!(", of which {parts}"));
    }
    s.push('.');
    if state_unchanged {
        s.push_str(" State unchanged.");
    }
    s.push('\n');
    s.push_str(&open_boxes_by_origin(id, gate));
    s
}

/// Every open box of the contract, in full and verbatim, grouped by origin:
/// the ticket itself, then each duplicate by its id and title, in the order
/// the walk read them (rule Q9TM). A box of a `## Post deploy` section says
/// so.
fn open_boxes_by_origin(id: &str, gate: &DoneGate) -> String {
    let mut order: Vec<&str> = vec![id];
    for source in gate
        .origins
        .iter()
        .map(|(o, _)| o.as_str())
        .chain(gate.open_items.iter().map(|b| b.source.as_str()))
    {
        if !order.contains(&source) {
            order.push(source);
        }
    }
    let mut post = gate.post_release_items.clone();
    let mut s = String::new();
    for origin in order {
        let boxes: Vec<&OpenBox> = gate
            .open_items
            .iter()
            .filter(|b| b.source == origin)
            .collect();
        if boxes.is_empty() {
            continue;
        }
        if origin == id {
            s.push_str(&format!("\nUnchecked on {id}, this ticket:\n"));
        } else {
            let title = gate
                .origins
                .iter()
                .find(|(o, _)| o == origin)
                .map(|(_, t)| t.trim())
                .filter(|t| !t.is_empty());
            match title {
                Some(t) => s.push_str(&format!(
                    "\nUnchecked on {origin} — {t}, a duplicate merged into this ticket:\n"
                )),
                None => s.push_str(&format!(
                    "\nUnchecked on {origin}, a duplicate merged into this ticket:\n"
                )),
            }
        }
        for b in boxes {
            let in_post = match post.iter().position(|p| p == b) {
                Some(i) => {
                    post.remove(i);
                    " (Post deploy)"
                }
                None => "",
            };
            s.push_str(&format!("{}{in_post}\n", b.line.trim()));
        }
    }
    s.trim_end().to_string()
}

/// How the closure ends its comment on a ticket it sends back: unassigned,
/// never handed to someone named (BR-ASDLC-009).
pub const RETURNED_UNASSIGNED: &str = "Returned to `In Progress` by the release closure, unassigned: \
     the agent system resumes it by role, with nobody named.";

/// How to reconcile the boxes a ticket carries from its merged duplicates
/// (rules W4KD, Q9TM).
pub const RECONCILE_DUPLICATES: &str = "How to reconcile the duplicates: merging a duplicate into a ticket keeps \
     its boxes, and the closure counts them until they are checked on the duplicate itself (rule W4KD). For each box \
     above taken from a duplicate:\n\
     - when this ticket's work already proves it, check it on the duplicate and comment there with the evidence \
     (the comment, PR or test that proves it);\n\
     - when it needs a gesture in production, hand it to a human through a decision request that names the gesture \
     and the box it checks;\n\
     - otherwise, do the work it asks, as for a box of this ticket.\n\
     Never check a box without its evidence, and never delete one: nothing is lost in a merge.";

/// What follows a closure's reopening, once the boxes are checked.
pub const WHAT_COMES_NEXT: &str = "What happens next: once only this ticket's own `## Post deploy` boxes are \
     left unchecked (a duplicate's are checked on the duplicate), the ticket goes to the `Post Deploy` column as \
     usual, where they are checked once verified in production; once every box is checked, it goes to `Done`.";

/// The closure's comment on a ticket it sends back (REQ-REL-089): what is
/// open, in full and by origin; how to reconcile the duplicates' boxes when
/// some are open; what happens next; and that nobody is named (rule Q9TM).
pub fn reopen_comment(id: &str, gate: &DoneGate) -> String {
    let mut s = refusal(id, gate, false);
    let readable = gate.unreadable_reason.is_none() && !gate.no_criteria_declared;
    if readable && gate.open_items.iter().any(|b| b.source != id) {
        s.push_str("\n\n");
        s.push_str(RECONCILE_DUPLICATES);
    }
    if readable && !gate.open_items.is_empty() {
        s.push_str("\n\n");
        s.push_str(WHAT_COMES_NEXT);
    }
    format!("{s}\n\n{RETURNED_UNASSIGNED}")
}

#[cfg(test)]
mod tests;
