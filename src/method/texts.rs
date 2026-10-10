//! Every text the bridge writes to a tracker or to GitHub, in English, with
//! the markers that make its once-only effects idempotent across machines
//! (the owner's decision: markers only for effects nobody can observe
//! otherwise). Pure functions over plain inputs.
//!
//! A marker is the token `spl-asdlc:<kind> k=v …`, written hidden as an HTML
//! comment; a tracker that does not keep comments gets the token visible,
//! as inline code ([`visible_markers`]). Readers look for the token, so both
//! forms read the same.

use std::sync::LazyLock;

use regex::Regex;

use crate::method::contract::is_acceptance_heading;
use crate::method::markdown;

pub mod protocol;

// ---- markers ----

/// A hidden marker: `<!-- spl-asdlc:<kind> k=v … -->`. Values carry no
/// space, backtick or `>`.
pub fn marker(kind: &str, fields: &[(&str, &str)]) -> String {
    let fields: String = fields
        .iter()
        .map(|(k, v)| {
            let v: String = v
                .chars()
                .map(|c| {
                    if c.is_whitespace() || c == '`' || c == '>' {
                        '_'
                    } else {
                        c
                    }
                })
                .collect();
            format!(" {k}={v}")
        })
        .collect();
    format!("<!-- spl-asdlc:{kind}{fields} -->")
}

/// The token of a marker, without its comment delimiters.
pub fn token(marker: &str) -> &str {
    marker
        .trim()
        .trim_start_matches("<!--")
        .trim_end_matches("-->")
        .trim()
}

/// Whether a text carries the marker, hidden or visible, whole (`key=ab`
/// is not `key=abc`).
pub fn has_marker(text: &str, marker: &str) -> bool {
    let t = token(marker);
    text.match_indices(t).any(|(i, _)| {
        let rest = &text[i + t.len()..];
        rest.is_empty() || rest.starts_with(char::is_whitespace) || rest.starts_with('`')
    })
}

static HIDDEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<!-- (spl-asdlc:[^`>]*?) -->").unwrap());

/// Markers made visible as inline code, for a tracker that drops HTML
/// comments.
pub fn visible_markers(text: &str) -> String {
    HIDDEN.replace_all(text, "`$1`").to_string()
}

/// Untrusted text (an agent's report, a ticket's title) made inert before
/// it enters a text the bridge writes: one line, and no marker of the
/// bridge's can be forged through it.
pub fn inert(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace("<!--", "&lt;!--")
        .replace("spl-asdlc:", "spl-asdlc :")
        .replace("release_blocker::", "release_blocker :")
}

/// The kind of item the bridge created, read off its description marker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeItem {
    Release { product: String, version: String },
    Angle { scope: String, angle: String },
    Consolidation { scope: String },
}

static ITEM_MARKER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"spl-asdlc:(release|angle|consolidation)((?: [a-z]+=[^\s`>]+)*)").unwrap()
});

/// Read the marker of an item the bridge created (REQ-MERGE-068): never by
/// title. An angle or consolidation item counts only with a parent: a
/// marker pasted into an ordinary ticket does not exempt it from anything.
pub fn bridge_item(description: &str, parent: Option<&str>) -> Option<BridgeItem> {
    match marked_item(description)? {
        BridgeItem::Release { product, version } => Some(BridgeItem::Release { product, version }),
        other if parent.is_some() => Some(other),
        _ => None,
    }
}

fn marked_item(description: &str) -> Option<BridgeItem> {
    let c = ITEM_MARKER.captures(description)?;
    let field = |name: &str| {
        c[2].split_whitespace()
            .find_map(|kv| kv.strip_prefix(&format!("{name}=")))
            .map(str::to_string)
    };
    match &c[1] {
        "release" => Some(BridgeItem::Release {
            product: field("product")?,
            version: field("version")?,
        }),
        "angle" => Some(BridgeItem::Angle {
            scope: field("scope")?,
            angle: field("angle")?,
        }),
        _ => Some(BridgeItem::Consolidation {
            scope: field("scope")?,
        }),
    }
}

// ---- the release item ----

pub fn release_title(product: &str, version: &str) -> String {
    format!(
        "Release {product} {}",
        crate::method::vocabulary::normalise_version(version)
    )
}

/// An angle as the texts show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AngleLine {
    pub key: String,
    pub title: String,
    pub assignee: Option<String>,
}

/// The description of a new release item (REQ-REL-040, spec 05).
pub fn release_description(
    product: &str,
    version: &str,
    label: &str,
    angles: &[AngleLine],
) -> String {
    let v = crate::method::vocabulary::normalise_version(version);
    let table: String = angles
        .iter()
        .map(|a| {
            format!(
                "| {} | {} |\n",
                a.title,
                a.assignee.as_deref().unwrap_or("(unassigned)")
            )
        })
        .collect();
    let (angle_condition, angle_scope) = if angles.is_empty() {
        (
            "",
            "No QA angle is configured (`release.angles = []`): the GO rests on the gate and the conditions that \
             involve no angle.\n"
                .to_string(),
        )
    } else {
        (
            " every angle's PR merged,",
            format!("| Angle | Owner |\n|---|---|\n{table}"),
        )
    };
    format!(
        "{marker}\n\n## Goal\nDeliver {product} {v} to production: the gate green on the latest candidate,{angle_condition} \
         no open blocker, the release-please note covering the range, the fixes in the candidate. The GO belongs \
         to nobody: it is computed. Nobody closes this item by hand: the closure does.\n\n\
         ## Release note\n\n_{placeholder}_\n\n\
         ## Candidates\n\n\
         ## Scope\nThe label `{label}` groups the release's issues.\n\n{angle_scope}",
        marker = marker("release", &[("product", product), ("version", &v)]),
        placeholder = crate::method::release::note::PLACEHOLDER,
    )
}

