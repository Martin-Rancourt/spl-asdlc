//! `spl-asdlc.toml`: the per-repository configuration (`docs/spec/08-configuration.md`).
//!
//! The file is parsed into [`Config`] with every default applied, or refused
//! with the full list of what is wrong. Nothing falls back to a default shape
//! when the file is invalid: an invalid file stops the bridge for that
//! repository (fail closed).
//!
//! The repository's file may leave the identities of an installation to an
//! instance overlay kept on the host ([`overlay`]); the two are merged before
//! anything is validated, and a key set in both is an error.

pub mod overlay;
mod raw;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

use raw::RawConfig;

pub use overlay::{INSTANCE_FILE_NAME, instance_path, sibling_of};

/// The only schema version this build reads.
pub const SCHEMA_VERSION: i64 = 1;

/// Default file name, at the root of the repository.
pub const FILE_NAME: &str = "spl-asdlc.toml";

/// Canonical statuses of a work item (`docs/spec/03-vocabulary.md`).
pub const CANONICAL_STATUSES: [&str; 9] = [
    "Triage",
    "Backlog",
    "Ready",
    "In Progress",
    "In Review",
    "To Deploy",
    "Post Deploy",
    "Done",
    "Canceled",
];

/// Events a notification channel may subscribe to.
pub const NOTIFY_EVENTS: [&str; 5] = [
    "release-note",
    "escalation",
    "stagnation",
    "stuck-pr",
    "blind-pass",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub product: Product,
    pub owner: Owner,
    pub tracker: Tracker,
    pub release: Release,
    pub review: Review,
    pub bridge: Bridge,
    pub notify: Vec<Notify>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Product {
    pub name: String,
    /// `owner/name` on GitHub.
    pub repository: String,
}

/// The human of the method (`docs/spec/00-overview.md`): takes escalations
/// and, under `go_policy = "human"`, approves the GO.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Owner {
    pub tracker_user: String,
    pub github_login: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackerKind {
    Linear,
    Livela,
}

impl fmt::Display for TrackerKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            TrackerKind::Linear => "linear",
            TrackerKind::Livela => "livela",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkFallback {
    /// Only the tracker's own PR link (attachment / web link) names a ticket.
    None,
    /// The tracker item URL in the PR body is accepted when the tracker has no link.
    BodyUrl,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tracker {
    pub kind: TrackerKind,
    pub team: String,
    pub service_identity: String,
    /// Identifier prefixes of the team's items; empty means "ask the tracker".
    pub team_keys: Vec<String>,
    pub link_fallback: LinkFallback,
    /// Canonical status -> tracker status, for every canonical status.
    pub statuses: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaterPasses {
    Full,
    GateAndCoordinator,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoPolicy {
    Computed,
    Human,
}

/// How the nits of a release become tickets (spec 05 §6): one shared
/// ticket per release, for a tracker that limits its ticket count, or one
/// ticket per nit, for a tracker that does not. Each carries a contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NitsGrouping {
    Shared,
    PerNit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Angle {
    pub key: String,
    pub title: String,
    pub assignee: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub candidate_tag: String,
    pub release_tag: String,
    pub branch_prefix: String,
    pub max_rc_passes: u32,
    pub later_passes: LaterPasses,
    pub go_policy: GoPolicy,
    pub nits: NitsGrouping,
    pub record: String,
    pub components: Vec<String>,
    pub version_component: Option<String>,
    pub release_pr_label: String,
    pub release_please_authors: Vec<String>,
    pub gate_record: Option<String>,
    pub bookkeeping_paths: Vec<String>,
    pub registry_path: String,
    pub business_rule_paths: Vec<String>,
    pub stagnation_hours: u32,
    pub coordinators: Vec<String>,
    pub escalation_label: String,
    pub auto_candidate: bool,
    pub cut_workflow: String,
    pub gate_workflow: String,
    /// The workflow whose `workflow_run` of a red gate reruns it once when
    /// it is a runner abort or a known flake: a red gate's work waits for it.
    pub gate_rerun_workflow: String,
    /// How long after a red gate run completed the bridge waits for a run of
    /// `gate_rerun_workflow` to show up before it opens the gate-fix ticket
    /// anyway (the rerun workflow may be absent or broken).
    pub gate_rerun_wait_minutes: u32,
    pub release_please_workflow: String,
    /// release-please's configuration and manifest, at the repository root
    /// unless the product names them otherwise.
    pub release_please_config: String,
    pub release_please_manifest: String,
    pub angles: Vec<Angle>,
}

impl Release {
    pub fn multi_component(&self) -> bool {
        !self.components.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeMethod {
    Squash,
    Merge,
}

impl From<MergeMethod> for crate::github::MergeMethod {
    fn from(m: MergeMethod) -> Self {
        match m {
            MergeMethod::Squash => crate::github::MergeMethod::Squash,
            MergeMethod::Merge => crate::github::MergeMethod::Merge,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HumanApproval {
    Never,
    SensitivePaths,
    Always,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathAngle {
    pub paths: Vec<String>,
    pub angle: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Review {
    pub merge_method: MergeMethod,
    pub require_up_to_date: bool,
    /// Changed lines above which `diff_size_angle` is added; 0 disables it.
    pub diff_size_extra_angle: u32,
    pub diff_size_angle: Option<String>,
    pub human_approval: HumanApproval,
    /// Path of the sensitive-path list, read at the PR's base; `None` means
    /// the project declares no sensitive paths.
    pub sensitive_paths: Option<String>,
    pub sensitive_angle: String,
    pub coordinators: Vec<String>,
    pub human_reviewer: Option<String>,
    /// Apps whose PRs need no ticket: the bridge opens one for each.
    pub ticketless_authors: Vec<String>,
    pub angles: Vec<Angle>,
    pub path_angles: Vec<PathAngle>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GithubIdentity {
    App,
    Token,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bridge {
    pub poll_minutes: u32,
    pub github_identity: GithubIdentity,
    pub sweep_hours: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifyKind {
    Telegram,
    Email,
    GoogleChat,
}

/// A secret is never a value in the file, only the name of where to read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretRef {
    Env(String),
}

impl fmt::Display for SecretRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SecretRef::Env(name) => write!(f, "env:{name}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notify {
    pub kind: NotifyKind,
    pub target: SecretRef,
    pub token: Option<SecretRef>,
    pub events: Vec<String>,
}

/// One thing wrong with the file, named by its key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    /// The file the error comes from, when an instance overlay is merged
    /// (one file alone is named by whoever read it).
    pub file: Option<String>,
    pub key: String,
    pub message: String,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(file) = &self.file {
            write!(f, "{file}: ")?;
        }
        if self.key.is_empty() {
            write!(f, "{}", self.message)
        } else {
            write!(f, "{}: {}", self.key, self.message)
        }
    }
}

/// Every problem found; never empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigErrors(pub Vec<ConfigError>);

impl fmt::Display for ConfigErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, e) in self.0.iter().enumerate() {
            if i > 0 {
                writeln!(f)?;
            }
            write!(f, "{e}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ConfigErrors {}

/// A configuration file as read: what names it in a message, and its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub label: String,
    pub text: String,
}

impl Source {
    pub fn read(path: &Path) -> Result<Source, ConfigErrors> {
        let text = std::fs::read_to_string(path).map_err(|e| {
            ConfigErrors(vec![ConfigError {
                file: None,
                key: String::new(),
                message: format!("{} could not be read: {e}", path.display()),
            }])
        })?;
        Ok(Source {
            label: path.display().to_string(),
            text,
        })
    }
}

impl Config {
    /// Read and validate a file on disk, merged with its instance overlay
    /// when one is beside it ([`instance_path`]).
    pub fn load(path: &Path) -> Result<Config, ConfigErrors> {
        Config::load_with(path, None).map(|(c, _)| c)
    }

    /// Read and validate a file on disk with its instance overlay: the one
    /// named, which must exist, or else the one beside the file, if any.
    /// Returns the overlay read, for whoever reports it.
    pub fn load_with(
        path: &Path,
        instance: Option<&Path>,
    ) -> Result<(Config, Option<Source>), ConfigErrors> {
        let repository = Source::read(path)?;
        let overlay = match instance_path(path, instance)? {
            Some(p) => Some(Source::read(&p)?),
            None => None,
        };
        let config = Config::parse_with(&repository, overlay.as_ref())?;
        Ok((config, overlay))
    }

    /// Validate the text of a `spl-asdlc.toml` that stands alone.
    pub fn parse(text: &str) -> Result<Config, ConfigErrors> {
        let raw: RawConfig = toml::from_str(text).map_err(|e| {
            ConfigErrors(vec![ConfigError {
                file: None,
                key: String::new(),
                message: format!("not a valid {FILE_NAME}: {}", e.to_string().trim_end()),
            }])
        })?;
        let mut v = Validator::default();
        let config = v.config(raw);
        match config {
            Some(c) if v.errors.is_empty() => Ok(c),
            _ => Err(ConfigErrors(v.errors)),
        }
    }

    /// Validate the repository's file merged with an instance overlay
    /// (spec 08, "The instance overlay"). Each file is first read for its
    /// own shape, with its lines; every rule is then checked on the merged
    /// result, and each error names the file its value comes from.
    pub fn parse_with(
        repository: &Source,
        instance: Option<&Source>,
    ) -> Result<Config, ConfigErrors> {
        let Some(instance) = instance else {
            return Config::parse(&repository.text);
        };
        let in_repository = |message: String| {
            ConfigErrors(vec![ConfigError {
                file: Some(repository.label.clone()),
                key: String::new(),
                message,
            }])
        };
        let not_valid = |e: toml::de::Error| {
            in_repository(format!(
                "not a valid {FILE_NAME}: {}",
                e.to_string().trim_end()
            ))
        };
        toml::from_str::<RawConfig>(&repository.text).map_err(not_valid)?;
        let mut table: toml::Table = toml::from_str(&repository.text).map_err(not_valid)?;
        let mut merged = overlay::merge(&mut table, &repository.label, instance)?;
        let raw: RawConfig = toml::Value::Table(table).try_into().map_err(|e| {
            in_repository(format!(
                "the merge with {} is not a valid {FILE_NAME}: {e}",
                instance.label
            ))
        })?;
        let mut v = Validator::default();
        let config = v.config(raw);
        let mut errors = std::mem::take(&mut merged.errors);
        errors.extend(v.errors.into_iter().map(|mut e| {
            e.file = Some(if merged.is_from_overlay(&e.key) {
                instance.label.clone()
            } else {
                repository.label.clone()
            });
            e
        }));
        match config {
            Some(c) if errors.is_empty() => Ok(c),
            _ => Err(ConfigErrors(errors)),
        }
    }
}

/// `key`, `title`, `assignee`, `notes` of an angle entry, as written.
type AngleFields = (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

#[derive(Default)]
struct Validator {
    errors: Vec<ConfigError>,
}

impl Validator {
    fn err(&mut self, key: &str, message: impl Into<String>) {
        let mut message = message.into();
        if message.contains("is required") && overlay::is_instance_key(key) {
            message.push_str(
                " (an instance key: the repository's file or its instance overlay carries it)",
            );
        }
        self.errors.push(ConfigError {
            file: None,
            key: key.to_string(),
            message,
        });
    }

    /// A required, non-blank string.
    fn required(&mut self, key: &str, value: Option<String>) -> Option<String> {
        match value {
            Some(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
            Some(_) => {
                self.err(key, "must not be empty");
                None
            }
            None => {
                self.err(key, "is required");
                None
            }
        }
    }

    /// An optional string that, when present, must not be blank.
    fn optional(&mut self, key: &str, value: Option<String>) -> Option<String> {
        match value {
            Some(s) if s.trim().is_empty() => {
                self.err(key, "must not be empty when given");
                None
            }
            other => other.map(|s| s.trim().to_string()),
        }
    }

    fn choice<T: Copy>(
        &mut self,
        key: &str,
        value: Option<String>,
        default: Option<T>,
        accepted: &[(&str, T)],
    ) -> Option<T> {
        let names = || {
            accepted
                .iter()
                .map(|(n, _)| format!("\"{n}\""))
                .collect::<Vec<_>>()
                .join(", ")
        };
        match value {
            None => {
                if default.is_none() {
                    self.err(key, format!("is required (one of {})", names()));
                }
                default
            }
            Some(s) => match accepted.iter().find(|(n, _)| *n == s) {
                Some((_, v)) => Some(*v),
                None => {
                    self.err(key, format!("\"{s}\" is not one of {}", names()));
                    None
                }
            },
        }
    }

    fn positive(&mut self, key: &str, value: Option<i64>, default: u32) -> u32 {
        match value {
            None => default,
            Some(n) if n >= 1 && n <= u32::MAX as i64 => n as u32,
            Some(n) => {
                self.err(key, format!("must be a positive integer, got {n}"));
                default
            }
        }
    }

    fn non_negative(&mut self, key: &str, value: Option<i64>, default: u32) -> u32 {
        match value {
            None => default,
            Some(n) if n >= 0 && n <= u32::MAX as i64 => n as u32,
            Some(n) => {
                self.err(key, format!("must be zero or a positive integer, got {n}"));
                default
            }
        }
    }

    /// A list whose entries must all be non-blank; `required_non_empty`
    /// refuses an absent or empty list.
    fn list(
        &mut self,
        key: &str,
        value: Option<Vec<String>>,
        default: Vec<String>,
        required_non_empty: bool,
    ) -> Vec<String> {
        let list = match value {
            None if required_non_empty => {
                self.err(key, "is required and must name at least one entry");
                return Vec::new();
            }
            None => return default,
            Some(l) => l,
        };
        if required_non_empty && list.is_empty() {
            self.err(key, "must name at least one entry");
        }
        let mut out = Vec::new();
        let mut seen = BTreeSet::new();
        for (i, s) in list.into_iter().enumerate() {
            let t = s.trim().to_string();
            if t.is_empty() {
                self.err(&format!("{key}[{i}]"), "must not be empty");
            } else if !seen.insert(t.clone()) {
                self.err(&format!("{key}[{i}]"), format!("\"{t}\" is listed twice"));
            } else {
                out.push(t);
            }
        }
        out
    }

    fn secret(&mut self, key: &str, value: &str) -> Option<SecretRef> {
        match value.strip_prefix("env:") {
            Some(name) if is_env_name(name) => Some(SecretRef::Env(name.to_string())),
            _ => {
                self.err(
                    key,
                    "must be a secret reference `env:<VARIABLE>` (a value is never written in the file)",
                );
                None
            }
        }
    }

    fn config(&mut self, raw: RawConfig) -> Option<Config> {
        match raw.schema {
            Some(SCHEMA_VERSION) => {}
            Some(n) => self.err(
                "schema",
                format!("version {n} is not read by this build (expected {SCHEMA_VERSION})"),
            ),
            None => self.err("schema", format!("is required (schema = {SCHEMA_VERSION})")),
        }
        // Read before the release table is consumed, so that a bad value
        // elsewhere in [release] does not hide the owner's requirement.
        let human_go = raw.release.as_ref().and_then(|r| r.go_policy.as_deref()) == Some("human");
        let product = self.product(raw.product);
        let tracker = self.tracker(raw.tracker);
        let release = self.release(raw.release, product.as_ref());
        let review = self.review(raw.review);
        let owner = self.owner(raw.owner, human_go);
        let bridge = self.bridge(raw.bridge);
        let notify = self.notify(raw.notify);
        Some(Config {
            product: product?,
            owner: owner?,
            tracker: tracker?,
            release: release?,
            review: review?,
            bridge,
            notify,
        })
    }

    fn product(&mut self, raw: Option<raw::RawProduct>) -> Option<Product> {
        let Some(raw) = raw else {
            self.err("product", "the [product] table is required");
            return None;
        };
        let name = self.required("product.name", raw.name);
        if let Some(n) = &name {
            if !is_product_name(n) {
                self.err(
                    "product.name",
                    format!(
                        "\"{n}\" must be lower-case letters, digits and hyphens, starting with a letter or digit (it is used in tags and labels)"
                    ),
                );
            }
        }
        let repository = self.required("product.repository", raw.repository);
        if let Some(r) = &repository {
            if !is_repository(r) {
                self.err(
                    "product.repository",
                    format!("\"{r}\" must be `<owner>/<repo>` as GitHub names it"),
                );
            }
        }
        Some(Product {
            name: name?,
            repository: repository?,
        })
    }

    fn owner(&mut self, raw: Option<raw::RawOwner>, human_go: bool) -> Option<Owner> {
        let Some(raw) = raw else {
            self.err(
                "owner",
                "the [owner] table is required: escalations are assigned to the owner",
            );
            return None;
        };
        let tracker_user = self.required("owner.tracker_user", raw.tracker_user);
        let github_login = self.optional("owner.github_login", raw.github_login);
        if github_login.is_none() && human_go {
            self.err(
                "owner.github_login",
                "is required when release.go_policy = \"human\": the owner's approval is read on GitHub",
            );
        }
        Some(Owner {
            tracker_user: tracker_user?,
            github_login,
        })
    }

    fn tracker(&mut self, raw: Option<raw::RawTracker>) -> Option<Tracker> {
        let Some(raw) = raw else {
            self.err("tracker", "the [tracker] table is required");
            return None;
        };
        let kind = self.choice(
            "tracker.kind",
            raw.kind,
            None,
            &[
                ("linear", TrackerKind::Linear),
                ("livela", TrackerKind::Livela),
            ],
        );
        let team = self.required("tracker.team", raw.team);
        let service_identity = self.required("tracker.service_identity", raw.service_identity);
        let team_keys = self.list("tracker.team_keys", raw.team_keys, Vec::new(), false);
        for (i, k) in team_keys.iter().enumerate() {
            let mut chars = k.chars();
            let ok = chars.next().is_some_and(|c| c.is_ascii_uppercase())
                && chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
                && k.len() <= 10;
            if !ok {
                self.err(
                    &format!("tracker.team_keys[{i}]"),
                    format!(
                        "\"{k}\" must be capital letters and digits, starting with a letter (an identifier prefix such as `ABC`)"
                    ),
                );
            }
        }
        let link_fallback = self.choice(
            "tracker.link_fallback",
            raw.link_fallback,
            Some(LinkFallback::None),
            &[
                ("none", LinkFallback::None),
                ("body-url", LinkFallback::BodyUrl),
            ],
        );
        let mut statuses: BTreeMap<String, String> = CANONICAL_STATUSES
            .iter()
            .map(|s| (s.to_string(), s.to_string()))
            .collect();
        for (canonical, name) in raw.statuses.unwrap_or_default() {
            let key = format!("tracker.statuses.\"{canonical}\"");
            if !CANONICAL_STATUSES.contains(&canonical.as_str()) {
                self.err(
                    &key,
                    format!(
                        "\"{canonical}\" is not a canonical status (one of {})",
                        CANONICAL_STATUSES.join(", ")
                    ),
                );
            } else if name.trim().is_empty() {
                self.err(&key, "must name the tracker's status");
            } else {
                statuses.insert(canonical, name.trim().to_string());
            }
        }
        let mut seen: BTreeMap<&str, &str> = BTreeMap::new();
        for (canonical, name) in &statuses {
            if let Some(other) = seen.insert(name.as_str(), canonical.as_str()) {
                self.err(
                    "tracker.statuses",
                    format!(
                        "\"{other}\" and \"{canonical}\" both map to the tracker status \"{name}\": the bridge could not tell them apart"
                    ),
                );
            }
        }
        Some(Tracker {
            kind: kind?,
            team: team?,
            service_identity: service_identity?,
            team_keys,
            link_fallback: link_fallback?,
            statuses,
        })
    }

    fn angles(&mut self, key: &str, raw: Vec<AngleFields>) -> Vec<Angle> {
        if raw.is_empty() {
            self.err(
                key,
                "at least one angle is required: a review with no angle computes its verdict from nobody",
            );
        }
        let mut out = Vec::new();
        let mut seen = BTreeSet::new();
        for (i, (k, title, assignee, notes)) in raw.into_iter().enumerate() {
            let at = format!("{key}[{i}]");
            let Some(k) = self.required(&format!("{at}.key"), k) else {
                continue;
            };
            if !is_angle_key(&k) {
                self.err(
                    &format!("{at}.key"),
                    format!(
                        "\"{k}\" must be lower-case letters, digits and hyphens (it names the branch `qa/<candidate>/<angle>`)"
                    ),
                );
                continue;
            }
            if !seen.insert(k.clone()) {
                self.err(&format!("{at}.key"), format!("\"{k}\" is configured twice"));
                continue;
            }
            let title = self
                .optional(&format!("{at}.title"), title)
                .unwrap_or_else(|| default_title(&k));
            let assignee = self.optional(&format!("{at}.assignee"), assignee);
            let notes = self.optional(&format!("{at}.notes"), notes);
            out.push(Angle {
                key: k,
                title,
                assignee,
                notes,
            });
        }
        out
    }

    fn release(
        &mut self,
        raw: Option<raw::RawRelease>,
        product: Option<&Product>,
    ) -> Option<Release> {
        let Some(raw) = raw else {
            self.err("release", "the [release] table is required");
            return None;
        };
        let product_name = product.map(|p| p.name.as_str()).unwrap_or("product");

        let candidate_tag = raw
            .candidate_tag
            .unwrap_or_else(|| format!("{product_name}-v{{version}}-rc.{{rc}}"));
        self.template(
            "release.candidate_tag",
            &candidate_tag,
            &[("{version}", 1), ("{rc}", 1)],
        );
        let release_tag = raw
            .release_tag
            .unwrap_or_else(|| format!("{product_name}-v{{version}}"));
        self.template(
            "release.release_tag",
            &release_tag,
            &[("{version}", 1), ("{rc}", 0)],
        );

        let branch_prefix = raw.branch_prefix.unwrap_or_else(|| "release/".to_string());
        if !branch_prefix.ends_with('/')
            || branch_prefix.len() < 2
            || branch_prefix.chars().any(char::is_whitespace)
        {
            self.err(
                "release.branch_prefix",
                format!("\"{branch_prefix}\" must be a branch path ending with `/`"),
            );
        }
        let max_rc_passes = self.positive("release.max_rc_passes", raw.max_rc_passes, 2);
        let later_passes = self.choice(
            "release.later_passes",
            raw.later_passes,
            Some(LaterPasses::Full),
            &[
                ("full", LaterPasses::Full),
                ("gate-and-coordinator", LaterPasses::GateAndCoordinator),
            ],
        );
        let go_policy = self.choice(
            "release.go_policy",
            raw.go_policy,
            Some(GoPolicy::Computed),
            &[("computed", GoPolicy::Computed), ("human", GoPolicy::Human)],
        );
        let nits = self.choice(
            "release.nits",
            raw.nits,
            Some(NitsGrouping::Shared),
            &[
                ("shared", NitsGrouping::Shared),
                ("per-nit", NitsGrouping::PerNit),
            ],
        );
        let record = raw
            .record
            .unwrap_or_else(|| ".spl-asdlc/qa/{candidate}/".to_string());
        self.template("release.record", &record, &[("{candidate}", 1)]);

        let components = self.list("release.components", raw.components, Vec::new(), false);
        for (i, c) in components.iter().enumerate() {
            if !is_angle_key(c) {
                self.err(
                    &format!("release.components[{i}]"),
                    format!("\"{c}\" must be lower-case letters, digits and hyphens"),
                );
            }
        }
        let version_component = self.optional("release.version_component", raw.version_component);
        match (&version_component, components.is_empty()) {
            (Some(v), true) => self.err(
                "release.version_component",
                format!("\"{v}\" is set but release.components is empty"),
            ),
            (Some(v), false) if !components.contains(v) => self.err(
                "release.version_component",
                format!("\"{v}\" is not one of release.components"),
            ),
            (None, false) => self.err(
                "release.version_component",
                "is required for a multi-component product (the component whose release ends the cycle)",
            ),
            _ => {}
        }
        let release_pr_label = self
            .optional("release.release_pr_label", raw.release_pr_label)
            .unwrap_or_else(|| "autorelease: pending".to_string());
        let release_please_authors = self.list(
            "release.release_please_authors",
            raw.release_please_authors,
            Vec::new(),
            true,
        );
        let gate_record = self.optional("release.gate_record", raw.gate_record);
        match &gate_record {
            Some(g) => self.template("release.gate_record", g, &[("{version}", 1), ("{rc}", 0)]),
            None if !components.is_empty() => self.err(
                "release.gate_record",
                "is required for a multi-component product: every component's deployment reads it",
            ),
            None => {}
        }
        let bookkeeping_paths = self.list(
            "release.bookkeeping_paths",
            raw.bookkeeping_paths,
            Vec::new(),
            false,
        );
        let registry_path = self
            .optional("release.registry_path", raw.registry_path)
            .unwrap_or_else(|| "docs/business-rules.md".to_string());
        let business_rule_paths = self.list(
            "release.business_rule_paths",
            raw.business_rule_paths,
            vec![
                "docs/business-rules.md".to_string(),
                "**/business-rules.md".to_string(),
            ],
            false,
        );
        let stagnation_hours = self.positive("release.stagnation_hours", raw.stagnation_hours, 2);
        let coordinators = self.list("release.coordinators", raw.coordinators, Vec::new(), true);
        let escalation_label = self
            .optional("release.escalation_label", raw.escalation_label)
            .unwrap_or_else(|| "blocked-by-human".to_string());
        let auto_candidate = raw.auto_candidate.unwrap_or(false);
        let cut_workflow =
            self.workflow("release.cut_workflow", raw.cut_workflow, "release-cut.yml");
        let gate_workflow = self.workflow(
            "release.gate_workflow",
            raw.gate_workflow,
            "release-gate.yml",
        );
        let gate_rerun_workflow = self.workflow(
            "release.gate_rerun_workflow",
            raw.gate_rerun_workflow,
            "release-gate-auto-rerun.yml",
        );
        let gate_rerun_wait_minutes = self.positive(
            "release.gate_rerun_wait_minutes",
            raw.gate_rerun_wait_minutes,
            30,
        );
        let release_please_workflow = self.workflow(
            "release.release_please_workflow",
            raw.release_please_workflow,
            "release-please.yml",
        );
        let release_please_config = self
            .optional("release.release_please_config", raw.release_please_config)
            .unwrap_or_else(|| "release-please-config.json".to_string());
        let release_please_manifest = self
            .optional(
                "release.release_please_manifest",
                raw.release_please_manifest,
            )
            .unwrap_or_else(|| ".release-please-manifest.json".to_string());
        let angles = self.angles(
            "release.angles",
            raw.angles
                .into_iter()
                .map(|a| (a.key, a.title, a.assignee, a.notes))
                .collect(),
        );
        Some(Release {
            candidate_tag,
            release_tag,
            branch_prefix,
            max_rc_passes,
            later_passes: later_passes?,
            go_policy: go_policy?,
            nits: nits?,
            record,
            components,
            version_component,
            release_pr_label,
            release_please_authors,
            gate_record,
            bookkeeping_paths,
            registry_path,
            business_rule_paths,
            stagnation_hours,
            coordinators,
            escalation_label,
            auto_candidate,
            cut_workflow,
            gate_workflow,
            gate_rerun_workflow,
            gate_rerun_wait_minutes,
            release_please_workflow,
            release_please_config,
            release_please_manifest,
            angles,
        })
    }

    fn review(&mut self, raw: Option<raw::RawReview>) -> Option<Review> {
        let Some(raw) = raw else {
            self.err("review", "the [review] table is required");
            return None;
        };
        let merge_method = self.choice(
            "review.merge_method",
            raw.merge_method,
            Some(MergeMethod::Squash),
            &[
                ("squash", MergeMethod::Squash),
                ("merge", MergeMethod::Merge),
            ],
        );
        let require_up_to_date = raw.require_up_to_date.unwrap_or(true);
        let diff_size_extra_angle = self.non_negative(
            "review.diff_size_extra_angle",
            raw.diff_size_extra_angle,
            1500,
        );
        let diff_size_angle = self.optional("review.diff_size_angle", raw.diff_size_angle);
        if diff_size_extra_angle > 0 && diff_size_angle.is_none() {
            self.err(
                "review.diff_size_angle",
                format!(
                    "is required while review.diff_size_extra_angle is {diff_size_extra_angle} (set it to 0 to add no angle by size)"
                ),
            );
        }
        let human_approval = self.choice(
            "review.human_approval",
            raw.human_approval,
            Some(HumanApproval::Never),
            &[
                ("never", HumanApproval::Never),
                ("sensitive-paths", HumanApproval::SensitivePaths),
                ("always", HumanApproval::Always),
            ],
        );
        let sensitive_paths = self.optional("review.sensitive_paths", raw.sensitive_paths);
        if human_approval == Some(HumanApproval::SensitivePaths) && sensitive_paths.is_none() {
            self.err(
                "review.sensitive_paths",
                "is required when review.human_approval = \"sensitive-paths\"",
            );
        }
        let sensitive_angle = self
            .optional("review.sensitive_angle", raw.sensitive_angle)
            .unwrap_or_else(|| "security".to_string());
        for (key, angle) in [
            ("review.sensitive_angle", Some(&sensitive_angle)),
            ("review.diff_size_angle", diff_size_angle.as_ref()),
        ] {
            if let Some(a) = angle {
                if !is_angle_key(a) {
                    self.err(
                        key,
                        format!("\"{a}\" must be lower-case letters, digits and hyphens"),
                    );
                }
            }
        }
        let coordinators = self.list("review.coordinators", raw.coordinators, Vec::new(), true);
        let human_reviewer = self.optional("review.human_reviewer", raw.human_reviewer);
        let ticketless_authors = self.list(
            "review.ticketless_authors",
            raw.ticketless_authors,
            Vec::new(),
            false,
        );
        let angles = self.angles(
            "review.angles",
            raw.angles
                .into_iter()
                .map(|a| (a.key, None, a.assignee, None))
                .collect(),
        );
        let mut path_angles = Vec::new();
        for (i, pa) in raw.path_angles.into_iter().enumerate() {
            let at = format!("review.path_angles[{i}]");
            let paths = self.list(&format!("{at}.paths"), pa.paths, Vec::new(), true);
            let angle = self.required(&format!("{at}.angle"), pa.angle);
            if let Some(a) = &angle {
                if !is_angle_key(a) {
                    self.err(
                        &format!("{at}.angle"),
                        format!("\"{a}\" must be lower-case letters, digits and hyphens"),
                    );
                }
            }
            if let Some(angle) = angle {
                path_angles.push(PathAngle { paths, angle });
            }
        }
        Some(Review {
            merge_method: merge_method?,
            require_up_to_date,
            diff_size_extra_angle,
            diff_size_angle,
            human_approval: human_approval?,
            sensitive_paths,
            sensitive_angle,
            coordinators,
            human_reviewer,
            ticketless_authors,
            angles,
            path_angles,
        })
    }

    fn bridge(&mut self, raw: Option<raw::RawBridge>) -> Bridge {
        let raw = raw.unwrap_or(raw::RawBridge {
            poll_minutes: None,
            github_identity: None,
            sweep_hours: None,
        });
        let poll_minutes = self.positive("bridge.poll_minutes", raw.poll_minutes, 5);
        let github_identity = self
            .choice(
                "bridge.github_identity",
                raw.github_identity,
                Some(GithubIdentity::App),
                &[
                    ("app", GithubIdentity::App),
                    ("token", GithubIdentity::Token),
                ],
            )
            .unwrap_or(GithubIdentity::App);
        let sweep_hours = self.positive("bridge.sweep_hours", raw.sweep_hours, 24);
        Bridge {
            poll_minutes,
            github_identity,
            sweep_hours,
        }
    }

    fn notify(&mut self, raw: Vec<raw::RawNotify>) -> Vec<Notify> {
        let mut out = Vec::new();
        for (i, n) in raw.into_iter().enumerate() {
            let at = format!("notify[{i}]");
            let kind = self.choice(
                &format!("{at}.kind"),
                n.kind,
                None,
                &[
                    ("telegram", NotifyKind::Telegram),
                    ("email", NotifyKind::Email),
                    ("google-chat", NotifyKind::GoogleChat),
                ],
            );
            if let Some(k) = kind {
                if k != NotifyKind::Telegram {
                    self.err(
                        &format!("{at}.kind"),
                        "this build ships the \"telegram\" channel only",
                    );
                }
            }
            let target = self
                .required(&format!("{at}.target"), n.target)
                .and_then(|t| self.secret(&format!("{at}.target"), &t));
            let token = match n.token {
                Some(t) => self.secret(&format!("{at}.token"), &t),
                None => {
                    if kind == Some(NotifyKind::Telegram) {
                        self.err(
                            &format!("{at}.token"),
                            "is required for telegram (a reference to the bot token)",
                        );
                    }
                    None
                }
            };
            let events = self.list(&format!("{at}.events"), n.events, Vec::new(), true);
            for (j, e) in events.iter().enumerate() {
                if !NOTIFY_EVENTS.contains(&e.as_str()) {
                    self.err(
                        &format!("{at}.events[{j}]"),
                        format!("\"{e}\" is not one of {}", NOTIFY_EVENTS.join(", ")),
                    );
                }
            }
            if let (Some(kind), Some(target)) = (kind, target) {
                out.push(Notify {
                    kind,
                    target,
                    token,
                    events,
                });
            }
        }
        out
    }

    fn workflow(&mut self, key: &str, value: Option<String>, default: &str) -> String {
        let w = self
            .optional(key, value)
            .unwrap_or_else(|| default.to_string());
        if !(w.ends_with(".yml") || w.ends_with(".yaml")) || w.contains('/') {
            self.err(
                key,
                format!("\"{w}\" must be a workflow file name (`<name>.yml`)"),
            );
        }
        w
    }

    /// Each placeholder must appear exactly the given number of times, and
    /// no other `{…}` may appear.
    fn template(&mut self, key: &str, value: &str, placeholders: &[(&str, usize)]) {
        if value.trim().is_empty() {
            self.err(key, "must not be empty");
            return;
        }
        for (p, n) in placeholders {
            let found = value.matches(p).count();
            if found != *n {
                let want = match n {
                    0 => format!("must not contain {p}"),
                    1 => format!("must contain {p} exactly once"),
                    n => format!("must contain {p} exactly {n} times"),
                };
                self.err(key, format!("\"{value}\" {want}"));
            }
        }
        let mut rest = value.to_string();
        for (p, _) in placeholders {
            rest = rest.replace(p, "");
        }
        if rest.contains('{') || rest.contains('}') {
            let allowed = placeholders
                .iter()
                .filter(|(_, n)| *n > 0)
                .map(|(p, _)| *p)
                .collect::<Vec<_>>()
                .join(", ");
            self.err(
                key,
                format!("\"{value}\" carries an unknown placeholder (allowed: {allowed})"),
            );
        }
    }
}

fn is_product_name(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase() || c.is_ascii_digit())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn is_angle_key(s: &str) -> bool {
    is_product_name(s)
}

fn is_repository(s: &str) -> bool {
    let ok = |p: &str| {
        !p.is_empty()
            && p != "."
            && p != ".."
            && p.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    match s.split_once('/') {
        Some((owner, repo)) => ok(owner) && ok(repo),
        None => false,
    }
}

fn is_env_name(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_uppercase() || c == '_')
        && chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

fn default_title(key: &str) -> String {
    let words: Vec<String> = key
        .split('-')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_ascii_uppercase().to_string() + c.as_str(),
                None => String::new(),
            }
        })
        .collect();
    format!("{} review", words.join(" "))
}

/// The example of `docs/spec/08-configuration.md`, as the tests read it:
/// the repository's file and its instance overlay.
#[cfg(test)]
pub(crate) mod spec_example {
    use super::{Config, FILE_NAME, INSTANCE_FILE_NAME, Source};

    const SPEC: &str = include_str!("../../docs/spec/08-configuration.md");

    /// The `n`-th ```toml block of spec 08, from 0.
    fn block(n: usize) -> &'static str {
        let open = "```toml\n";
        let mut start = 0;
        for _ in 0..=n {
            start += SPEC[start..]
                .find(open)
                .expect("spec 08 carries the toml blocks of its example")
                + open.len();
        }
        let end = start + SPEC[start..].find("```").expect("the toml block is closed");
        &SPEC[start..end]
    }

    /// The repository's file of the example.
    pub(crate) fn repository() -> &'static str {
        block(0)
    }

    /// The instance overlay of the example.
    pub(crate) fn instance() -> &'static str {
        block(1)
    }

    pub(crate) fn source(label: &str, text: &str) -> Source {
        Source {
            label: label.to_string(),
            text: text.to_string(),
        }
    }

    /// A repository's file (the example's, edited) merged with the
    /// example's overlay.
    pub(crate) fn parse(repository: &str) -> Config {
        Config::parse_with(
            &source(FILE_NAME, repository),
            Some(&source(INSTANCE_FILE_NAME, instance())),
        )
        .unwrap_or_else(|e| panic!("spec 08 example: {e}"))
    }

    /// The example, merged.
    pub(crate) fn config() -> Config {
        parse(repository())
    }
}

#[cfg(test)]
mod tests;
