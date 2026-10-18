//! `.spl-asdlc/config.toml`: the per-repository configuration
//! (`docs/reference/08-configuration.md`).
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

use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;

use raw::RawConfig;

pub use overlay::{INSTANCE_FILE_NAME, instance_path, sibling_of};

/// The only schema version this build reads.
pub const SCHEMA_VERSION: i64 = 1;

/// The method's folder, at the root of the repository.
pub const DIR: &str = ".spl-asdlc";

/// The configuration's file name, in [`DIR`].
pub const FILE_NAME: &str = "config.toml";

/// The configuration's default path, from the root of the repository: the
/// default of every `--file`, and where `doctor` reads it at the default
/// branch.
pub const DEFAULT_PATH: &str = ".spl-asdlc/config.toml";

/// The default of the `[release]` key the PR pass reads too (spec 08,
/// `[release]`).
pub const DEFAULT_BRANCH_PREFIX: &str = "release/";

/// The CHANGELOG sections `release prepare` writes when the file names none
/// (`release.changelog_sections`): one per type `asdlc-pr-title.yml` allows
/// by default, in this order. A type no section names is left out of the
/// notes; a breaking change is always listed, under its own section first.
pub const DEFAULT_CHANGELOG_SECTIONS: [(&str, &str); 11] = [
    ("feat", "Features"),
    ("fix", "Bug Fixes"),
    ("perf", "Performance"),
    ("revert", "Reverts"),
    ("docs", "Documentation"),
    ("style", "Style"),
    ("chore", "Chores"),
    ("refactor", "Refactoring"),
    ("test", "Tests"),
    ("build", "Build"),
    ("ci", "Continuous Integration"),
];

/// Canonical statuses of a work item (`docs/reference/03-vocabulary.md`).
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
    /// The release cycle (`docs/reference/05-release-cycle.md`): every project
    /// has one, so a file without a `[release]` section is refused.
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

/// The human of the method (`docs/reference/00-overview.md`): takes escalations
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
    /// The tracker project (its name or id) every item the bridge creates
    /// lands in; `None` creates them in the team alone.
    pub project: Option<String>,
    pub link_fallback: LinkFallback,
    /// The label of a ticket with nothing to deploy (documentation, say):
    /// it goes Done at the merge of its PR rather than To Deploy, and no
    /// release embarks it (spec 04 §10). The bridge reads it, never writes it.
    pub no_deploy_label: String,
}

/// The default of `tracker.no_deploy_label`.
pub const DEFAULT_NO_DEPLOY_LABEL: &str = "no-deploy";

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Angle {
    pub key: String,
    pub title: String,
    pub assignee: Option<String>,
    pub notes: Option<String>,
    /// A review angle's paths (globs, `*` crossing `/`): it applies only to a
    /// PR that touches one. Empty for an angle on every PR, and always for a
    /// release angle, which reviews the whole candidate.
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub candidate_tag: String,
    pub release_tag: String,
    pub branch_prefix: String,
    pub max_rc_passes: u32,
    pub later_passes: LaterPasses,
    pub go_policy: GoPolicy,
    pub record: String,
    pub bookkeeping_paths: Vec<String>,
    pub stagnation_hours: u32,
    pub coordinators: Vec<String>,
    /// The tracker participant who receives the consolidation item of a
    /// candidate; `None` leaves it unassigned.
    pub coordinator_assignee: Option<String>,
    /// The tracker participant who fixes release problems: the urgent fix
    /// ticket of a blocking QA finding and the gate-fix ticket of a red gate
    /// are assigned to it; `None` leaves them unassigned, taken by role.
    pub fix_assignee: Option<String>,
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
    /// The project's publication workflow, run on the published release:
    /// the bridge reads its run to retract a release whose publication
    /// failed before anything was deployed (`05-release-cycle.md` §9).
    pub publish_workflow: String,
    /// The files besides Rust's own that `release prepare` writes the
    /// version into.
    pub version_files: Vec<VersionFile>,
    /// The CHANGELOG sections, by Conventional Commit type, in order.
    pub changelog_sections: Vec<ChangelogSection>,
    pub angles: Vec<Angle>,
}

/// A file `release prepare` writes the next version into
/// (`[[release.version_files]]`): every match of `pattern`, whose
/// `{version}` stands for a semantic version, gets the new one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionFile {
    pub path: String,
    pub pattern: String,
}