/// One line of the `## Candidates` log.
pub fn candidate_log_line(candidate: &str, class: &str, previous: Option<&str>) -> String {
    match previous {
        Some(p) => format!("- `{candidate}` — class `{class}` — after `{p}`"),
        None => format!("- `{candidate}` — class `{class}` — first candidate"),
    }
}

/// Append a line to the `## Candidates` section, once.
pub fn append_candidate(description: &str, line: &str) -> String {
    if description.contains(line) {
        return description.to_string();
    }
    let lines: Vec<&str> = description.lines().collect();
    match lines
        .iter()
        .position(|l| l.trim().eq_ignore_ascii_case("## Candidates"))
    {
        Some(h) => {
            let end = lines[h + 1..]
                .iter()
                .position(|l| l.starts_with("## "))
                .map(|i| h + 1 + i)
                .unwrap_or(lines.len());
            let mut body: Vec<&str> = lines[h + 1..end]
                .iter()
                .copied()
                .filter(|l| !l.trim().is_empty())
                .collect();
            body.push(line);
            let mut out: Vec<String> = lines[..=h].iter().map(|s| s.to_string()).collect();
            out.push(String::new());
            out.extend(body.iter().map(|s| s.to_string()));
            if end < lines.len() {
                out.push(String::new());
                out.extend(lines[end..].iter().map(|s| s.to_string()));
            }
            out.join("\n")
        }
        None => format!("{}\n\n## Candidates\n\n{line}\n", description.trim_end()),
    }
}

/// An angle sub-item of a release candidate (REQ-REL-048): self-contained,
/// with real checkboxes, the protocol of a release angle and the method's
/// notes of the angle embedded (BR-ASDLC-015), the project's notes linked.
#[allow(clippy::too_many_arguments)]
pub fn release_angle_item(
    product: &str,
    version: &str,
    candidate: &str,
    rc: u64,
    angle: &AngleLine,
    branch: &str,
    report_path: &str,
    label: &str,
    notes: Option<&str>,
) -> (String, String) {
    let v = crate::method::vocabulary::normalise_version(version);
    let title = format!("{} — {product} {v} (rc.{rc})", angle.title);
    let body = format!(
        "{marker}\n\n## Goal\nRender the {key} review of the whole project at candidate `{candidate}`, as the \
         protocol below describes it (`## Protocol`: the release angle's protocol and the angle's notes).\n\n\
         ## Non-goals\n- Another angle's review: the angles run in parallel.\n\n\
         ## Acceptance criteria\n\
         - [ ] The branch `qa/{candidate}/{key}` is cut from `{branch}`.\n\
         - [ ] Every blocking finding is fixed in that branch, with a test each.\n\
         - [ ] The report `{report_path}` is committed (mandatory, even when nothing was found).\n\
         - [ ] A PR from `qa/{candidate}/{key}` to `{branch}` names this item.\n\n\
         ## Business rules\n{absence}\n\n\
         ## What closes this item\nThe bridge sets this sub-item Done when it merges your angle PR into `{branch}`, \
         which it does once the PR is ready (not a draft), names this item's key in its title or body, carries a valid \
         report at `{report_path}` on its head, is approved at its head by a release coordinator with no request for \
         changes standing, has its required checks green and is up to date with `{branch}`. A report that is missing or \
         invalid is said in a comment here and the PR is not merged. An item still open when the cycle closes is closed \
         with the release.\n\n\
         ## Context\n- Candidate: `{candidate}`\n- Release label: `{label}`\n- Project notes: {notes}\n- The brief is on the release item.\n\
         - This item follows your PR: `In Progress` once it appears; `In Review` once it is ready with its checks green, \
         assigned to the coordinator who reviews it; back `In Progress` when the coordinator requests changes, assigned to \
         this angle's assignee again (or to nobody); `Done` when the bridge merges it after the coordinator's approval.\n\n\
         {protocol}",
        marker = marker("angle", &[("scope", candidate), ("angle", &angle.key)]),
        key = angle.key,
        absence = crate::method::business_rules::ABSENCE_DECLARATION,
        notes = project_notes(notes),
        protocol = protocol::section(protocol::Job::ReleaseAngle, Some(&angle.key)),
    );
    (title, body)
}

/// The project's notes of an angle, linked (never embedded): they live in
/// the project's repository.
fn project_notes(notes: Option<&str>) -> String {
    notes
        .map(|n| format!("`{n}`, in the project's repository"))
        .unwrap_or_else(|| "none".into())
}

