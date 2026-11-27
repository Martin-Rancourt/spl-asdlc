//! The Livela runner's local socket (`docs/reference/livela-contract.md`,
//! "Socket protocol"): how the `spl-asdlc` program, run apart from the
//! runner, reaches the runner's store.
//!
//! The protocol is exactly [`LivelaStore`], nothing more: one JSON request
//! per line, one JSON response per line, over a Unix socket the runner
//! serves. The runner stays the store's only owner; the program never opens
//! the store itself.
//!
//! - [`Request`] and [`Response`] are the lines, [`Call`] the store methods.
//! - [`handle`] answers one request line from a store: the runner serves the
//!   same protocol with it, or with [`serve_stream`] and [`serve`].
//! - [`SocketStore`] is the program's side: a [`LivelaStore`] whose every
//!   method is a request on the socket.
//!
//! Every request and response carries the SPL-ASDLC major ([`MAJOR`]). A
//! request of another major is refused with a message that says so, never
//! interpreted: its call is not even parsed.
//!
//! Identity: `swap_author` is not on the wire. A write carries its author
//! in the request; the server signs it with that participant, or with
//! `spl-asdlc` when the request names none, and restores the store's
//! previous author after the write itself.

use std::cell::RefCell;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{LivelaDecision, LivelaItem, LivelaProject, LivelaRelation, LivelaStore};

/// The SPL-ASDLC major this crate speaks on the socket.
pub const MAJOR: u64 = parse_major(env!("CARGO_PKG_VERSION_MAJOR"));

const fn parse_major(s: &str) -> u64 {
    let b = s.as_bytes();
    let mut n = 0u64;
    let mut i = 0;
    while i < b.len() {
        n = n * 10 + (b[i] - b'0') as u64;
        i += 1;
    }
    n
}

/// How long the program waits for the runner's answer to one request.
pub const TIMEOUT: Duration = Duration::from_secs(60);

/// One request line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    /// The SPL-ASDLC major of the sender.
    pub major: u64,
    /// The participant that signs a write; `None` signs it `spl-asdlc`.
    /// Ignored on a read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    pub call: Call,
}

/// One response line: `ok` with the method's value, or `error`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    /// The SPL-ASDLC major of the server.
    pub major: u64,
    /// The value, `null` included (a method that answers `None`).
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub ok: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// A key present with `null` is `Some(Null)`; only an absent key is `None`.
fn present<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(d).map(Some)
}

impl Response {
    fn ok(value: Value) -> Response {
        Response {
            major: MAJOR,
            ok: Some(value),
            error: None,
        }
    }

    fn error(message: impl Into<String>) -> Response {
        Response {
            major: MAJOR,
            ok: None,
            error: Some(message.into()),
        }
    }
}

/// The methods of [`LivelaStore`] but `swap_author`, plus `hello`, which
/// answers the server's major.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", content = "args", rename_all = "snake_case")]
pub enum Call {
    Hello,
    BridgeParticipant,
    ExistingBridgeParticipant,
    ParticipantsNamed {
        name: String,
    },
    ParticipantName {
        id: String,
    },
    ParticipantIsHuman {
        id: String,
    },
    Project {
        reference: String,
    },
    Resolve {
        short_id: String,
    },
    Item {
        id: String,
    },
    Items {
        project: String,
        tag: Option<String>,
        parent: Option<String>,
    },
    DuplicatesOf {
        id: String,
    },
    TicketStatuses,
    FindByWebLink {
        url: String,
    },
    CreateTicket {
        project: String,
        title: String,
        description: String,
        tags: Vec<String>,
        assignee: Option<String>,
        parent: Option<String>,
        priority: Option<u8>,
        status: String,
    },
    SetStatus {
        id: String,
        status: String,
    },
    SetDescription {
        id: String,
        text: String,
    },
    AddTag {
        id: String,
        tag: String,
    },
    RemoveTag {
        id: String,
        tag: String,
    },
    Assign {
        id: String,
        participant: Option<String>,
    },
    Comment {
        id: String,
        author: String,
        text: String,
    },
    Log {
        id: String,
        author: String,
        text: String,
    },
    AddRelation {
        id: String,
        kind: LivelaRelation,
        target: String,
    },
    RemoveRelation {
        id: String,
        kind: LivelaRelation,
        target: String,
    },
    AddWebLink {
        id: String,
        url: String,
        title: String,
    },
    SetPrStatus {
        id: String,
        url: String,
        code: String,
    },
    CreatesTasks,
    CreateTask {
        project: String,
        title: String,
        description: String,
        tags: Vec<String>,
        assignee: Option<String>,
        parent: Option<String>,
        priority: Option<u8>,
        status: String,
    },
    IsTask {
        id: String,
    },
    MakeTicket {
        id: String,
    },
    CreatesMilestones,
    CreateMilestone {
        project: String,
        title: String,
        description: String,
        parent: String,
    },
    HoldsPrReviews,
    PrReviews {
        id: String,
        url: String,
    },
    SetPrReviews {
        id: String,
        url: String,
        code: String,
    },
    HoldsImplementationBlocks,
    ImplementationBlockers {
        id: String,
    },
    AddImplementationBlock {
        id: String,
        target: String,
    },
    HoldsDecisions,
    Decisions {
        id: String,
    },
    RequestDecision {
        id: String,
        author: String,
        question: String,
    },
    WithdrawDecision {
        request: String,
        by: String,
    },
}

