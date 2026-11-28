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

use crate::github::COMPARE_FILES_CAP;
use crate::method::contract::is_acceptance_heading;
use crate::method::markdown;
use crate::method::release::gate;

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

/// Whether a text carries the marker as a line of its own, hidden or
/// visible: the form the bridge writes a gesture's marker in, which a
/// marker quoted inside a copied line (a ticket's title, a criterion) never
/// takes.
pub fn has_marker_line(text: &str, marker: &str) -> bool {
    let hidden = marker.trim();
    let visible = format!("`{}`", token(marker));
    text.lines()
        .map(str::trim)
        .any(|l| l == hidden || l == visible)
}

/// Whether a bridge comment records the `release abandon` verb: its
/// marker on a line of its own.
pub fn records_abandon(body: &str) -> bool {
    has_marker_line(body, &marker("cycle-abandoned", &[]))
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

/// Untrusted text made inert for an inline code span: [`inert`], and no
/// backtick that would close the span.
fn inert_code(text: &str) -> String {
    inert(text).replace('`', "'")
}

/// The kind of item the bridge created, read off its description marker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeItem {
    Release { product: String, version: String },
    Angle { scope: String, angle: String },
    Consolidation { scope: String },
}

/// The description of a candidate's QA milestone (BR-ASDLC-038): one item
/// per QA angle and the overall review, in the milestone named after the
/// candidate.
pub fn milestone_description(candidate: &str) -> String {
    format!(
        "The QA of the release candidate `{candidate}`: one item per QA angle and the overall review. \
         A newer candidate redoes every review in its own milestone; what is left open here is then canceled.\n\n{}\n",
        marker("milestone", &[("scope", candidate)])
    )
}

static MILESTONE_MARKER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"spl-asdlc:milestone scope=([^\s`>]+)").unwrap());

/// The candidate whose QA milestone this description is, read off its
/// marker.
pub fn milestone_scope(description: &str) -> Option<String> {
    MILESTONE_MARKER
        .captures(description)
        .map(|c| c[1].to_string())
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

// ---- the identity of the bridge's items ----

/// The kinds of marker that name an item the bridge created: its identity,
/// never a fact about it.
static IDENTITY_MARKER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"spl-asdlc:(release-note|release|angle|consolidation|gate-fix|merge-back-fix|finding|pr-ticket|post-deploy-validation)((?: [a-z]+=[^\s`>]+)*)(?:[\s`]|$)",
    )
    .unwrap()
});

/// The identity marker an item's description carries, hidden form (what
/// [`marker`] writes), hidden or visible in the text.
pub fn identity_marker(description: &str) -> Option<String> {
    IDENTITY_MARKER
        .captures(description)
        .map(|c| format!("<!-- spl-asdlc:{}{} -->", &c[1], &c[2]))
}

/// The `## Release note` of a ticket the bridge creates for its own
/// bookkeeping (the release's unified note, a merge back): a tracker that
/// asks every ticket for an `EN:` and an `FR:` line finds them, so the
/// ticket is refined as it is created (ASDLC-DJCU).
pub const INTERNAL_RELEASE_NOTE: &str =
    "EN: Internal tooling improvements.\nFR: Amélioration de l'outillage interne.";

/// What every item the bridge creates asks of whoever edits it.
pub const KEEP_MARKER: &str = "When you edit this description, ticking a box included, change only what you mean to: \
     keep the bridge's `spl-asdlc` marker line, by which it finds this item.";

/// The comment the bridge posts on an item it has just created: its marker,
/// kept where agents do not edit, should the description lose it.
pub fn identity_comment(marker: &str) -> String {
    format!(
        "The bridge created this item and finds it by the marker below, kept here as well as in the description. \
         {KEEP_MARKER}\n\n{marker}"
    )
}

