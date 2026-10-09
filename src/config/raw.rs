//! The file as written: every key optional, unknown keys refused.
//!
//! Validation and defaults live in `super`; this module only describes the
//! shape TOML may take, so that a misspelt key is an error rather than a
//! silently ignored setting.

use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawConfig {
    pub schema: Option<i64>,
    pub product: Option<RawProduct>,
    pub owner: Option<RawOwner>,
    pub tracker: Option<RawTracker>,
    pub release: Option<RawRelease>,
    pub review: Option<RawReview>,
    pub bridge: Option<RawBridge>,
    #[serde(default)]
    pub notify: Vec<RawNotify>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawProduct {
    pub name: Option<String>,
    pub repository: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawOwner {
    pub tracker_user: Option<String>,
    pub github_login: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawTracker {
    pub kind: Option<String>,
    pub team: Option<String>,
    pub service_identity: Option<String>,
    pub team_keys: Option<Vec<String>>,
    pub link_fallback: Option<String>,
    pub statuses: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawRelease {
    pub candidate_tag: Option<String>,
    pub release_tag: Option<String>,
    pub branch_prefix: Option<String>,
    pub max_rc_passes: Option<i64>,
    pub later_passes: Option<String>,
    pub go_policy: Option<String>,
    pub nits: Option<String>,
    pub record: Option<String>,
    pub components: Option<Vec<String>>,
    pub version_component: Option<String>,
    pub release_pr_label: Option<String>,
    pub release_please_authors: Option<Vec<String>>,
    pub gate_record: Option<String>,
    pub bookkeeping_paths: Option<Vec<String>>,
    pub registry_path: Option<String>,
    pub business_rule_paths: Option<Vec<String>>,
    pub stagnation_hours: Option<i64>,
    pub coordinators: Option<Vec<String>>,
    pub coordinator_assignee: Option<String>,
    pub escalation_label: Option<String>,
    pub auto_candidate: Option<bool>,
    pub cut_workflow: Option<String>,
    pub gate_workflow: Option<String>,
    pub release_please_workflow: Option<String>,
    pub release_please_config: Option<String>,
    pub release_please_manifest: Option<String>,
    #[serde(default)]
    pub angles: Vec<RawReleaseAngle>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawReleaseAngle {
    pub key: Option<String>,
    pub title: Option<String>,
    pub assignee: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawReview {
    pub merge_method: Option<String>,
    pub require_up_to_date: Option<bool>,
    pub diff_size_extra_angle: Option<i64>,
    pub diff_size_angle: Option<String>,
    pub human_approval: Option<String>,
    pub sensitive_paths: Option<String>,
    pub sensitive_angle: Option<String>,
    pub coordinators: Option<Vec<String>>,
    pub coordinator_assignee: Option<String>,
    pub human_reviewer: Option<String>,
    pub ticketless_authors: Option<Vec<String>>,
    #[serde(default)]
    pub angles: Vec<RawReviewAngle>,
    #[serde(default)]
    pub path_angles: Vec<RawPathAngle>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawReviewAngle {
    pub key: Option<String>,
    pub assignee: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawPathAngle {
    pub paths: Option<Vec<String>>,
    pub angle: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawBridge {
    pub poll_minutes: Option<i64>,
    pub github_identity: Option<String>,
    pub sweep_hours: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawNotify {
    pub kind: Option<String>,
    pub target: Option<String>,
    pub token: Option<String>,
    pub events: Option<Vec<String>>,
}

/// The instance overlay as written (spec 08, "The instance overlay"): only
/// the instance keys, so that a wrong type is refused with its line. Which
/// keys are instance keys is checked before, in `super::overlay`, so that a
/// key of the method's shape is named as such rather than as unknown.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawOverlay {
    pub schema: Option<i64>,
    pub product: Option<RawOverlayProduct>,
    pub owner: Option<RawOwner>,
    pub tracker: Option<RawOverlayTracker>,
    pub release: Option<RawOverlayRelease>,
    pub review: Option<RawOverlayReview>,
    #[serde(default)]
    pub notify: Vec<RawNotify>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawOverlayProduct {
    pub repository: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawOverlayTracker {
    pub team: Option<String>,
    pub service_identity: Option<String>,
    pub team_keys: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawOverlayRelease {
    pub coordinators: Option<Vec<String>>,
    pub coordinator_assignee: Option<String>,
    pub release_please_authors: Option<Vec<String>>,
    #[serde(default)]
    pub angles: Vec<RawOverlayAngle>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawOverlayReview {
    pub coordinators: Option<Vec<String>>,
    pub coordinator_assignee: Option<String>,
    pub human_reviewer: Option<String>,
    #[serde(default)]
    pub angles: Vec<RawOverlayAngle>,
}

/// An angle of the repository's file, named by its key, given its assignee.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawOverlayAngle {
    pub key: Option<String>,
    pub assignee: Option<String>,
}