/// A Conventional Commit type and the CHANGELOG section that lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangelogSection {
    pub kind: String,
    pub section: String,
}

/// The default sections, [`DEFAULT_CHANGELOG_SECTIONS`].
pub fn default_changelog_sections() -> Vec<ChangelogSection> {
    DEFAULT_CHANGELOG_SECTIONS
        .iter()
        .map(|(k, s)| ChangelogSection {
            kind: k.to_string(),
            section: s.to_string(),
        })
        .collect()
}

/// What `release prepare` reads of the repository's file: the shape of the
/// release, never an identity. It runs in the cut workflow, where no
/// instance overlay exists, so the instance keys are not required.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preparation {
    pub product: String,
    /// `product.repository`, when the repository's file carries it.
    pub repository: Option<String>,
    pub release_tag: String,
    pub version_files: Vec<VersionFile>,
    pub changelog_sections: Vec<ChangelogSection>,
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
    Always,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Review {
    pub merge_method: MergeMethod,
    pub require_up_to_date: bool,
    /// Changed lines above which `diff_size_angle` is added; 0 (the
    /// default) disables it.
    pub diff_size_extra_angle: u32,
    pub diff_size_angle: Option<String>,
    pub human_approval: HumanApproval,
    pub coordinators: Vec<String>,
    /// The tracker participant who receives the consolidation item of a PR
    /// round; `None` leaves it unassigned.
    pub coordinator_assignee: Option<String>,
    pub human_reviewer: Option<String>,
    /// Apps whose PRs need no ticket: the bridge opens one for each.
    pub ticketless_authors: Vec<String>,
    /// The review angles, each on every PR or on the PRs that touch its
    /// paths ([`Angle::paths`]).
    pub angles: Vec<Angle>,
}

