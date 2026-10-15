//! The instance overlay (`docs/spec/08-configuration.md`, "The instance
//! overlay"): the identities of one installation, kept on the host that runs
//! the bridge, outside the repository, and merged at load time with the
//! repository's `.spl-asdlc/config.toml`, which then carries the method's shape only.
//!
//! A key set in both files is an error, never an override (BR-ASDLC-014): a
//! silent override would let a file outside review hide a reviewed setting.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::path::{Path, PathBuf};

use toml::de::{DeTable, DeValue};
use toml::{Table, Value};

use super::raw::{RawNotify, RawOverlay, RawOverlayAngle};
use super::{ConfigError, ConfigErrors, SCHEMA_VERSION, Source};

/// The overlay's file name, beside the configuration it completes.
pub const INSTANCE_FILE_NAME: &str = "config.instance.toml";

/// The instance keys, by table. Besides these, an overlay carries `schema`,
/// the `assignee` of an angle the repository's file declares (an entry of
/// `[[release.angles]]` or `[[review.angles]]` with `key` and `assignee`
/// only) and the `[[notify]]` tables. Nothing else.
pub const INSTANCE_KEYS: [(&str, &[&str]); 5] = [
    ("product", &["repository"]),
    ("owner", &["tracker_user", "github_login"]),
    ("tracker", &["team", "service_identity", "team_keys"]),
    (
        "release",
        &["coordinators", "coordinator_assignee", "fix_assignee"],
    ),
    (
        "review",
        &["coordinators", "coordinator_assignee", "human_reviewer"],
    ),
];

/// The tables whose angles may receive an assignee from the overlay.
const ANGLE_TABLES: [&str; 2] = ["release", "review"];

/// Whether a key (as the validator names it) is an instance key.
pub(super) fn is_instance_key(key: &str) -> bool {
    key == "owner"
        || key == "notify"
        || INSTANCE_KEYS.iter().any(|(table, keys)| {
            key.strip_prefix(table)
                .and_then(|k| k.strip_prefix('.'))
                .is_some_and(|k| keys.contains(&k))
        })
}

/// The sibling overlay of `file`: `<stem>.instance.toml` beside it, so
/// `config.instance.toml` beside the default `config.toml`, and two
/// configurations kept in one directory never share an overlay.
pub fn sibling_of(file: &Path) -> PathBuf {
    let dir = file.parent().unwrap_or_else(|| Path::new(""));
    match file.file_stem().and_then(|s| s.to_str()) {
        Some(stem) if !stem.is_empty() => dir.join(format!("{stem}.instance.toml")),
        _ => dir.join(INSTANCE_FILE_NAME),
    }
}

/// The overlay of `file`: the one named (`--instance`), which must exist,
/// or else its sibling ([`sibling_of`]) when there is one. Nothing at the
/// sibling's path means no overlay; something there that is not a file (a
/// directory, a dangling link, an unreadable entry) is an error, never "no
/// overlay".
pub fn instance_path(file: &Path, named: Option<&Path>) -> Result<Option<PathBuf>, ConfigErrors> {
    let refuse = |p: &Path, why: &str| {
        Err(ConfigErrors(vec![ConfigError {
            file: None,
            key: String::new(),
            message: format!("{} could not be read: {why}", p.display()),
        }]))
    };
    if let Some(p) = named {
        if p.is_file() {
            return Ok(Some(p.to_path_buf()));
        }
        return refuse(p, "the instance overlay named does not exist");
    }
    let sibling = sibling_of(file);
    match std::fs::symlink_metadata(&sibling) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => refuse(&sibling, &e.to_string()),
        Ok(_) if sibling.is_file() => Ok(Some(sibling)),
        Ok(_) => refuse(&sibling, "the instance overlay is not a regular file"),
    }
}

/// What the merge did: the keys the overlay set (as the validator names
/// them, in the merged file) and what it refused.
pub(super) struct Merged {
    pub from_overlay: Vec<String>,
    pub errors: Vec<ConfigError>,
}

impl Merged {
    /// Whether an error on `key` is about a value the overlay gave.
    pub fn is_from_overlay(&self, key: &str) -> bool {
        self.from_overlay.iter().any(|p| {
            key.strip_prefix(p.as_str())
                .is_some_and(|rest| rest.is_empty() || rest.starts_with(['.', '[']))
        })
    }
}