impl Call {
    /// Whether the call writes to the store: the server signs it.
    pub fn writes(&self) -> bool {
        matches!(
            self,
            Call::CreateTicket { .. }
                | Call::SetStatus { .. }
                | Call::SetDescription { .. }
                | Call::AddTag { .. }
                | Call::RemoveTag { .. }
                | Call::Assign { .. }
                | Call::Comment { .. }
                | Call::Log { .. }
                | Call::AddRelation { .. }
                | Call::RemoveRelation { .. }
                | Call::AddWebLink { .. }
                | Call::SetPrStatus { .. }
                | Call::CreateTask { .. }
                | Call::MakeTicket { .. }
                | Call::CreateMilestone { .. }
                | Call::SetPrReviews { .. }
                | Call::AddImplementationBlock { .. }
                | Call::RequestDecision { .. }
                | Call::WithdrawDecision { .. }
        )
    }
}

/// The refusal of a line of another major (or of none): said, never
/// interpreted.
pub fn other_major(theirs: Option<u64>) -> String {
    match theirs {
        Some(m) => format!(
            "this socket speaks SPL-ASDLC major {MAJOR}; the request is major {m}: refused, not interpreted (install the spl-asdlc program at major {MAJOR})"
        ),
        None => format!(
            "this socket speaks SPL-ASDLC major {MAJOR}; the request carries no major: refused, not interpreted"
        ),
    }
}

/// Encode a line (no newline).
pub fn encode<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).expect("a protocol value always serializes")
}

/// Decode a request line. The major is read first: a line of another major
/// is refused before its call is parsed.
pub fn decode_request(line: &str) -> Result<Request, String> {
    let value: Value =
        serde_json::from_str(line).map_err(|e| format!("a request is not JSON: {e}"))?;
    let major = value.get("major").and_then(Value::as_u64);
    if major != Some(MAJOR) {
        return Err(other_major(major));
    }
    serde_json::from_value(value).map_err(|e| format!("a request is not understood: {e}"))
}

/// Decode a response line. A response of another major is refused.
pub fn decode_response(line: &str) -> Result<Response, String> {
    let value: Value =
        serde_json::from_str(line).map_err(|e| format!("a response is not JSON: {e}"))?;
    let major = value.get("major").and_then(Value::as_u64);
    if major != Some(MAJOR) {
        return Err(match major {
            Some(m) => format!(
                "the Livela runner's socket speaks SPL-ASDLC major {m}; this program is major {MAJOR}: refused"
            ),
            None => "the Livela runner's socket answered with no major: refused".to_string(),
        });
    }
    serde_json::from_value(value).map_err(|e| format!("a response is not understood: {e}"))
}

fn to_value<T: Serialize>(v: T) -> Value {
    serde_json::to_value(v).expect("a protocol value always serializes")
}

fn answer<T: Serialize>(r: Result<T, String>) -> Response {
    match r {
        Ok(v) => Response::ok(to_value(v)),
        Err(e) => Response::error(e),
    }
}