/// The comment of the bridge writing back the marker an edit removed from
/// the description.
pub fn marker_restored_comment() -> String {
    format!(
        "The bridge wrote its marker back at the top of the description: an edit had removed it, and the bridge found \
         this item by its identity comment. {KEEP_MARKER}"
    )
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
         no open blocker, the fixes in the candidate, nothing but its own QA reports after it. The GO belongs \
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

/// The candidate a line of the `## Candidates` log names: its first inline
/// code, after a list bullet of any kind (`-`, `*`, `+`). A tracker
/// rewrites the markdown it stores (Linear turns `- ` into `* `, spaces the
/// items with blank lines), so a line is found again by its tag, never by
/// its exact text.
fn listed_candidate(line: &str) -> Option<&str> {
    let rest = line.trim().strip_prefix(['-', '*', '+'])?;
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let rest = rest.trim_start().strip_prefix('`')?;
    rest.split_once('`').map(|(tag, _)| tag)
}

/// Append `line`, the log line of `candidate`, to the `## Candidates`
/// section, unless a line of the section already names `candidate`
/// (whatever its bullet, blank lines or trailing spaces): the description
/// comes back unchanged then.
pub fn append_candidate(description: &str, candidate: &str, line: &str) -> String {
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
            if lines[h + 1..end]
                .iter()
                .any(|l| listed_candidate(l) == Some(candidate))
            {
                return description.to_string();
            }
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
    texts: &protocol::Texts,
) -> (String, String) {
    let title = release_angle_title(product, version, rc, angle);
    let body = format!(
        "{marker}\n\n## Goal\nRender the {key} review of the whole project at candidate `{candidate}`, as the \
         protocol below describes it (`## Protocol`: the release angle's protocol and the angle's notes).\n\n\
         ## Non-goals\n- Another angle's review: the angles run in parallel.\n\n\
         ## Acceptance criteria\n\
         - [ ] The branch `qa/{candidate}/{key}` is cut from `{branch}`.\n\
         - [ ] Every finding is documented in the report, blocking or not, and nothing is fixed in that branch.\n\
         - [ ] The report `{report_path}` is committed (mandatory, even when nothing was found).\n\
         - [ ] A PR from `qa/{candidate}/{key}` to `{branch}` names this item.\n\n\
         ## Business rules\n{absence}\n\n\
         ## What closes this item\nThe bridge sets this sub-item To Deploy when it merges your angle PR into `{branch}`, \
         which it does with no approval once the PR is ready (not a draft), names this item's key in its title or body, \
         carries a valid report at `{report_path}` on its head and no other change, has no request for changes standing, \
         has its required checks green and is up to date with `{branch}`. A report that is missing or invalid is said in a \
         comment here and the PR is not merged. It goes Done when the release is published or a newer candidate is \
         tagged; an item still open when the cycle closes is closed with the release. The coordinator judges your \
         findings afterwards, in the candidate's verdict.\n\n\
         ## Context\n- Candidate: `{candidate}`\n- Release label: `{label}`\n- Project notes: {notes}\n- The brief is on the release item.\n\
         - This item follows your PR: `In Progress` once it appears; move it `In Review` yourself once your PR is \
         opened (not a draft) or pushed again, before you give it back (the bridge does too once its checks are green), \
         the bridge's own step, kept by its assignee; back `In Progress` when a coordinator requests changes at its head, \
         assigned to this angle's assignee again (or to nobody), or when a required check is red, its report is missing \
         or invalid, or it carries more than its report; `To Deploy` when the bridge merges it.\n\n\
         {protocol}",
        marker = marker("angle", &[("scope", candidate), ("angle", &angle.key)]),
        key = angle.key,
        absence = crate::method::business_rules::ABSENCE_DECLARATION,
        notes = project_notes(notes),
        protocol = protocol::section(protocol::Job::ReleaseAngle, Some(&angle.key), texts),
    );
    (title, body)
}

/// The title of a release angle sub-item, by which it is found again
/// (REQ-REL-048).
pub fn release_angle_title(product: &str, version: &str, rc: u64, angle: &AngleLine) -> String {
    let v = crate::method::vocabulary::normalise_version(version);
    format!("{} — {product} {v} (rc.{rc})", angle.title)
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
    texts: &protocol::Texts,
) -> (String, String) {
    let title = review_angle_title(ticket, head, angle);
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
         ## What closes this item\nYou do: once your review is submitted on {pr_url} (any state but pending), its \
         body naming this item's key as a whole word, mark this item Done before you give it back; the bridge never \
         closes it for a review, and the coordinator's approval does not count while it is open. The coordinator \
         reopens it when no submitted review names its key. The PR's merge closes it too (Done), its close without \
         merge cancels it.\n\n\
         ## Context\n- Ticket: {ticket}\n- Project notes: {notes}{why}\n\n\
         {protocol}",
        marker = marker(
            "angle",
            &[("scope", &round_scope(pr_url, head)), ("angle", angle)]
        ),
        head12 = short(head),
        absence = crate::method::business_rules::ABSENCE_DECLARATION,
        notes = project_notes(notes),
        protocol = protocol::section(protocol::Job::PrAngle, Some(angle), texts),
    );
    (title, body)
}