/// Merge the overlay into the repository's file, read as a table. Refused
/// outright when the overlay is not valid TOML, carries a key that is not an
/// instance key, or a value of the wrong type; otherwise every conflict is
/// listed in [`Merged::errors`].
pub(super) fn merge(
    repository: &mut Table,
    repository_label: &str,
    overlay: &Source,
) -> Result<Merged, ConfigErrors> {
    let invalid = |e: &dyn std::fmt::Display| {
        ConfigErrors(vec![ConfigError {
            file: Some(overlay.label.clone()),
            key: String::new(),
            message: format!("not a valid instance overlay: {}", e.to_string().trim_end()),
        }])
    };
    let lines = instance_keys_only(overlay)?;
    let raw: RawOverlay = toml::from_str(&overlay.text).map_err(|e| invalid(&e))?;
    let mut m = Merge {
        repository,
        repository_label,
        overlay_label: &overlay.label,
        lines: &lines,
        from_overlay: Vec::new(),
        errors: Vec::new(),
    };
    match raw.schema {
        Some(SCHEMA_VERSION) => {}
        Some(n) => m.refuse(
            "schema",
            format!("version {n} is not read by this build (expected {SCHEMA_VERSION})"),
        ),
        None => m.refuse("schema", format!("is required (schema = {SCHEMA_VERSION})")),
    }
    if let Some(p) = raw.product {
        m.put("product", "repository", p.repository.map(Value::from));
    }
    if let Some(o) = raw.owner {
        m.put("owner", "tracker_user", o.tracker_user.map(Value::from));
        m.put("owner", "github_login", o.github_login.map(Value::from));
    }
    if let Some(t) = raw.tracker {
        m.put("tracker", "team", t.team.map(Value::from));
        m.put(
            "tracker",
            "service_identity",
            t.service_identity.map(Value::from),
        );
        m.put("tracker", "team_keys", t.team_keys.map(Value::from));
    }
    // The overlay never creates the `[release]` section, which every
    // repository's file carries: without it, the file is refused for it,
    // and the overlay's release keys complete nothing (spec 08).
    let has_release = m.repository.contains_key("release");
    if let Some(r) = raw.release.filter(|_| has_release) {
        m.put("release", "coordinators", r.coordinators.map(Value::from));
        m.put(
            "release",
            "coordinator_assignee",
            r.coordinator_assignee.map(Value::from),
        );
        m.put("release", "fix_assignee", r.fix_assignee.map(Value::from));
        m.angles("release", r.angles);
    }
    if let Some(r) = raw.review {
        m.put("review", "coordinators", r.coordinators.map(Value::from));
        m.put(
            "review",
            "coordinator_assignee",
            r.coordinator_assignee.map(Value::from),
        );
        m.put(
            "review",
            "human_reviewer",
            r.human_reviewer.map(Value::from),
        );
        m.angles("review", r.angles);
    }
    m.notify(raw.notify);
    Ok(Merged {
        from_overlay: m.from_overlay,
        errors: m.errors,
    })
}

/// Every key of the overlay is an instance key, or the overlay is refused
/// with the line of each one that is not. Returns the line of each key.
fn instance_keys_only(overlay: &Source) -> Result<BTreeMap<String, usize>, ConfigErrors> {
    let text = overlay.text.as_str();
    let doc = DeTable::parse(text).map_err(|e| {
        ConfigErrors(vec![ConfigError {
            file: Some(overlay.label.clone()),
            key: String::new(),
            message: format!("not a valid instance overlay: {}", e.to_string().trim_end()),
        }])
    })?;
    let line = |span: Range<usize>| text[..span.start.min(text.len())].matches('\n').count() + 1;
    let mut lines = BTreeMap::new();
    let mut foreign: Vec<(usize, String)> = Vec::new();
    for (k, v) in doc.get_ref() {
        let name: &str = k.get_ref();
        let Some((table, keys)) = INSTANCE_KEYS.iter().find(|(t, _)| *t == name) else {
            if matches!(name, "schema" | "notify") {
                lines.insert(name.to_string(), line(k.span()));
            } else {
                foreign.push((line(k.span()), name.to_string()));
            }
            continue;
        };
        lines.insert(name.to_string(), line(k.span()));
        // A table written as another type is refused by its type, below.
        let DeValue::Table(inner) = v.get_ref() else {
            continue;
        };
        for (k2, v2) in inner {
            let key: &str = k2.get_ref();
            let path = format!("{table}.{key}");
            if key == "angles" && ANGLE_TABLES.contains(table) {
                let DeValue::Array(entries) = v2.get_ref() else {
                    continue;
                };
                for (i, e) in entries.iter().enumerate() {
                    let DeValue::Table(entry) = e.get_ref() else {
                        continue;
                    };
                    for (k3, _) in entry {
                        let field: &str = k3.get_ref();
                        let at = format!("{path}[{i}].{field}");
                        if matches!(field, "key" | "assignee") {
                            lines.insert(at, line(k3.span()));
                        } else {
                            foreign.push((line(k3.span()), at));
                        }
                    }
                }
            } else if keys.contains(&key) {
                lines.insert(path, line(k2.span()));
            } else {
                foreign.push((line(k2.span()), path));
            }
        }
    }
    if foreign.is_empty() {
        return Ok(lines);
    }
    foreign.sort();
    Err(ConfigErrors(
        foreign
            .into_iter()
            .map(|(at, key)| ConfigError {
                file: Some(overlay.label.clone()),
                key,
                message: format!(
                    "line {at}: not an instance key; it belongs in the repository's configuration, where it is reviewed"
                ),
            })
            .collect(),
    ))
}