/// The scope of a PR review round: the PR and the head it opened at. A
/// round opens when the ticket enters In Review; the coordinator's fixes
/// move the head within the round, a new round needs a new entry.
pub fn round_scope(pr_url: &str, head: &str) -> String {
    format!("{pr_url}@{}", short(head))
}

/// An angle sub-item of a PR review (spec 04 §3): the PR-review protocol
/// and the method's notes of the angle embedded (BR-ASDLC-015), the
/// project's notes linked.
pub fn review_angle_item(
    ticket: &str,
    pr_url: &str,
    head: &str,
    angle: &str,
    notes: Option<&str>,
    reasons: &[String],
) -> (String, String) {
    let title = format!("{} review — {ticket} ({})", capitalised(angle), short(head));
    let why = if reasons.is_empty() {
        String::new()
    } else {
        format!(
            "\n- Why this round carries more angles: {}",
            reasons.join("; ")
        )
    };
    let body = format!(
        "{marker}\n\n## Goal\nReview the diff of {pr_url} at `{head12}` from the {angle} angle, as the protocol below \
         describes it (`## Protocol`: the PR-review protocol and the angle's notes), and post the findings as review \
         comments on the PR (one thread per finding, severity in the first line).\n\n\
         ## Non-goals\n- The other angles of the round.\n- Fixing: the coordinator applies the retained fixes.\n\n\
         ## Acceptance criteria\n- [ ] Every finding is a review thread on the PR, with its severity.\n\
         - [ ] The review is submitted on GitHub, its body naming this item's key.\n\n\
         ## Business rules\n{absence}\n\n\
         ## What closes this item\nThe bridge sets this sub-item Done at the first tick that finds on {pr_url} a \
         submitted review (any state but pending) whose body names this item's key as a whole word: name it in the \
         body of your review. The PR's merge closes it too (Done), its close without merge cancels it.\n\n\
         ## Context\n- Ticket: {ticket}\n- Project notes: {notes}{why}\n\n\
         {protocol}",
        marker = marker(
            "angle",
            &[("scope", &round_scope(pr_url, head)), ("angle", angle)]
        ),
        head12 = short(head),
        absence = crate::method::business_rules::ABSENCE_DECLARATION,
        notes = project_notes(notes),
        protocol = protocol::section(protocol::Job::PrAngle, Some(angle)),
    );
    (title, body)
}

/// The marker of the ticket the bridge opened for the PR at `pr_url`.
pub fn pr_ticket_marker(pr_url: &str) -> String {
    marker("pr-ticket", &[("pr", pr_url)])
}

/// The ticket the bridge opens for a PR of an App that needs none
/// (`review.ticketless_authors`, spec 04): a complete contract whose one box
/// the PR meets, so the PR takes the path of every ticketed PR.
pub fn pr_ticket(number: u64, title: &str, author: &str, pr_url: &str) -> (String, String) {
    let title = format!("PR #{number}: {}", inert(title));
    let body = format!(
        "{marker}\n\n## Goal\nShip {pr_url}, opened by `{author}`, an App whose PRs need no ticket: the bridge \
         opened this one so the PR is reviewed, merged, released and deployed like any other.\n\n\
         ## Non-goals\n- Anything beyond the PR's diff.\n\n\
         ## Acceptance criteria\n- [x] The PR's change is reviewed and merged with its checks green.\n\n\
         ## Business rules\n{absence}",
        marker = pr_ticket_marker(pr_url),
        author = inert(author),
        absence = crate::method::business_rules::ABSENCE_DECLARATION,
    );
    (title, body)
}

/// The marker of the gate-fix ticket of a candidate whose gate is red.
pub fn gate_fix_marker(candidate: &str) -> String {
    marker("gate-fix", &[("candidate", candidate)])
}

/// The candidate a gate-fix ticket was opened for, read off its marker.
pub fn gate_fix_candidate(description: &str) -> Option<String> {
    static GATE_FIX: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"spl-asdlc:gate-fix candidate=([^\s`>]+)").unwrap());
    GATE_FIX.captures(description).map(|c| c[1].to_string())
}

/// The facts of a red gate the gate-fix ticket carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedGateFacts<'a> {
    pub release: &'a str,
    pub candidate: &'a str,
    pub sha: &'a str,
    pub branch: &'a str,
    pub conclusion: &'a str,
    pub run_url: &'a str,
    /// The failed suites of the evidence pack, or why it could not be read.
    pub suites: Result<Vec<String>, String>,
    /// Why the red is taken as final although no automatic rerun was seen
    /// (the bounded wait ran out), when that is the case.
    pub rerun_unseen: Option<&'a str>,
}

/// At most this many failed suites are named one by one.
const SUITES_SHOWN: usize = 30;