/// The title of a PR-review angle sub-item, by which it is found again.
pub fn review_angle_title(ticket: &str, head: &str, angle: &str) -> String {
    format!("{} review — {ticket} ({})", capitalised(angle), short(head))
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
         ## Business rules\n{absence}\n\n\
         ## Context\n- {keep}",
        marker = pr_ticket_marker(pr_url),
        keep = KEEP_MARKER,
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
    /// The failed jobs of the gate run, read when the evidence pack names no
    /// failed suite; empty when they were not read or none failed. They name
    /// the title only, never the contract.
    pub failed_jobs: Vec<String>,
    /// Why the red is taken as final although no automatic rerun was seen
    /// (the bounded wait ran out), when that is the case.
    pub rerun_unseen: Option<&'a str>,
}

/// At most this many failed suites are named one by one.
const SUITES_SHOWN: usize = 30;

/// At most this many failures are named in a gate-fix ticket's title, the
/// rest counted (`+N`).
const TITLE_FAILURES_SHOWN: usize = 2;

/// A failure's name in a title is cut past this many characters.
const TITLE_NAME_CHARS: usize = 40;

/// The title of a gate-fix ticket: what failed in the candidate's gate (the
/// failed suites of the evidence pack, otherwise the failed jobs of the
/// run), or today's wording when nothing failed is known (ASDLC-HZYJ). The
/// bridge finds the ticket by its marker, never by its title.
pub fn gate_fix_title(f: &RedGateFacts) -> String {
    let candidate = inert(f.candidate);
    let failed: &[String] = match &f.suites {
        Ok(suites) if !suites.is_empty() => suites,
        _ => &f.failed_jobs,
    };
    let names: Vec<String> = failed
        .iter()
        .map(|n| inert(n))
        .filter(|n| !n.is_empty())
        .collect();
    if names.is_empty() {
        return format!("Make the gate green — {candidate}");
    }
    let shown: Vec<String> = names
        .iter()
        .take(TITLE_FAILURES_SHOWN)
        .map(|n| {
            if n.chars().count() > TITLE_NAME_CHARS {
                let cut: String = n.chars().take(TITLE_NAME_CHARS - 1).collect();
                format!("{cut}…")
            } else {
                n.clone()
            }
        })
        .collect();
    let more = match names.len().saturating_sub(TITLE_FAILURES_SHOWN) {
        0 => String::new(),
        n => format!(" +{n}"),
    };
    let verb = if names.len() == 1 { "fails" } else { "fail" };
    format!("Gate red on {candidate}: {}{more} {verb}", shown.join(", "))
}