/// Answer one request line from `store`: the response line, without its
/// newline. A write is signed by the request's author (`spl-asdlc` when it
/// names none), and the store's previous author is restored after it.
pub fn handle<S: LivelaStore + ?Sized>(store: &mut S, line: &str) -> String {
    let response = match decode_request(line) {
        Ok(request) => respond(store, request),
        Err(e) => Response::error(e),
    };
    encode(&response)
}

fn respond<S: LivelaStore + ?Sized>(store: &mut S, request: Request) -> Response {
    if !request.call.writes() {
        return call(store, request.call);
    }
    let author = match request.author.filter(|a| !a.is_empty()) {
        Some(a) => a,
        None => match store.bridge_participant() {
            Ok(id) => id,
            Err(e) => return Response::error(format!("the spl-asdlc participant: {e}")),
        },
    };
    let previous = store.swap_author(&author);
    let r = call(store, request.call);
    store.swap_author(&previous);
    r
}

fn call<S: LivelaStore + ?Sized>(s: &mut S, call: Call) -> Response {
    let ok = |v: Value| Response::ok(v);
    match call {
        Call::Hello => ok(to_value(MAJOR)),
        Call::BridgeParticipant => answer(s.bridge_participant()),
        Call::ExistingBridgeParticipant => ok(to_value(s.existing_bridge_participant())),
        Call::ParticipantsNamed { name } => ok(to_value(s.participants_named(&name))),
        Call::ParticipantName { id } => ok(to_value(s.participant_name(&id))),
        Call::ParticipantIsHuman { id } => ok(to_value(s.participant_is_human(&id))),
        Call::Project { reference } => answer(s.project(&reference)),
        Call::Resolve { short_id } => answer(s.resolve(&short_id)),
        Call::Item { id } => answer(s.item(&id)),
        Call::Items {
            project,
            tag,
            parent,
        } => ok(to_value(s.items(
            &project,
            tag.as_deref(),
            parent.as_deref(),
        ))),
        Call::DuplicatesOf { id } => ok(to_value(s.duplicates_of(&id))),
        Call::TicketStatuses => ok(to_value(s.ticket_statuses())),
        Call::FindByWebLink { url } => ok(to_value(s.find_by_web_link(&url))),
        Call::CreateTicket {
            project,
            title,
            description,
            tags,
            assignee,
            parent,
            priority,
            status,
        } => answer(s.create_ticket(
            &project,
            &title,
            &description,
            &tags,
            assignee.as_deref(),
            parent.as_deref(),
            priority,
            &status,
        )),
        Call::SetStatus { id, status } => answer(s.set_status(&id, &status)),
        Call::SetDescription { id, text } => answer(s.set_description(&id, &text)),
        Call::AddTag { id, tag } => answer(s.add_tag(&id, &tag)),
        Call::RemoveTag { id, tag } => answer(s.remove_tag(&id, &tag)),
        Call::Assign { id, participant } => answer(s.assign(&id, participant.as_deref())),
        Call::Comment { id, author, text } => answer(s.comment(&id, &author, &text)),
        Call::Log { id, author, text } => answer(s.log(&id, &author, &text)),
        Call::AddRelation { id, kind, target } => answer(s.add_relation(&id, kind, &target)),
        Call::RemoveRelation { id, kind, target } => answer(s.remove_relation(&id, kind, &target)),
        Call::AddWebLink { id, url, title } => answer(s.add_web_link(&id, &url, &title)),
        Call::SetPrStatus { id, url, code } => answer(s.set_pr_status(&id, &url, &code)),
        Call::CreatesTasks => ok(to_value(s.creates_tasks())),
        Call::CreateTask {
            project,
            title,
            description,
            tags,
            assignee,
            parent,
            priority,
            status,
        } => answer(s.create_task(
            &project,
            &title,
            &description,
            &tags,
            assignee.as_deref(),
            parent.as_deref(),
            priority,
            &status,
        )),
        Call::IsTask { id } => answer(s.is_task(&id)),
        Call::MakeTicket { id } => answer(s.make_ticket(&id)),
        Call::CreatesMilestones => ok(to_value(s.creates_milestones())),
        Call::CreateMilestone {
            project,
            title,
            description,
            parent,
        } => answer(s.create_milestone(&project, &title, &description, &parent)),
        Call::HoldsPrReviews => ok(to_value(s.holds_pr_reviews())),
        Call::PrReviews { id, url } => ok(to_value(s.pr_reviews(&id, &url))),
        Call::SetPrReviews { id, url, code } => answer(s.set_pr_reviews(&id, &url, &code)),
        Call::HoldsImplementationBlocks => ok(to_value(s.holds_implementation_blocks())),
        Call::ImplementationBlockers { id } => answer(s.implementation_blockers(&id)),
        Call::AddImplementationBlock { id, target } => {
            answer(s.add_implementation_block(&id, &target))
        }
        Call::HoldsDecisions => ok(to_value(s.holds_decisions())),
        Call::Decisions { id } => answer(s.decisions(&id)),
        Call::RequestDecision {
            id,
            author,
            question,
        } => answer(s.request_decision(&id, &author, &question)),
        Call::WithdrawDecision { request, by } => answer(s.withdraw_decision(&request, &by)),
    }
}