/// The gate-fix ticket the bridge opens when a candidate's gate is red and
/// its automatic rerun settled (BR-ASDLC-016): served to an implementer by
/// role, a complete contract whose goal is the gate green on the release
/// branch.
pub fn gate_fix_ticket(f: &RedGateFacts) -> (String, String) {
    // Suite names and read errors come from the evidence pack and from
    // GitHub: one line each, no marker, no backtick that closes a span.
    let code = |s: &str| inert(s).replace('`', "'");
    let title = format!("Make the gate green — {}", code(f.candidate));
    let (criterion, found) = match &f.suites {
        Ok(suites) if suites.is_empty() => (
            format!(
                "- [ ] The step that failed outside the suites (every suite passed: see the gate run {}) is fixed on `{}`.",
                f.run_url, f.branch
            ),
            "none: every suite passed, a step outside the suites failed".to_string(),
        ),
        Ok(suites) => {
            let mut names: Vec<String> = suites
                .iter()
                .take(SUITES_SHOWN)
                .map(|s| format!("`{}`", code(s)))
                .collect();
            if suites.len() > SUITES_SHOWN {
                names.push(format!("and {} more", suites.len() - SUITES_SHOWN));
            }
            let names = names.join(", ");
            (
                format!(
                    "- [ ] Each failing suite passes on `{}`: {names}.",
                    f.branch
                ),
                names,
            )
        }
        Err(why) => (
            format!(
                "- [ ] The suites that failed in the gate run {} (unknown here: the evidence pack could not be read) pass on `{}`.",
                f.run_url, f.branch
            ),
            format!(
                "unknown: the evidence pack could not be read ({})",
                inert(why)
            ),
        ),
    };
    let body = format!(
        "{marker}\n\n## Goal\nMake the release gate green on the release branch `{branch}`. The gate run of the \
         candidate `{candidate}` concluded `{conclusion}` ({run_url}) after its automatic rerun of known flakes, if \
         any, had settled: fix what the failure needs on `{branch}`, through a PR whose base is `{branch}` \
         (`docs/agents/implement.md` step 2), so that the gate of the next candidate is green.\n\n\
         ## Non-goals\n- No new feature.\n- No change beyond what the failure needs.\n\
         - Cutting the next candidate: the coordinator does it through the cut workflow (or the bridge, when \
         `auto_candidate = true`).\n\n\
         ## Acceptance criteria\n{criterion}\n\n\
         ## Business rules\n{absence}\n\n\
         ## Context\n- Release: {release}\n- Candidate: `{candidate}` at `{sha12}`\n- Gate run: {run_url}\n\
         - Failing suites (`{suites_file}` of the evidence pack `{pack}`): {found}\n{unseen}\
         - This ticket follows the flow of any ticket: To Deploy at its fix's merge, Done at the release's \
         closure. The gate of the next candidate is the verification: when it is green and no PR of this ticket \
         was merged yet, the bridge closes it (Done, with a comment naming that candidate); when it is red, the \
         bridge opens a new gate-fix ticket for that candidate.",
        marker = gate_fix_marker(f.candidate),
        branch = f.branch,
        candidate = code(f.candidate),
        conclusion = code(f.conclusion),
        run_url = f.run_url,
        release = f.release,
        sha12 = short(f.sha),
        suites_file = crate::method::release::gate::SUITES_FILE,
        pack = crate::method::release::gate::evidence_artifact(f.sha),
        absence = crate::method::business_rules::ABSENCE_DECLARATION,
        unseen = f
            .rerun_unseen
            .map(|why| format!(
                "- Automatic rerun: {}; check the rerun workflow.\n",
                inert(why)
            ))
            .unwrap_or_default(),
    );
    (title, body)
}

/// The comment of the bridge closing a gate-fix ticket: the gate is green on
/// a newer candidate (or on the same one, rerun by hand).
pub fn gate_fix_closed_comment(candidate: &str, green: &str, run_url: &str) -> String {
    let why = if green == candidate {
        format!("the gate of `{candidate}` this ticket was opened for is green now ({run_url})")
    } else {
        format!(
            "the gate of the newer candidate `{green}` is green ({run_url}): the red gate of `{candidate}` this \
             ticket was opened for is superseded"
        )
    };
    format!(
        "Done: {why}, so the bridge closes this ticket.\n\n{}",
        marker(
            "gate-fix-closed",
            &[("candidate", candidate), ("green", green)]
        )
    )
}

/// The marker of the validation sub-task of a ticket in Post Deploy.
pub fn post_deploy_validation_marker(parent: &str) -> String {
    marker("post-deploy-validation", &[("parent", parent)])
}