/// The gate-fix ticket the bridge opens when a candidate's gate is red and
/// its automatic rerun settled (BR-ASDLC-016): served to an implementer by
/// role, a complete contract whose goal is the gate green on the release
/// branch.
pub fn gate_fix_ticket(f: &RedGateFacts) -> (String, String) {
    // Suite names and read errors come from the evidence pack and from
    // GitHub: one line each, no marker, no backtick that closes a span.
    let code = |s: &str| inert(s).replace('`', "'");
    let title = gate_fix_title(f);
    let (criterion, found) = match &f.suites {
        Ok(suites) if suites.is_empty() => (
            format!(
                "- [ ] The step that failed outside the suites (every suite passed: see the gate run {}) is fixed in a PR whose base is `{}`.",
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
                    "- [ ] The cause of each failing suite is fixed in a PR whose base is `{}`: {names}.",
                    f.branch
                ),
                names,
            )
        }
        Err(why) => (
            format!(
                "- [ ] The cause of the suites that failed in the gate run {} (unknown here: the evidence pack could not be read) is fixed in a PR whose base is `{}`.",
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
         (`.spl-asdlc/process.md`, \"Implement a ticket\" step 2), so that the gate of the next candidate is green.\n\n\
         ## Non-goals\n- No new feature.\n- No change beyond what the failure needs.\n\
         - Cutting the next candidate: the coordinator does it through the cut workflow (or the bridge, when \
         `auto_candidate = true`).\n\n\
         ## Acceptance criteria\n{criterion}\n\n\
         ## Business rules\n{absence}\n\n\
         ## Context\n- Release: {release}\n- Candidate: `{candidate}` at `{sha12}`\n- Gate run: {run_url}\n\
         - Failing suites (`{suites_file}` of the evidence pack `{pack}`): {found}\n{unseen}\
         - Tick the box once the fix's PR is ready, before its merge: the bridge merges a PR only once its ticket \
         ticks a box, and the suites passing is the next gate's verification, not this box's (SID-1203).\n\
         - This ticket follows the flow of any ticket: To Deploy at its fix's merge, Done at the release's \
         closure. The gate of the next candidate is the verification: when it is green and no PR of this ticket \
         was merged yet, the bridge closes it (Done, with a comment naming that candidate); when it is red, the \
         bridge opens a new gate-fix ticket for that candidate.\n- {keep}",
        marker = gate_fix_marker(f.candidate),
        keep = KEEP_MARKER,
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

// ---- a blocked merge back (ASDLC-PQ5J) ----

/// Why the merge back of a published release is blocked, read off its
/// merge-back PR once the publication's close concluded red.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeBackBlock {
    /// GitHub reports a real conflict (`DIRTY`).
    Dirty,
    /// A required check of the default branch is red on the PR's head.
    RedCheck(Vec<String>),
    /// The required checks are green and GitHub still refused the App's
    /// merge (`BLOCKED`, `BEHIND`): the App is not on the ruleset's bypass list.
    AppRefused(String),
}

impl MergeBackBlock {
    /// The cause in one line, for the release item and the fix ticket.
    pub fn cause(&self) -> String {
        match self {
            MergeBackBlock::Dirty => {
                "a real conflict: GitHub reports the merge-back PR `DIRTY`".to_string()
            }
            MergeBackBlock::RedCheck(names) => format!(
                "a required check is red on the merge-back PR's head: {}",
                names
                    .iter()
                    .map(|n| format!("`{}`", inert(n).replace('`', "'")))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            MergeBackBlock::AppRefused(state) => format!(
                "the required checks are green and the merge-back PR stays `{}`: GitHub refused the App's merge \
                 (the App is not on the bypass list of the default branch's ruleset)",
                inert(state).replace('`', "'")
            ),
        }
    }

    /// The short key of the cause, for the action log.
    pub fn key(&self) -> &'static str {
        match self {
            MergeBackBlock::Dirty => "dirty",
            MergeBackBlock::RedCheck(_) => "red-check",
            MergeBackBlock::AppRefused(_) => "app-refused",
        }
    }
}

/// The marker of the fix ticket of a release whose merge back is blocked:
/// one per release tag, ever.
pub fn merge_back_fix_marker(tag: &str) -> String {
    marker("merge-back-fix", &[("tag", tag)])
}

/// The release tag a merge-back fix ticket was opened for, read off its marker.
pub fn merge_back_fix_tag(description: &str) -> Option<String> {
    static FIX: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"spl-asdlc:merge-back-fix tag=([^\s`>]+)").unwrap());
    FIX.captures(description).map(|c| c[1].to_string())
}

/// The facts of a blocked merge back the fix ticket carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockedMergeBack<'a> {
    /// The release item's key.
    pub release: &'a str,
    pub tag: &'a str,
    pub branch: &'a str,
    pub default_branch: &'a str,
    /// The merge-back PR the publication opened.
    pub pr_url: &'a str,
    pub run_url: &'a str,
    pub block: &'a MergeBackBlock,
}

/// The comment on the release item that says why its merge back is blocked.
pub fn merge_back_blocked_comment(f: &BlockedMergeBack) -> String {
    format!(
        "### Merge back of `{tag}` blocked\n\nThe publication's close could not merge `{branch}` back into \
         `{default}` ({run}): {cause}. Merge-back PR: {pr}. The release is out; it is `{default}` that is \
         behind. The bridge opens one fix ticket under this item; once its `chore/merge-back-*` PR is merged \
         as a merge commit, the bridge dispatches the publication on `{branch}` with `replay: true`, and the \
         closure goes on.",
        tag = inert(f.tag),
        branch = inert(f.branch),
        default = inert(f.default_branch),
        run = f.run_url,
        cause = f.block.cause(),
        pr = f.pr_url,
    )
}