impl Review {
    /// Whether some PR can get an angle (spec 04 §11): an angle, on every PR
    /// or on its paths, or a size threshold. When none can, the project has
    /// no PR review: a PR merges on its CI and the other merge conditions,
    /// with no round and no approval.
    pub fn angle_possible(&self) -> bool {
        angle_possible(!self.angles.is_empty(), self.diff_size_extra_angle)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GithubIdentity {
    App,
    Token,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bridge {
    /// A hint for the host that runs the bridge: the interval is the host's
    /// choice (Livela's runner, a timer), and the bridge never reads it.
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
    /// The release branch prefix by which the PR pass leaves the release
    /// machinery's PRs alone (spec 04 §8), so that a merge back is never
    /// squashed: `release.branch_prefix`.
    pub fn release_machinery(&self) -> &str {
        &self.release.branch_prefix
    }

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

    /// Validate the text of a configuration that stands alone.
    pub fn parse(text: &str) -> Result<Config, ConfigErrors> {
        let raw: RawConfig = toml::from_str(text).map_err(|e| {
            ConfigErrors(vec![ConfigError {
                file: None,
                key: String::new(),
                message: format!("not a valid configuration: {}", e.to_string().trim_end()),
            }])
        })?;
        let mut v = Validator::default();
        let config = v.config(raw);
        match config {
            Some(c) if v.errors.is_empty() => Ok(c),
            _ => Err(ConfigErrors(v.errors)),
        }
    }

    /// What `release prepare` needs of the repository's file alone (spec
    /// 08): the product's name, the release tag pattern, the version files
    /// and the CHANGELOG sections, each validated as the bridge validates
    /// it. A file that is not valid TOML or carries an unknown key is
    /// refused, as always; the instance keys, which an overlay on the
    /// bridge's host may carry, are not required here.
    pub fn preparation(text: &str) -> Result<Preparation, ConfigErrors> {
        let raw: RawConfig = toml::from_str(text).map_err(|e| {
            ConfigErrors(vec![ConfigError {
                file: None,
                key: String::new(),
                message: format!("not a valid configuration: {}", e.to_string().trim_end()),
            }])
        })?;
        let mut v = Validator::default();
        let product = match raw.product {
            Some(p) => {
                let repository = p.repository;
                v.required("product.name", p.name).map(|n| (n, repository))
            }
            None => {
                v.err("product", "the [product] table is required");
                None
            }
        };
        if let Some((name, _)) = &product {
            if !is_product_name(name) {
                v.err(
                    "product.name",
                    format!("\"{name}\" must be lower-case letters, digits and hyphens"),
                );
            }
        }
        let Some(release) = raw.release else {
            v.err("release", NO_RELEASE);
            return Err(ConfigErrors(v.errors));
        };
        let name = product.as_ref().map_or("product", |(n, _)| n.as_str());
        let release_tag = release
            .release_tag
            .unwrap_or_else(|| format!("{name}-v{{version}}"));
        v.template(
            "release.release_tag",
            &release_tag,
            &[("{version}", 1), ("{rc}", 0)],
        );
        let version_files = v.version_files(release.version_files);
        let changelog_sections = v.changelog_sections(release.changelog_sections);
        match product {
            Some((product, repository)) if v.errors.is_empty() => Ok(Preparation {
                product,
                repository,
                release_tag,
                version_files,
                changelog_sections,
            }),
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
                "not a valid configuration: {}",
                e.to_string().trim_end()
            ))
        };
        toml::from_str::<RawConfig>(&repository.text).map_err(not_valid)?;
        let mut table: toml::Table = toml::from_str(&repository.text).map_err(not_valid)?;
        let mut merged = overlay::merge(&mut table, &repository.label, instance)?;
        let raw: RawConfig = toml::Value::Table(table).try_into().map_err(|e| {
            in_repository(format!(
                "the merge with {} is not a valid configuration: {e}",
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

/// Why a file without `[release]` is refused: every project has the cycle.
const NO_RELEASE: &str = "the [release] section is required: every project has a release cycle (write `angles = []` under it for a release with no QA angle)";

/// `key`, `title`, `assignee`, `notes`, `paths` of an angle entry, as
/// written.
type AngleFields = (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<Vec<String>>,
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
        let release = match raw.release {
            Some(r) => self.release(r, product.as_ref()),
            None => {
                self.err("release", NO_RELEASE);
                None
            }
        };
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
        let project = self.optional("tracker.project", raw.project);
        let link_fallback = self.choice(
            "tracker.link_fallback",
            raw.link_fallback,
            Some(LinkFallback::None),
            &[
                ("none", LinkFallback::None),
                ("body-url", LinkFallback::BodyUrl),
            ],
        );
        let no_deploy_label = self
            .optional("tracker.no_deploy_label", raw.no_deploy_label)
            .unwrap_or_else(|| DEFAULT_NO_DEPLOY_LABEL.to_string());
        Some(Tracker {
            kind: kind?,
            team: team?,
            service_identity: service_identity?,
            team_keys,
            project,
            link_fallback: link_fallback?,
            no_deploy_label,
        })
    }

    /// The angles of a list, each checked; an empty list is refused only
    /// when `at_least_one`.
    fn angles(&mut self, key: &str, raw: Vec<AngleFields>, at_least_one: bool) -> Vec<Angle> {
        if raw.is_empty() && at_least_one {
            self.err(
                key,
                "at least one angle is required: a review with no angle computes its verdict from nobody",
            );
        }
        let mut out = Vec::new();
        let mut seen = BTreeSet::new();
        for (i, (k, title, assignee, notes, paths)) in raw.into_iter().enumerate() {
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
            // Given, `paths` names at least one glob: an angle on every PR
            // leaves the key out.
            let paths = match paths {
                Some(p) if p.is_empty() => {
                    self.err(
                        &format!("{at}.paths"),
                        "must name at least one glob (leave the key out for an angle on every PR)",
                    );
                    Vec::new()
                }
                Some(p) => self.list(&format!("{at}.paths"), Some(p), Vec::new(), false),
                None => Vec::new(),
            };
            out.push(Angle {
                key: k,
                title,
                assignee,
                notes,
                paths,
            });
        }
        out
    }

    fn release(&mut self, raw: raw::RawRelease, product: Option<&Product>) -> Option<Release> {
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

        let branch_prefix = raw
            .branch_prefix
            .unwrap_or_else(|| DEFAULT_BRANCH_PREFIX.to_string());
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
        let record = raw
            .record
            .unwrap_or_else(|| ".spl-asdlc/qa/{candidate}/".to_string());
        self.template("release.record", &record, &[("{candidate}", 1)]);

        // One version per product: a multi-component product, whose
        // components each ship their own version, is not supported by the
        // release cycle yet (spec 08), and fails closed.
        const MULTI: &str = "a multi-component product is not supported by the release cycle yet: it prepares one version per product (`release prepare`); remove the key";
        if raw.components.as_ref().is_some_and(|c| !c.is_empty()) {
            self.err("release.components", MULTI);
        }
        if raw.version_component.is_some() {
            self.err("release.version_component", MULTI);
        }
        if raw.gate_record.is_some() {
            self.err("release.gate_record", MULTI);
        }
        let bookkeeping_paths = self.list(
            "release.bookkeeping_paths",
            raw.bookkeeping_paths,
            Vec::new(),
            false,
        );
        let stagnation_hours = self.positive("release.stagnation_hours", raw.stagnation_hours, 2);
        // `angles = []` written under [release] is a release with no QA
        // angle, whose GO rests on the gate; no angle written at all is a
        // file that forgot them, refused.
        let no_qa_angle = raw.angles.as_ref().is_some_and(Vec::is_empty);
        let angles = match raw.angles {
            None => {
                self.err(
                    "release.angles",
                    "at least one angle is required, or `angles = []` written under [release] for a release with no QA angle (its GO rests on the gate)",
                );
                Vec::new()
            }
            Some(list) if list.is_empty() => Vec::new(),
            Some(list) => self.angles(
                "release.angles",
                list.into_iter()
                    .map(|a| (a.key, a.title, a.assignee, a.notes, None))
                    .collect(),
                true,
            ),
        };
        // The coordinators approve angle PRs: required only when there is
        // an angle to approve.
        let coordinators = self.list(
            "release.coordinators",
            raw.coordinators,
            Vec::new(),
            !no_qa_angle,
        );
        let coordinator_assignee =
            self.optional("release.coordinator_assignee", raw.coordinator_assignee);
        let fix_assignee = self.optional("release.fix_assignee", raw.fix_assignee);
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
        let publish_workflow = self.workflow(
            "release.publish_workflow",
            raw.publish_workflow,
            "release-publish.yml",
        );
        let gate_rerun_wait_minutes = self.positive(
            "release.gate_rerun_wait_minutes",
            raw.gate_rerun_wait_minutes,
            30,
        );
        let version_files = self.version_files(raw.version_files);
        let changelog_sections = self.changelog_sections(raw.changelog_sections);
        Some(Release {
            candidate_tag,
            release_tag,
            branch_prefix,
            max_rc_passes,
            later_passes: later_passes?,
            go_policy: go_policy?,
            record,
            bookkeeping_paths,
            stagnation_hours,
            coordinators,
            coordinator_assignee,
            fix_assignee,
            escalation_label,
            auto_candidate,
            cut_workflow,
            gate_workflow,
            gate_rerun_workflow,
            gate_rerun_wait_minutes,
            publish_workflow,
            version_files,
            changelog_sections,
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
        // Off by default: a project that wants an angle by size sets the
        // threshold and the angle it adds.
        let diff_size_extra_angle =
            self.non_negative("review.diff_size_extra_angle", raw.diff_size_extra_angle, 0);
        let diff_size_angle = self.optional("review.diff_size_angle", raw.diff_size_angle);
        if diff_size_extra_angle > 0 && diff_size_angle.is_none() {
            self.err(
                "review.diff_size_angle",
                format!(
                    "is required while review.diff_size_extra_angle is {diff_size_extra_angle} (leave the threshold out, or 0, to add no angle by size)"
                ),
            );
        }
        let human_approval = self.choice(
            "review.human_approval",
            raw.human_approval,
            Some(HumanApproval::Never),
            &[
                ("never", HumanApproval::Never),
                ("always", HumanApproval::Always),
            ],
        );
        if let Some(a) = &diff_size_angle {
            if !is_angle_key(a) {
                self.err(
                    "review.diff_size_angle",
                    format!("\"{a}\" must be lower-case letters, digits and hyphens"),
                );
            }
        }
        let coordinator_assignee =
            self.optional("review.coordinator_assignee", raw.coordinator_assignee);
        let human_reviewer = self.optional("review.human_reviewer", raw.human_reviewer);
        let ticketless_authors = self.list(
            "review.ticketless_authors",
            raw.ticketless_authors,
            Vec::new(),
            false,
        );
        // No PR review is an explicit choice (`angles = []`): a file that
        // says nothing about its angles is refused, never read as none.
        let angles = match raw.angles {
            None => {
                self.err(
                    "review.angles",
                    "is required: one `[[review.angles]]` table per angle, or `angles = []` under [review] for no PR review",
                );
                Vec::new()
            }
            Some(list) => self.angles(
                "review.angles",
                list.into_iter()
                    .map(|a| (a.key, None, a.assignee, a.notes, a.paths))
                    .collect(),
                false,
            ),
        };
        // The coordinator approves a round; a project where no PR can get an
        // angle has no round, so no coordinator to name.
        let coordinators = self.list("review.coordinators", raw.coordinators, Vec::new(), false);
        let angle_possible = angle_possible(!angles.is_empty(), diff_size_extra_angle);
        if angle_possible && coordinators.is_empty() {
            self.err(
                "review.coordinators",
                "is required and must name at least one entry while a PR can get an angle (a review angle or a size threshold): the coordinator approves its round",
            );
        }
        Some(Review {
            merge_method: merge_method?,
            require_up_to_date,
            diff_size_extra_angle,
            diff_size_angle,
            human_approval: human_approval?,
            coordinators,
            coordinator_assignee,
            human_reviewer,
            ticketless_authors,
            angles,
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

    /// `[[release.version_files]]`: a relative path and a pattern holding
    /// `{version}` once, each pair listed once.
    fn version_files(&mut self, raw: Option<Vec<raw::RawVersionFile>>) -> Vec<VersionFile> {
        let mut out: Vec<VersionFile> = Vec::new();
        for (i, f) in raw.unwrap_or_default().into_iter().enumerate() {
            let key = format!("release.version_files[{i}]");
            let path = self.required(&format!("{key}.path"), f.path);
            let pattern = self.required(&format!("{key}.pattern"), f.pattern);
            if let Some(p) = &path {
                if p.starts_with('/')
                    || p.contains('\\')
                    || p.split('/').any(|s| s.is_empty() || s == "." || s == "..")
                {
                    self.err(
                        &format!("{key}.path"),
                        format!("\"{p}\" must be a path inside the repository (`dir/file`)"),
                    );
                }
            }
            if let Some(p) = &pattern {
                self.template(&format!("{key}.pattern"), p, &[("{version}", 1)]);
            }
            if let (Some(path), Some(pattern)) = (path, pattern) {
                let entry = VersionFile { path, pattern };
                if out.contains(&entry) {
                    self.err(&key, "is listed twice");
                } else {
                    out.push(entry);
                }
            }
        }
        out
    }

    /// `[[release.changelog_sections]]`: a lower-case type, each listed
    /// once, and a section title; absent, the defaults.
    fn changelog_sections(
        &mut self,
        raw: Option<Vec<raw::RawChangelogSection>>,
    ) -> Vec<ChangelogSection> {
        let Some(raw) = raw else {
            return default_changelog_sections();
        };
        if raw.is_empty() {
            self.err(
                "release.changelog_sections",
                "must name at least one type (leave the key out for the defaults)",
            );
        }
        let mut out: Vec<ChangelogSection> = Vec::new();
        for (i, s) in raw.into_iter().enumerate() {
            let key = format!("release.changelog_sections[{i}]");
            let kind = self.required(&format!("{key}.type"), s.kind);
            let section = self.required(&format!("{key}.section"), s.section);
            if let Some(k) = &kind {
                if k.is_empty() || !k.chars().all(|c| c.is_ascii_lowercase()) {
                    self.err(
                        &format!("{key}.type"),
                        format!("\"{k}\" must be a Conventional Commit type (lower-case letters)"),
                    );
                } else if out.iter().any(|o| &o.kind == k) {
                    self.err(&format!("{key}.type"), format!("\"{k}\" is listed twice"));
                }
            }
            if let (Some(kind), Some(section)) = (kind, section) {
                out.push(ChangelogSection { kind, section });
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

/// Whether some PR can get an angle: a review angle (on every PR or on its
/// paths) or a size threshold. Without one, no PR is reviewed.
fn angle_possible(angles: bool, diff_size_extra_angle: u32) -> bool {
    angles || diff_size_extra_angle > 0
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

/// The example of `docs/reference/08-configuration.md`, as the tests read it:
/// the repository's file and its instance overlay.
#[cfg(test)]
pub(crate) mod spec_example {
    use super::{Config, FILE_NAME, INSTANCE_FILE_NAME, Source};

    const SPEC: &str = include_str!("../../docs/reference/08-configuration.md");

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