/// The validation sub-task of a ticket the closure moved to Post Deploy
/// (BR-ASDLC-003): served to an agent like any ticket, with a complete
/// contract, it asks for the parent's `## Post deploy` boxes to be verified
/// in production and checked, and hands to the owner what an agent cannot
/// verify.
pub fn post_deploy_validation_item(
    parent: &str,
    parent_title: &str,
    parent_url: &str,
    owner: &str,
) -> (String, String) {
    let title = format!("Post-deploy validation — {parent}: {}", inert(parent_title));
    let body = format!(
        "{marker}\n\n## Goal\nValidate in production the `## Post deploy` boxes of {parent} ({parent_url}), now that its \
         release is deployed: check on {parent} each box you can verify yourself, then close this sub-task. If a check \
         needs a human or is beyond you, name it in a comment here and reassign this sub-task to the owner (`{owner}`), \
         so a human finishes. The bridge moves {parent} to `Done` at the tick that finds every one of its `## Post deploy` \
         boxes checked.\n\n\
         ## Non-goals\n- Changing the code, or any box of {parent} outside its `## Post deploy` section.\n\
         - Checking a box whose check was not verified in production.\n\n\
         ## Acceptance criteria\n\
         - [ ] Every `## Post deploy` box of {parent} is either checked, once its check was verified in production, or \
         named in a comment here as a check you could not verify, with this sub-task reassigned to the owner (`{owner}`).\n\n\
         ## Business rules\n{absence}\n\n\
         ## Context\n- Parent: {parent}\n- The owner: `{owner}`",
        marker = post_deploy_validation_marker(parent),
        owner = inert(owner),
        absence = crate::method::business_rules::ABSENCE_DECLARATION,
    );
    (title, body)
}

/// What a consolidation item consolidates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Consolidating {
    /// The review round of a PR.
    PullRequest,
    /// The QA of a release candidate.
    Candidate,
}

/// The consolidation item of a PR review or of a release candidate, with
/// the consolidation protocol embedded (BR-ASDLC-015). A candidate's is the
/// overall review (BR-ASDLC-020): served once every angle PR of the
/// candidate is merged, its item blocked by every angle sub-item; the
/// coordinator reviewed each angle PR as it came, this item reads them
/// together.
pub fn consolidation_item(scope: &str, title_of: &str, of: Consolidating) -> (String, String) {
    let title = format!("Consolidation — {title_of}");
    let (goal, non_goals, criteria, closes, job) = match of {
        Consolidating::PullRequest => (
            format!(
                "Consolidate every angle of {scope} as the protocol below describes it (`## Protocol`, its part \
                 \"For a PR review\"): confirm or reject each finding exactly once, with a reason for every rejection, \
                 apply the retained fixes, and approve."
            ),
            "- New findings beyond what a fix requires.",
            "- [ ] Every finding is confirmed or rejected in its thread.\n\
             - [ ] The retained fixes are commits of the branch.\n- [ ] The approval is submitted at the head.",
            "Nothing you set closes this item: its PR does. The bridge sets it Done when it merges the PR, which it does \
             once a coordinator's approval stands at the PR's head commit (of each coordinator's latest approval or \
             request for changes, none requests changes and one approves the current head), the required checks are \
             green there, every review thread is resolved, the branch is up to date when the base requires it, no hold \
             stands and, when the project requires one, a human approved too. A close of the PR without merge cancels \
             it. A request for changes at the head sends the ticket back to In Progress and leaves this item open.",
            protocol::Job::PrConsolidation,
        ),
        Consolidating::Candidate => (
            format!(
                "The overall review of candidate `{scope}`, as the protocol below describes it (`## Protocol`, its part \
                 \"For a release candidate\", the overall review), once every angle PR of the candidate is merged: read \
                 every merged report together, confirm or reject each non-blocking finding in a comment on its ticket, \
                 with a reason for every rejection, check that nothing falls between the angles, then tag the next \
                 candidate (the cut workflow, action `candidate`) unless the project automates it."
            ),
            "- Reviewing an angle PR: the coordinator reviews each one as it comes, before the bridge merges it.\n\
             - Filing tickets: the bridge creates them from the merged reports.\n- Declaring the GO: the bridge computes it.",
            "- [ ] Every merged report of the candidate is read, with the others.\n\
             - [ ] Every non-blocking finding is confirmed or rejected on its ticket, with a reason for every rejection.\n\
             - [ ] What falls between the angles is checked, and said on the release item.\n\
             - [ ] The next candidate is tagged, or the project's automation tags it.",
            "Nothing you set closes this item. It is served once every angle sub-item of the candidate is resolved, \
             that is once the bridge merged every angle PR, since it is blocked by each of them. The bridge sets it Done \
             when a newer candidate is tagged, or when the release is published; the cycle's closure closes it \
             otherwise. The GO is computed from the facts, never declared.",
            protocol::Job::ReleaseConsolidation,
        ),
    };
    let body = format!(
        "{marker}\n\n## Goal\n{goal}\n\n\
         ## Non-goals\n{non_goals}\n\n\
         ## Acceptance criteria\n{criteria}\n\n\
         ## Business rules\n{absence}\n\n\
         ## What closes this item\n{closes}\n\n\
         {protocol}",
        marker = marker("consolidation", &[("scope", scope)]),
        absence = crate::method::business_rules::ABSENCE_DECLARATION,
        protocol = protocol::section(job, None),
    );
    (title, body)
}

fn capitalised(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

pub fn short(sha: &str) -> &str {
    match sha.char_indices().nth(12) {
        Some((i, _)) => &sha[..i],
        None => sha,
    }
}

// ---- release comments ----

/// Which angles a candidate is offered, and why (spec 05 §3, §7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AngleScope {
    /// The configured angles, while passes remain.
    Configured,
    /// A later pass under `later_passes = "gate-and-coordinator"`: none.
    LighterPass,
    /// No QA angle is configured (`release.angles = []`): none, and the
    /// candidate keeps none should angles be configured before its GO.
    NoQaAngle,
}