/// The fix ticket of a blocked merge back: the recipe of « Release close »
/// (`docs/setup/repository.md`) as a contract, served like a gate-fix.
pub fn merge_back_fix_ticket(f: &BlockedMergeBack) -> (String, String) {
    let tag = inert(f.tag);
    let branch = inert(f.branch);
    let default = inert(f.default_branch);
    let title = format!("Make the merge back green — {tag}");
    let body = format!(
        "{marker}\n\n## Goal\nThe release `{tag}` is published, and its release branch `{branch}` could not be \
         merged back into `{default}`: {cause}. Land the release branch in `{default}` as a merge commit, through a \
         `chore/merge-back-*` PR the bridge merges, so that the release tag becomes an ancestor of `{default}` and \
         the release closes.\n\n\
         ## Non-goals\n- No new feature, no change beyond what the merge back needs.\n\
         - Republishing: the release is out.\n\
         - Merging the PR or dispatching the replay: the bridge does both.\n\n\
         ## Acceptance criteria\n\
         - [ ] A branch `chore/merge-back-<stamp>` is cut from the tip of `{branch}` (`<stamp>` the UTC date and time, \
         such as `2026-11-25-18-30-utc`), `{default}` is merged into it, and the fix is committed on top: the conflict resolved, or what \
         makes the required checks green.\n\
         - [ ] Its PR into `{default}` is open and ready, titled as a Conventional Commit naming this ticket, such as \
         `chore(release): merge back {tag} (<this ticket's id>)`.\n\n\
         ## Business rules\n{absence}\n\n\
         ## Release note\n{note}\n\n\
         ## Context\n- Release: {release}\n- Release branch: `{branch}`\n- Merge-back PR: {pr}\n\
         - Publication run: {run}\n\
         - Never bring `{default}` into `release/**`: no PR from `{default}` into `{branch}`, no branch update from \
         the base, no local merge pushed to `{branch}`. It adds code no candidate covered.\n\
         - The bridge merges the `chore/merge-back-*` PR as a merge commit, never a squash, once its required checks \
         and its title check are green, and only when its branch contains the tip of `{branch}`. Then it dispatches \
         the publication on `{branch}` with `replay: true`, which deletes the branch and publishes nothing, and the \
         release closes.\n\
         - A person who resolves the merge back by hand (the recipe of « Release close » in \
         `docs/setup/repository.md`) does the same: the bridge takes it as done and closes this ticket.\n\
         - Tick each box once it is met, before the merge.\n- {keep}",
        marker = merge_back_fix_marker(f.tag),
        cause = f.block.cause(),
        release = f.release,
        pr = f.pr_url,
        run = f.run_url,
        absence = crate::method::business_rules::ABSENCE_DECLARATION,
        note = INTERNAL_RELEASE_NOTE,
        keep = KEEP_MARKER,
    );
    (title, body)
}

/// The comment of the closure closing a merge-back fix ticket: the release
/// is in the default branch, by the bridge or by a person.
pub fn merge_back_fix_closed_comment(tag: &str) -> String {
    format!(
        "Done: the release `{}` is merged back and its release branch is gone, so the closure closes this ticket.\n\n{}",
        inert(tag),
        marker("merge-back-fix-closed", &[("tag", tag)])
    )
}

/// The marker of the replay the bridge dispatched for a release.
pub fn merge_back_replay_marker(tag: &str) -> String {
    marker("merge-back-replay", &[("tag", tag)])
}

/// The marker of the validation sub-task earlier versions created under a
/// ticket in Post Deploy. None is created any more: the validation happens on
/// the ticket itself (BR-ASDLC-003).
pub fn post_deploy_validation_marker(parent: &str) -> String {
    marker("post-deploy-validation", &[("parent", parent)])
}

/// Whether a text carries a validation sub-task's marker, hidden or visible,
/// whatever parent key it names (the key of a tracker the items came from
/// included).
pub fn is_post_deploy_validation(text: &str) -> bool {
    text.contains(token(&post_deploy_validation_marker("")))
}

/// The marker of the comment that cancels a validation sub-task.
pub fn post_deploy_validation_retired_marker() -> String {
    marker("post-deploy-validation-retired", &[])
}