/// Serve one connection: each request line gets its response line from
/// `answer` (usually `|line| handle(&mut store, line)`), until the client
/// closes it. A host that shares its store with other writers takes its lock
/// inside `answer`, one request at a time.
pub fn serve_stream_with(
    stream: UnixStream,
    mut answer: impl FnMut(&str) -> String,
) -> io::Result<()> {
    let mut writer = stream.try_clone()?;
    let reader = BufReader::new(stream);
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let mut response = answer(&line);
        response.push('\n');
        writer.write_all(response.as_bytes())?;
        writer.flush()?;
    }
    Ok(())
}

/// Serve one connection from `store`.
pub fn serve_stream<S: LivelaStore + ?Sized>(store: &mut S, stream: UnixStream) -> io::Result<()> {
    serve_stream_with(stream, |line| handle(store, line))
}

/// Serve the connections of `listener` from `store`, one after the other,
/// for ever. A connection that fails costs only itself.
pub fn serve<S: LivelaStore + ?Sized>(listener: &UnixListener, store: &mut S) -> io::Result<()> {
    for stream in listener.incoming() {
        let _ = serve_stream(store, stream?);
    }
    Ok(())
}

struct Connection {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
}

impl Connection {
    fn open(path: &Path) -> Result<Connection, String> {
        let stream = UnixStream::connect(path).map_err(|e| {
            format!(
                "the Livela runner's socket {} could not be opened: {e}",
                path.display()
            )
        })?;
        stream
            .set_read_timeout(Some(TIMEOUT))
            .and_then(|_| stream.set_write_timeout(Some(TIMEOUT)))
            .map_err(|e| e.to_string())?;
        let writer = stream.try_clone().map_err(|e| e.to_string())?;
        Ok(Connection {
            reader: BufReader::new(stream),
            writer,
        })
    }

    fn exchange(&mut self, line: &str) -> io::Result<String> {
        self.writer.write_all(line.as_bytes())?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()?;
        let mut response = String::new();
        if self.reader.read_line(&mut response)? == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "the runner closed the socket",
            ));
        }
        Ok(response)
    }
}

/// The program's side: a [`LivelaStore`] whose every method is a request on
/// the Livela runner's socket.
///
/// A method that returns a `Result` reports a broken socket as its error. A
/// method that cannot (a capability, a list) panics instead: an empty list
/// read from a dead socket would make the bridge create again what exists.
/// The pass stops before it writes on a guess.
pub struct SocketStore {
    path: PathBuf,
    connection: RefCell<Connection>,
    /// The author `swap_author` set; each write carries it.
    author: Option<String>,
}