/// The marker of a candidate recorded while no QA angle was configured:
/// angles configured later apply from the next candidate, never to it.
pub fn no_qa_angle_marker(candidate: &str) -> String {
    marker("no-qa-angle", &[("candidate", candidate)])
}

/// The comment posted once per candidate (REQ-REL-041).
pub fn candidate_comment(
    candidate: &str,
    class: &str,
    previous: Option<&str>,
    gate_url: Option<&str>,
    paths_since_previous: &[String],
    offered: &[String],
    scope: AngleScope,
) -> String {
    let mut out = format!("### Candidate `{candidate}`\n\n- Class: {class}\n");
    out.push_str(&match previous {
        Some(p) => format!("- Previous candidate: `{p}`\n"),
        None => "- First candidate.\n".to_string(),
    });
    if let Some(u) = gate_url {
        out.push_str(&format!("- Gate run: {u}\n"));
    }
    out.push_str(&format!(
        "- Paths touched since the previous candidate: {}\n",
        paths_since_previous.len()
    ));
    for p in paths_since_previous.iter().take(20) {
        out.push_str(&format!("  - `{p}`\n"));
    }
    if paths_since_previous.len() > 20 {
        out.push_str(&format!(
            "  - … and {} more\n",
            paths_since_previous.len() - 20
        ));
    }
    if scope == AngleScope::NoQaAngle {
        out.push_str("\nNo QA angle is configured (`release.angles = []`): this candidate opens no angle sub-item and no consolidation item, and its GO rests on the gate and the conditions that involve no angle. Angles configured later apply from the next candidate.\n");
    } else if scope == AngleScope::LighterPass {
        out.push_str("\nNo angle is offered on this candidate: later passes are judged by the gate and the coordinator (`later_passes = \"gate-and-coordinator\"`).\n");
    } else if offered.is_empty() {
        out.push_str("\nNo review reopened: no angle is offered on this candidate.\n");
    } else {
        out.push_str(&format!(
            "\nAngles offered on this candidate once its gate is green: {}.\n",
            offered.join(", ")
        ));
    }
    out.push('\n');
    out.push_str(&marker("candidate", &[("tag", candidate)]));
    if scope == AngleScope::NoQaAngle {
        out.push('\n');
        out.push_str(&no_qa_angle_marker(candidate));
    }
    out
}

/// One embarked ticket in the brief.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BriefTicket {
    pub id: String,
    pub title: String,
    pub state: String,
    pub prs: Vec<u64>,
    /// The description, or `None` when the ticket could not be read.
    pub description: Option<String>,
}

/// The facts of the brief (REQ-REL-044/045).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Brief {
    pub candidate: String,
    pub sha: String,
    pub class: String,
    pub last_release: Option<String>,
    pub previous_candidate: Option<String>,
    pub paths_since_release: Vec<String>,
    pub paths_since_candidate: Option<Vec<String>>,
    pub gate_url: Option<String>,
    pub evidence_url: Option<String>,
    pub tickets: Vec<BriefTicket>,
    pub untracked: Vec<String>,
    pub rule_files_touched: Vec<String>,
    /// The candidate has no QA angle (`release.angles = []`).
    pub no_qa_angle: bool,
}