/// The comment on a validation sub-task the bridge cancels (BR-ASDLC-017):
/// the validation goes on on its parent, under `parent`'s `## Post deploy`
/// section.
pub fn post_deploy_validation_retired_comment(parent: Option<&str>) -> String {
    let on = parent.map_or_else(|| "its parent ticket".to_string(), str::to_string);
    format!(
        "Canceled by the bridge: the post-deploy validation now happens on the ticket itself, not on a sub-task. \
         Verify in production and check the `## Post deploy` boxes of {on}: an agent checks those it can verify, and \
         hands that ticket to the owner for the rest. The bridge moves {on} to `Done` at the tick that finds every one \
         of its `## Post deploy` boxes checked.\n\n{}",
        post_deploy_validation_retired_marker()
    )
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
pub fn consolidation_item(
    scope: &str,
    title_of: &str,
    of: Consolidating,
    texts: &protocol::Texts,
) -> (String, String) {
    let title = consolidation_title(title_of);
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
            "You do: once your approving review stands at the PR's head, or your request for changes, mark this item \
             Done before you give it back. Before approving, check that every angle sub-item of the round has a \
             submitted review on the PR naming its key, and reopen the one that has none instead of approving. The \
             bridge merges the PR once a coordinator's approval stands at its head commit (of each coordinator's latest \
             approval or request for changes, none requests changes and one approves the current head), every angle \
             sub-item of the round is closed, the required checks are green there, every review thread is resolved, \
             the branch is up to date when the base requires it and no hold stands. While the PR still waits on an \
             approval at its head with this item closed, the bridge serves it again (Ready), once per head. The PR's \
             merge closes it too (Done), its close without merge cancels it. A request for changes at the head sends \
             the ticket back to In Progress.",
            protocol::Job::PrConsolidation,
        ),
        Consolidating::Candidate => (
            format!(
                "The overall review of candidate `{scope}`, as the protocol below describes it (`## Protocol`, its part \
                 \"For a release candidate\", the overall review), once every angle report of the candidate is merged: \
                 read every merged report together, judge each finding once (accepted, blocking or not, or rejected with \
                 its reason), check what falls between the angles, then commit your verdict, `go` or `hold`, in the file \
                 `consolidation.json` beside the reports, from the branch `qa/{scope}/consolidation`, in a PR to the \
                 release branch that names this item."
            ),
            "- Reviewing an angle PR: the bridge merges each valid report on its own.\n\
             - Filing tickets: the bridge creates them from your merged verdict, for the accepted findings only.\n\
             - Declaring the GO: the bridge computes it, your verdict `go` among its conditions.",
            "- [ ] Every merged report of the candidate is read, with the others.\n\
             - [ ] Every finding is judged once in the verdict: accepted (blocking or not) or rejected with its reason.\n\
             - [ ] What falls between the angles is checked, and said in the verdict.\n\
             - [ ] The verdict is committed in a PR that names this item.",
            "Your verdict does: move this item In Review yourself once your verdict PR is opened (not a draft), before \
             you give it back. The bridge sets it Done when it merges your verdict PR, which it does once every \
             angle PR of the candidate is merged, the PR names this item's key, carries a valid verdict and no other \
             change, is authored by a release coordinator or approved by one at its head, has its required checks green \
             and is up to date. What keeps it from the merge is said on the PR; the bridge moves this item back In \
             Progress when that is yours to mend (the PR names no item, carries another change or an invalid verdict, \
             or a required check is red) or when no verdict PR is open. This item is served once every angle \
             report of the candidate is merged: blocked by each angle's implementation, or held in Backlog by the bridge \
             where the tracker keeps no such block. A newer candidate cancels it.",
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
        protocol = protocol::section(job, None, texts),
    );
    (title, body)
}

/// The title of a consolidation item, by which it is found again.
pub fn consolidation_title(title_of: &str) -> String {
    format!("Consolidation — {title_of}")
}

impl Consolidating {
    /// The job of this consolidation's protocol.
    pub fn job(self) -> protocol::Job {
        match self {
            Consolidating::PullRequest => protocol::Job::PrConsolidation,
            Consolidating::Candidate => protocol::Job::ReleaseConsolidation,
        }
    }
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
    /// The configured angles, on every candidate under the full policy.
    Configured,
    /// A later pass under `later_passes = "gate-and-coordinator"`: none.
    LighterPass,
    /// No QA angle is configured (`release.angles = []`): none, and the
    /// candidate keeps none should angles be configured before its GO.
    NoQaAngle,
}

/// The marker of the comment that says why a later pass under the lighter
/// policy asks a verdict (ASDLC-DF2Q).
pub fn lighter_verdict_marker(candidate: &str) -> String {
    marker("lighter-verdict", &[("candidate", candidate)])
}

/// Why a later pass under `later_passes = "gate-and-coordinator"`, which
/// offers no angle, asks the coordinator's verdict: it frees blocking
/// findings of the release, and only a person judges that their merged fix
/// fixes them (ASDLC-DF2Q).
pub fn lighter_verdict_comment(candidate: &str, findings: &[String]) -> String {
    let which = if findings.is_empty() {
        String::new()
    } else {
        format!(" ({})", findings.join(", "))
    };
    format!(
        "`{candidate}` is a later pass under `later_passes = \"gate-and-coordinator\"`: it offers no angle, but it frees blocking findings of the release{which}, their fix merged and in it. Ticked boxes and a merged PR do not prove a finding fixed: read each fix against its finding (its proposed test included), then give the verdict on `{candidate}` as for any candidate. The GO waits for it.\n\n{}",
        lighter_verdict_marker(candidate)
    )
}

/// The marker of a candidate recorded while no QA angle was configured:
/// angles configured later apply from the next candidate, never to it.
pub fn no_qa_angle_marker(candidate: &str) -> String {
    marker("no-qa-angle", &[("candidate", candidate)])
}