impl SocketStore {
    /// Connect to the runner's socket and check that it speaks this major.
    pub fn connect(path: &Path) -> Result<SocketStore, String> {
        let store = SocketStore {
            path: path.to_path_buf(),
            connection: RefCell::new(Connection::open(path)?),
            author: None,
        };
        let theirs: u64 = store.request(Call::Hello)?;
        if theirs != MAJOR {
            return Err(format!(
                "the Livela runner's socket {} speaks SPL-ASDLC major {theirs}; this program is major {MAJOR}",
                path.display()
            ));
        }
        Ok(store)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn request<T: DeserializeOwned>(&self, call: Call) -> Result<T, String> {
        let author = if call.writes() {
            self.author.clone()
        } else {
            None
        };
        let line = encode(&Request {
            major: MAJOR,
            author,
            call,
        });
        let broken = |e: String| {
            format!(
                "the Livela runner's socket {} did not answer: {e}",
                self.path.display()
            )
        };
        let raw = self
            .connection
            .borrow_mut()
            .exchange(&line)
            .map_err(|e| broken(e.to_string()))?;
        let response = decode_response(raw.trim_end())?;
        match (response.ok, response.error) {
            (_, Some(e)) => Err(e),
            (Some(v), None) => {
                serde_json::from_value(v).map_err(|e| format!("a response is not understood: {e}"))
            }
            (None, None) => Err("a response carries neither ok nor error".to_string()),
        }
    }

    /// A method with no room for an error: a broken socket stops the pass.
    fn must<T: DeserializeOwned>(&self, call: Call) -> T {
        self.request(call).unwrap_or_else(|e| panic!("{e}"))
    }
}

impl LivelaStore for SocketStore {
    fn bridge_participant(&mut self) -> Result<String, String> {
        self.request(Call::BridgeParticipant)
    }
    fn existing_bridge_participant(&self) -> Option<String> {
        self.must(Call::ExistingBridgeParticipant)
    }
    fn swap_author(&mut self, participant: &str) -> String {
        let next = Some(participant.to_string()).filter(|p| !p.is_empty());
        std::mem::replace(&mut self.author, next).unwrap_or_default()
    }
    fn participants_named(&self, name: &str) -> Vec<(String, String)> {
        self.must(Call::ParticipantsNamed { name: name.into() })
    }
    fn participant_name(&self, id: &str) -> Option<String> {
        self.must(Call::ParticipantName { id: id.into() })
    }
    fn participant_is_human(&self, id: &str) -> Option<bool> {
        self.must(Call::ParticipantIsHuman { id: id.into() })
    }
    fn project(&self, reference: &str) -> Result<Option<LivelaProject>, String> {
        self.request(Call::Project {
            reference: reference.into(),
        })
    }
    fn resolve(&self, short_id: &str) -> Result<Option<String>, String> {
        self.request(Call::Resolve {
            short_id: short_id.into(),
        })
    }
    fn item(&self, id: &str) -> Result<LivelaItem, String> {
        self.request(Call::Item { id: id.into() })
    }
    fn items(&self, project: &str, tag: Option<&str>, parent: Option<&str>) -> Vec<LivelaItem> {
        self.must(Call::Items {
            project: project.into(),
            tag: tag.map(Into::into),
            parent: parent.map(Into::into),
        })
    }
    fn duplicates_of(&self, id: &str) -> Vec<String> {
        self.must(Call::DuplicatesOf { id: id.into() })
    }
    fn ticket_statuses(&self) -> Vec<String> {
        self.must(Call::TicketStatuses)
    }
    fn find_by_web_link(&self, url: &str) -> Vec<String> {
        self.must(Call::FindByWebLink { url: url.into() })
    }
    fn create_ticket(
        &mut self,
        project: &str,
        title: &str,
        description: &str,
        tags: &[String],
        assignee: Option<&str>,
        parent: Option<&str>,
        priority: Option<u8>,
        status: &str,
    ) -> Result<String, String> {
        self.request(Call::CreateTicket {
            project: project.into(),
            title: title.into(),
            description: description.into(),
            tags: tags.to_vec(),
            assignee: assignee.map(Into::into),
            parent: parent.map(Into::into),
            priority,
            status: status.into(),
        })
    }
    fn set_status(&mut self, id: &str, status: &str) -> Result<(), String> {
        self.request(Call::SetStatus {
            id: id.into(),
            status: status.into(),
        })
    }
    fn set_description(&mut self, id: &str, text: &str) -> Result<(), String> {
        self.request(Call::SetDescription {
            id: id.into(),
            text: text.into(),
        })
    }
    fn add_tag(&mut self, id: &str, tag: &str) -> Result<(), String> {
        self.request(Call::AddTag {
            id: id.into(),
            tag: tag.into(),
        })
    }
    fn remove_tag(&mut self, id: &str, tag: &str) -> Result<(), String> {
        self.request(Call::RemoveTag {
            id: id.into(),
            tag: tag.into(),
        })
    }
    fn assign(&mut self, id: &str, participant: Option<&str>) -> Result<(), String> {
        self.request(Call::Assign {
            id: id.into(),
            participant: participant.map(Into::into),
        })
    }
    fn comment(&mut self, id: &str, author: &str, text: &str) -> Result<String, String> {
        self.request(Call::Comment {
            id: id.into(),
            author: author.into(),
            text: text.into(),
        })
    }
    fn log(&mut self, id: &str, author: &str, text: &str) -> Result<String, String> {
        self.request(Call::Log {
            id: id.into(),
            author: author.into(),
            text: text.into(),
        })
    }
    fn add_relation(&mut self, id: &str, kind: LivelaRelation, target: &str) -> Result<(), String> {
        self.request(Call::AddRelation {
            id: id.into(),
            kind,
            target: target.into(),
        })
    }
    fn remove_relation(
        &mut self,
        id: &str,
        kind: LivelaRelation,
        target: &str,
    ) -> Result<(), String> {
        self.request(Call::RemoveRelation {
            id: id.into(),
            kind,
            target: target.into(),
        })
    }
    fn add_web_link(&mut self, id: &str, url: &str, title: &str) -> Result<(), String> {
        self.request(Call::AddWebLink {
            id: id.into(),
            url: url.into(),
            title: title.into(),
        })
    }
    fn set_pr_status(&mut self, id: &str, url: &str, code: &str) -> Result<(), String> {
        self.request(Call::SetPrStatus {
            id: id.into(),
            url: url.into(),
            code: code.into(),
        })
    }
    fn creates_tasks(&self) -> bool {
        self.must(Call::CreatesTasks)
    }
    fn create_task(
        &mut self,
        project: &str,
        title: &str,
        description: &str,
        tags: &[String],
        assignee: Option<&str>,
        parent: Option<&str>,
        priority: Option<u8>,
        status: &str,
    ) -> Result<String, String> {
        self.request(Call::CreateTask {
            project: project.into(),
            title: title.into(),
            description: description.into(),
            tags: tags.to_vec(),
            assignee: assignee.map(Into::into),
            parent: parent.map(Into::into),
            priority,
            status: status.into(),
        })
    }
    fn is_task(&self, id: &str) -> Result<bool, String> {
        self.request(Call::IsTask { id: id.into() })
    }
    fn make_ticket(&mut self, id: &str) -> Result<(), String> {
        self.request(Call::MakeTicket { id: id.into() })
    }
    fn creates_milestones(&self) -> bool {
        self.must(Call::CreatesMilestones)
    }
    fn create_milestone(
        &mut self,
        project: &str,
        title: &str,
        description: &str,
        parent: &str,
    ) -> Result<String, String> {
        self.request(Call::CreateMilestone {
            project: project.into(),
            title: title.into(),
            description: description.into(),
            parent: parent.into(),
        })
    }
    fn holds_pr_reviews(&self) -> bool {
        self.must(Call::HoldsPrReviews)
    }
    fn pr_reviews(&self, id: &str, url: &str) -> Option<String> {
        self.must(Call::PrReviews {
            id: id.into(),
            url: url.into(),
        })
    }
    fn set_pr_reviews(&mut self, id: &str, url: &str, code: &str) -> Result<(), String> {
        self.request(Call::SetPrReviews {
            id: id.into(),
            url: url.into(),
            code: code.into(),
        })
    }
    fn holds_implementation_blocks(&self) -> bool {
        self.must(Call::HoldsImplementationBlocks)
    }
    fn implementation_blockers(&self, id: &str) -> Result<Vec<String>, String> {
        self.request(Call::ImplementationBlockers { id: id.into() })
    }
    fn add_implementation_block(&mut self, id: &str, target: &str) -> Result<(), String> {
        self.request(Call::AddImplementationBlock {
            id: id.into(),
            target: target.into(),
        })
    }
    fn holds_decisions(&self) -> bool {
        self.must(Call::HoldsDecisions)
    }
    fn decisions(&self, id: &str) -> Result<Vec<LivelaDecision>, String> {
        self.request(Call::Decisions { id: id.into() })
    }
    fn request_decision(
        &mut self,
        id: &str,
        author: &str,
        question: &str,
    ) -> Result<String, String> {
        self.request(Call::RequestDecision {
            id: id.into(),
            author: author.into(),
            question: question.into(),
        })
    }
    fn withdraw_decision(&mut self, request: &str, by: &str) -> Result<(), String> {
        self.request(Call::WithdrawDecision {
            request: request.into(),
            by: by.into(),
        })
    }
}