pub fn brief(b: &Brief) -> String {
    let mut out = format!(
        "### Release brief — `{}`\n\n- Commit: `{}`\n- Class: {}\n",
        b.candidate, b.sha, b.class
    );
    out.push_str(&match &b.last_release {
        Some(t) => format!(
            "- Diff since the last published release: `{t}` — {} file(s)\n",
            b.paths_since_release.len()
        ),
        None => "- Diff since the last published release: (first release)\n".to_string(),
    });
    out.push_str(&match (&b.previous_candidate, &b.paths_since_candidate) {
        (None, _) => "- Diff since the previous candidate: first candidate of this version, identical to the diff above\n".to_string(),
        (Some(p), Some(paths)) => format!("- Diff since the previous candidate: `{p}` — {} file(s)\n", paths.len()),
        (Some(p), None) => format!("- Diff since the previous candidate: `{p}` not found — diff cannot be computed\n"),
    });
    if let Some(u) = &b.gate_url {
        out.push_str(&format!("- Gate run: {u}\n"));
    }
    if let Some(u) = &b.evidence_url {
        out.push_str(&format!("- Evidence pack: {u}\n"));
    }
    if b.no_qa_angle {
        out.push_str(
            "\n#### QA angles\n\nNo QA angle is configured (`release.angles = []`): no angle sub-item and no consolidation item are opened for this candidate. The GO rests on the gate green on the latest candidate, no open blocker, the release-please note covering the range and the staged version being the one judged.\n",
        );
    }
    out.push_str("\n#### Tickets included\n\n");
    if b.tickets.is_empty() {
        out.push_str("No ticket linked to the candidate's PRs.\n");
    }
    for t in &b.tickets {
        let prs = t
            .prs
            .iter()
            .map(|n| format!("#{n}"))
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!(
            "- **{}** — {} [{}] — PR {}\n",
            t.id,
            t.title,
            t.state,
            if prs.is_empty() { "none".into() } else { prs }
        ));
        match &t.description {
            None => out.push_str("  - (ticket unreadable: its criteria are not shown)\n"),
            Some(d) => {
                let lines = markdown::without_fences(d);
                let ac = markdown::sections_of(&lines, is_acceptance_heading);
                let boxes = markdown::checkboxes(d);
                let post: Vec<String> = crate::method::contract::post_deploy_boxes(d)
                    .into_iter()
                    .map(|b| b.line)
                    .collect();
                for bx in &boxes {
                    let tag = if post.contains(&bx.line) {
                        " (post deploy)"
                    } else {
                        ""
                    };
                    out.push_str(&format!("  {}{tag}\n", bx.line.trim()));
                }
                if boxes.is_empty() && ac.is_empty() {
                    out.push_str("  - (no acceptance criterion declared)\n");
                }
                let rules = crate::method::business_rules::cited_ids(d);
                if !rules.is_empty() {
                    out.push_str(&format!(
                        "  - Business rules cited: {}\n",
                        rules.into_iter().collect::<Vec<_>>().join(", ")
                    ));
                }
            }
        }
    }
    if !b.untracked.is_empty() {
        out.push_str("\n#### No linked ticket\n\n");
        for u in &b.untracked {
            out.push_str(&format!("- {u}\n"));
        }
    }
    if !b.rule_files_touched.is_empty() {
        out.push_str(&format!(
            "\n#### Business rules touched\n\n{}\n\nEach rule touched must have a named test that is green.\n",
            b.rule_files_touched.iter().map(|p| format!("- `{p}`")).collect::<Vec<_>>().join("\n")
        ));
    }
    let details = |title: &str, paths: &[String]| {
        let mut s = format!(
            "\n<details><summary>{title} ({})</summary>\n\n",
            paths.len()
        );
        for p in paths.iter().take(40) {
            s.push_str(&format!("- `{p}`\n"));
        }
        if paths.len() > 40 {
            s.push_str(&format!("- … and {} more\n", paths.len() - 40));
        }
        s.push_str("\n</details>\n");
        s
    };
    out.push_str(&details(
        "Paths since the last release",
        &b.paths_since_release,
    ));
    if let Some(p) = &b.paths_since_candidate {
        out.push_str(&details("Paths since the previous candidate", p));
    }
    out.push('\n');
    out.push_str(&marker("brief", &[("candidate", &b.candidate)]));
    out
}