/// How many files a list holds, said as GitHub gave it: a list GitHub cut
/// at [`COMPARE_FILES_CAP`] says so, never a count it is not (rule G7CK).
pub fn file_count(paths: &[String], cut: bool) -> String {
    if cut {
        format!(
            "{} file(s) or more: GitHub cut the list at {COMPARE_FILES_CAP}",
            paths.len()
        )
    } else {
        format!("{} file(s)", paths.len())
    }
}

/// The class a candidate is said with (rule X4PV): a file list GitHub cut
/// counts as touching every reviewed path, so the candidate is `sensible`
/// whatever its tag says, and says why.
pub fn class_line(class: &str, cut: bool) -> String {
    match gate::class_with_cut_list(class, cut) {
        c if c == class => format!("- Class: {class}\n"),
        c => format!(
            "- Class: {c} (GitHub cut the file list at {COMPARE_FILES_CAP}: it counts as touching every reviewed path; the tag says {class})\n"
        ),
    }
}

/// The comment posted once per candidate (REQ-REL-041).
#[allow(clippy::too_many_arguments)]
pub fn candidate_comment(
    candidate: &str,
    class: &str,
    previous: Option<&str>,
    gate_url: Option<&str>,
    paths_since_previous: &[String],
    paths_cut: bool,
    offered: &[String],
    scope: AngleScope,
) -> String {
    let mut out = format!(
        "### Candidate `{candidate}`\n\n{}",
        class_line(class, paths_cut)
    );
    out.push_str(&match previous {
        Some(p) => format!("- Previous candidate: `{p}`\n"),
        None => "- First candidate.\n".to_string(),
    });
    if let Some(u) = gate_url {
        out.push_str(&format!("- Gate run: {u}\n"));
    }
    out.push_str(&format!(
        "- Paths touched since the previous candidate: {}\n",
        if paths_cut {
            file_count(paths_since_previous, true)
        } else {
            paths_since_previous.len().to_string()
        }
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
    /// GitHub cut `paths_since_release` at 300 (rule G7CK).
    pub paths_since_release_cut: bool,
    pub paths_since_candidate: Option<Vec<String>>,
    /// GitHub cut `paths_since_candidate` at 300 (rule G7CK).
    pub paths_since_candidate_cut: bool,
    pub gate_url: Option<String>,
    pub evidence_url: Option<String>,
    pub tickets: Vec<BriefTicket>,
    pub untracked: Vec<String>,
    pub rule_files_touched: Vec<String>,
    /// The candidate has no QA angle (`release.angles = []`).
    pub no_qa_angle: bool,
}

pub fn brief(b: &Brief) -> String {
    let cut = b.paths_since_release_cut || b.paths_since_candidate_cut;
    let mut out = format!(
        "### Release brief — `{}`\n\n- Commit: `{}`\n{}",
        b.candidate,
        b.sha,
        class_line(&b.class, cut)
    );
    let since_release = file_count(&b.paths_since_release, b.paths_since_release_cut);
    out.push_str(&match &b.last_release {
        Some(t) => format!("- Diff since the last published release: `{t}` — {since_release}\n"),
        None => format!(
            "- Diff since the repository's root: first release, no earlier release to measure from — {since_release}\n"
        ),
    });
    out.push_str(&match (&b.previous_candidate, &b.paths_since_candidate) {
        (None, _) => "- Diff since the previous candidate: first candidate of this version, identical to the diff above\n".to_string(),
        (Some(p), Some(paths)) => format!(
            "- Diff since the previous candidate: `{p}` — {}\n",
            file_count(paths, b.paths_since_candidate_cut)
        ),
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
            "\n#### QA angles\n\nNo QA angle is configured (`release.angles = []`): no angle sub-item and no consolidation item are opened for this candidate. The GO rests on the gate green on the latest candidate, no open blocker and nothing but bookkeeping after the candidate.\n",
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
            inert(&t.id),
            inert(&t.title),
            inert(&t.state),
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
                    out.push_str(&format!("  {}{tag}\n", inert(&bx.line)));
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
            b.rule_files_touched.iter().map(|p| format!("- `{}`", inert_code(p))).collect::<Vec<_>>().join("\n")
        ));
    }
    if b.paths_since_release_cut {
        out.push_str(&format!(
            "\n#### File list cut by GitHub\n\nGitHub lists at most {COMPARE_FILES_CAP} files of a comparison, and cut this one: the paths below and the business rules touched are the first {COMPARE_FILES_CAP} only. The range's commits, and so its tickets, are complete. The cut list counts as touching every reviewed path: the candidate is reviewed as `sensible`, with every configured angle. Read the whole diff with `git diff --name-only` on the range.\n"
        ));
    }
    let details = |title: &str, paths: &[String]| {
        let mut s = format!(
            "\n<details><summary>{title} ({})</summary>\n\n",
            paths.len()
        );
        for p in paths.iter().take(40) {
            s.push_str(&format!("- `{}`\n", inert_code(p)));
        }
        if paths.len() > 40 {
            s.push_str(&format!("- … and {} more\n", paths.len() - 40));
        }
        s.push_str("\n</details>\n");
        s
    };
    out.push_str(&details(
        if b.last_release.is_some() {
            "Paths since the last release"
        } else {
            "Paths since the repository's root"
        },
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
        "**HOLD computed on `{candidate}`** — the release is not tagged.\n\n{}\n\nBlockers-Carried: {blockers_carried}\n\n\
         The GO is recomputed at every tick and the release is tagged and published by code as soon as it is true; nothing to tick by hand.\n\n{}",
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

/// The comment on a ticket when one of its PRs is merged (REQ-MERGE-062 as
/// spec 04 §10 corrects it): the state the rules gave the ticket, and what
/// comes next. Its `merged_pr:` line is the record of the merge
/// (REQ-MERGE-075). It states only the conditions that applied to the merge.
pub fn merged_comment(
    pr_url: &str,
    base: &str,
    merged_by: &MergedBy,
    state: &str,
    next: &str,
) -> String {
    let how = match merged_by {
        MergedBy::Bridge {
            approved_by,
            up_to_date,
        } => {
            let mut held = vec!["green".to_string()];
            if let Some(by) = approved_by {
                held.push(format!("approved by `{by}`"));
            }
            if *up_to_date {
                held.push(format!("up to date with `{base}`"));
            }
            let last = held.pop().unwrap_or_default();
            let held = if held.is_empty() {
                last
            } else {
                format!("{} and {last}", held.join(", "))
            };
            match approved_by {
                Some(_) => format!("{pr_url} was {held}, so the bridge merged it."),
                None => format!(
                    "{pr_url} was {held}; no review angle applies to it, so the bridge merged it with no PR review."
                ),
            }
        }
        MergedBy::Outside(who) => format!(
            "{pr_url} was merged by `{who}` outside the bridge; the bridge applies the same state rule to it."
        ),
    };
    let by = match merged_by {
        MergedBy::Bridge { .. } => "bridge",
        MergedBy::Outside(who) => who,
    };
    format!("{how}\n\nmerged_by: {by}\nmerged_pr: {pr_url}\nstate: {state}\nnext: {next}")
}

/// Who merged a PR, as the merge comment says it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergedBy<'a> {
    /// The bridge: `approved_by` is the coordinator whose approval at the
    /// head let it merge (`None` for a PR with no angle, merged with no PR
    /// review, spec 04 §11); `up_to_date` when the project requires a PR up
    /// to date with its base (`review.require_up_to_date`).
    Bridge {
        approved_by: Option<&'a str>,
        up_to_date: bool,
    },
    /// Someone outside the bridge.
    Outside(&'a str),
}

/// The comment on a ticket when one of its PRs is closed without merge and
/// the state rules move the ticket (spec 04 §10).
pub fn closed_unmerged_comment(pr_url: &str, state: &str, next: &str) -> String {
    format!(
        "{pr_url} was closed without merge: its review round is canceled and it delivers nothing.\n\nstate: {state}\nnext: {next}\n\n{}",
        marker("closed-unmerged", &[("pr", pr_url)])
    )
}

/// The marker of [`settled_comment`]: one per latest merged PR and state, so
/// a ticket settled again after a later merge or closure is told again.
pub fn settled_marker(pr_url: &str, state: &str) -> String {
    marker("settled", &[("pr", pr_url), ("state", state)])
}

/// The comment on a ticket left In Progress past a merge when it is settled
/// at a later tick, its boxes checked since (spec 04 §10). `shipped` is the
/// release tag that already holds its last merged PR, if any.
pub fn settled_comment(pr_url: &str, shipped: Option<&str>, state: &str, next: &str) -> String {
    let shipped = shipped
        .map(|tag| format!(" That PR already shipped in `{tag}`."))
        .unwrap_or_default();
    format!(
        "Every box of this ticket's contract outside `## Post deploy` is checked now, after its PR {pr_url} was merged, and no PR of it is open.{shipped}\n\nstate: {state}\nnext: {next}\n\n{}",
        settled_marker(pr_url, state)
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
pub(crate) mod tests;