struct Merge<'a> {
    repository: &'a mut Table,
    repository_label: &'a str,
    overlay_label: &'a str,
    lines: &'a BTreeMap<String, usize>,
    from_overlay: Vec<String>,
    errors: Vec<ConfigError>,
}

impl Merge<'_> {
    fn at_line(&self, overlay_key: &str) -> String {
        self.lines
            .get(overlay_key)
            .map(|l| format!(" (line {l})"))
            .unwrap_or_default()
    }

    /// An error in the overlay alone.
    fn refuse(&mut self, overlay_key: &str, message: impl Into<String>) {
        let message = format!("{}{}", message.into(), self.at_line(overlay_key));
        self.errors.push(ConfigError {
            file: Some(self.overlay_label.to_string()),
            key: overlay_key.to_string(),
            message,
        });
    }

    /// A key both files set: refused, whatever the two values are.
    fn both(&mut self, key: &str, overlay_key: &str) {
        let message = format!(
            "is set in both {} and {}{}: a setting is defined in exactly one place, and the overlay never overrides the repository's file",
            self.repository_label,
            self.overlay_label,
            self.at_line(overlay_key)
        );
        self.errors.push(ConfigError {
            file: None,
            key: key.to_string(),
            message,
        });
    }

    fn table(&mut self, name: &str) -> Option<&mut Table> {
        self.repository
            .entry(name)
            .or_insert_with(|| Value::Table(Table::new()))
            .as_table_mut()
    }

    fn put(&mut self, table: &str, key: &str, value: Option<Value>) {
        let Some(value) = value else {
            return;
        };
        let path = format!("{table}.{key}");
        // The repository's file was read with its shape: `table` is a table.
        let Some(t) = self.table(table) else {
            return;
        };
        if t.contains_key(key) {
            self.both(&path, &path);
            return;
        }
        t.insert(key.to_string(), value);
        self.from_overlay.push(path);
    }

    fn angles(&mut self, table: &str, entries: Vec<RawOverlayAngle>) {
        let mut seen = BTreeSet::new();
        for (i, a) in entries.into_iter().enumerate() {
            let at = format!("{table}.angles[{i}]");
            let key = a
                .key
                .map(|k| k.trim().to_string())
                .filter(|k| !k.is_empty());
            let Some(key) = key else {
                self.refuse(
                    &format!("{at}.key"),
                    "is required: it names an angle the repository's file declares",
                );
                continue;
            };
            let assignee = a.assignee.filter(|s| !s.trim().is_empty());
            let Some(assignee) = assignee else {
                self.refuse(
                    &format!("{at}.assignee"),
                    "is required: an angle entry of the overlay gives the assignee of an angle, and nothing else",
                );
                continue;
            };
            if !seen.insert(key.clone()) {
                self.refuse(&format!("{at}.key"), format!("\"{key}\" is given twice"));
                continue;
            }
            let declared = self
                .table(table)
                .and_then(|t| t.get_mut("angles"))
                .and_then(Value::as_array_mut)
                .and_then(|list| {
                    list.iter_mut().enumerate().find(|(_, e)| {
                        e.get("key").and_then(Value::as_str).map(str::trim) == Some(key.as_str())
                    })
                })
                .and_then(|(j, e)| Some((j, e.as_table_mut()?)));
            let Some((j, entry)) = declared else {
                self.refuse(
                    &format!("{at}.key"),
                    format!(
                        "\"{key}\" is not an angle of {}: the overlay gives an angle its assignee, it never declares one",
                        self.repository_label
                    ),
                );
                continue;
            };
            let merged = format!("{table}.angles[{j}].assignee");
            if entry.contains_key("assignee") {
                self.both(&merged, &format!("{at}.assignee"));
                continue;
            }
            entry.insert("assignee".to_string(), Value::from(assignee));
            self.from_overlay.push(merged);
        }
    }

    fn notify(&mut self, channels: Vec<RawNotify>) {
        if channels.is_empty() {
            return;
        }
        if self.repository.contains_key("notify") {
            self.both("notify", "notify");
            return;
        }
        let tables = channels
            .into_iter()
            .map(|n| {
                let mut t = Table::new();
                for (k, v) in [
                    ("kind", n.kind.map(Value::from)),
                    ("target", n.target.map(Value::from)),
                    ("token", n.token.map(Value::from)),
                    ("events", n.events.map(Value::from)),
                ] {
                    if let Some(v) = v {
                        t.insert(k.to_string(), v);
                    }
                }
                Value::Table(t)
            })
            .collect::<Vec<_>>();
        self.repository
            .insert("notify".to_string(), Value::Array(tables));
        self.from_overlay.push("notify".to_string());
    }
}