/// A stable 64-bit FNV-1a digest, for marker keys that must read the same
/// on every machine and every build.
pub fn digest(text: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// The dedupe key of a HOLD (REQ-REL-069): sorted blockers and fix gaps.
pub fn hold_key(blockers: &[String], fix_gaps: &[String]) -> String {
    let mut b = blockers.to_vec();
    b.sort();
    let mut f = fix_gaps.to_vec();
    f.sort();
    digest(&format!("{}|{}", b.join(","), f.join(",")))
}

pub fn hold_comment(
    candidate: &str,
    reasons: &[String],
    blockers_carried: &str,
    key: &str,
) -> String {
    format!(
        "**HOLD computed on `{candidate}`** — the release PRs stay open.\n\n{}\n\nBlockers-Carried: {blockers_carried}\n\n\
         The GO is recomputed at every tick and the PRs are merged by code as soon as it is true; nothing to tick by hand.\n\n{}",
        reasons
            .iter()
            .map(|r| format!("- {r}"))
            .collect::<Vec<_>>()
            .join("\n"),
        marker("hold", &[("candidate", candidate), ("key", key)])
    )
}

/// The escalation when the passes are spent (REQ-REL-064, spec 05 §10).
pub fn escalation_comment(
    candidate: &str,
    rc: u64,
    max: u32,
    blockers: &[String],
    branch_moved: bool,
) -> String {
    let cause = if branch_moved {
        "fixes landed after the last candidate".to_string()
    } else {
        format!("{} blocker(s) remain", blockers.len())
    };
    format!(
        "### Release-candidate passes exhausted — `{candidate}`\n\nCandidate {rc} of {max} allowed, and {cause}.\n\n{}\n\n\
         To decide: requalify a finding (`spl-asdlc release requalify`), allow one more candidate, or abandon the cycle \
         (`spl-asdlc release abandon`). Remove the hold label once decided.\n\n{}",
        blockers
            .iter()
            .map(|b| format!("- {b}"))
            .collect::<Vec<_>>()
            .join("\n"),
        marker("qa-escalation", &[("candidate", candidate)])
    )
}

/// The stagnation notice (REQ-REL-079): the red gate first and apart.
pub fn stagnation_message(
    candidate: &str,
    hours: u32,
    gate_red: Option<&str>,
    lists: &[(&str, Vec<String>)],
    fallback_reasons: &[String],
    release: &str,
    url: &str,
) -> String {
    let mut out =
        format!("Release stuck: `{candidate}` has been waiting for more than {hours} h.\n");
    if let Some(detail) = gate_red {
        out.push_str(&format!(
            "\nCandidate gate red ({detail}) — it is not the QA that blocks.\n"
        ));
    }
    let mut any = false;
    for (title, items) in lists {
        if items.is_empty() {
            continue;
        }
        any = true;
        out.push_str(&format!("\n{title}:\n"));
        for i in items {
            out.push_str(&format!("- {i}\n"));
        }
    }
    if !any {
        for r in fallback_reasons
            .iter()
            .filter(|r| !r.contains("gate is not green"))
        {
            out.push_str(&format!("- {r}\n"));
        }
    }
    out.push_str(&format!("\nRelease item: {release} — {url}"));
    out
}

/// The gate record (REQ-REL-078); the `Blockers-Carried:` line is the GO's.
#[allow(clippy::too_many_arguments)]
pub fn gate_record(
    version: &str,
    candidate: &str,
    sha: &str,
    branch: &str,
    release: &str,
    release_url: &str,
    computed_at: &str,
    go: &crate::method::release::go::GoDecision,
    angles: &[(String, String)],
    prs: &[(String, String, u64)],
) -> Option<String> {
    // A record only ever attests a computed GO.
    if !go.go {
        return None;
    }
    let blockers_carried = go.blockers_carried();
    let rows: String = angles
        .iter()
        .map(|(a, s)| format!("| {a} | {s} |\n"))
        .collect();
    let qa = if angles.is_empty() {
        "The gate is green on the candidate, no QA angle is configured, no blocker is open.\n"
            .to_string()
    } else {
        format!(
            "The gate is green on the candidate, every angle PR is merged, no blocker is open.\n\n| angle | state |\n|---|---|\n{rows}"
        )
    };
    let prs: String = prs
        .iter()
        .map(|(c, v, n)| format!("- `{c}` {v}: #{n}\n"))
        .collect();
    Some(format!(
        "# Release v{version}\n\n{marker}\n\n- Candidate: {candidate} ({sha12}) on {branch}\n- Release item: [{release}]({release_url})\n\
         - GO computed at {computed_at} UTC by the release pass\n\nBlockers-Carried: {blockers_carried}\n\n## Computed GO\n\n\
         {qa}\n## Release PRs of the cycle\n\n{prs}",
        marker = record_marker(candidate),
        sha12 = short(sha),
    ))
}

pub fn record_marker(candidate: &str) -> String {
    marker("release-record", &[("candidate", candidate)])
}

/// A record attests `candidate` iff its marker is a line of its own and it
/// carries exactly one `Blockers-Carried:` line whose value is `none`.
pub fn record_names(text: &str, candidate: &str) -> bool {
    let m = record_marker(candidate);
    let has_marker = text.lines().any(|l| l.trim() == m);
    let carried: Vec<&str> = text
        .lines()
        .filter_map(|l| l.strip_prefix("Blockers-Carried:"))
        .collect();
    has_marker && carried.len() == 1 && carried[0].trim().eq_ignore_ascii_case("none")
}

/// The comment on a ticket when one of its PRs is merged (REQ-MERGE-062 as
/// spec 04 §10 corrects it): the state the rules gave the ticket, and what
/// comes next. Its `merged_pr:` line is the record of the merge
/// (REQ-MERGE-075).
pub fn merged_comment(
    pr_url: &str,
    base: &str,
    merged_by: Option<&str>,
    state: &str,
    next: &str,
) -> String {
    let how = match merged_by {
        None => format!(
            "{pr_url} was green, approved and up to date with `{base}`, so the bridge merged it."
        ),
        Some(who) => format!(
            "{pr_url} was merged by `{who}` outside the bridge; the bridge applies the same state rule to it."
        ),
    };
    format!(
        "{how}\n\nmerged_by: {}\nmerged_pr: {pr_url}\nstate: {state}\nnext: {next}",
        merged_by.unwrap_or("bridge")
    )
}

/// The comment on a ticket when one of its PRs is closed without merge and
/// the state rules move the ticket (spec 04 §10).
pub fn closed_unmerged_comment(pr_url: &str, state: &str, next: &str) -> String {
    format!(
        "{pr_url} was closed without merge: its review round is canceled and it delivers nothing.\n\nstate: {state}\nnext: {next}\n\n{}",
        marker("closed-unmerged", &[("pr", pr_url)])
    )
}

/// The comment that opens a review round on the ticket (spec 04 §3).
pub fn review_round_comment(
    pr_url: &str,
    head: &str,
    angles: &[(String, String)],
    reasons: &[String],
    key: &str,
) -> String {
    let items = angles
        .iter()
        .map(|(a, id)| format!("- {a}: {id}"))
        .collect::<Vec<_>>()
        .join("\n");
    let why = if reasons.is_empty() {
        String::new()
    } else {
        format!("\n\nWhy: {}", reasons.join("; "))
    };
    format!(
        "Review round for {pr_url} at `{}`: the PR is ready, its checks are green and its contract is complete. The angles review the diff and post their findings on the PR; the coordinator consolidates and approves, then the bridge merges.\n\n{items}{why}\n\n{}",
        short(head),
        marker("review-round", &[("pr", pr_url), ("head", key)])
    )
}

#[cfg(test)]
mod tests;
