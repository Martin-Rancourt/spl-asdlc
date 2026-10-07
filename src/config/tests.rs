use super::*;

const MINIMAL: &str = r#"
schema = 1

[product]
name = "acme"
repository = "example-org/acme"

[owner]
tracker_user = "owner-handle"

[tracker]
kind = "linear"
team = "ABC"
service_identity = "spl-asdlc"

[release]
release_please_authors = ["app/release-app"]
coordinators = ["coordinator-login"]

[[release.angles]]
key = "security"

[review]
coordinators = ["coordinator-login"]
diff_size_angle = "maintainability"

[[review.angles]]
key = "maintainability"
"#;

fn errors(text: &str) -> Vec<String> {
    match Config::parse(text) {
        Ok(_) => panic!("expected the file to be refused"),
        Err(e) => e.0.iter().map(|e| e.to_string()).collect(),
    }
}

fn assert_refused_with(text: &str, needle: &str) {
    let errs = errors(text);
    assert!(
        errs.iter().any(|e| e.contains(needle)),
        "no error contains {needle:?}; got {errs:#?}"
    );
}

#[test]
fn a_minimal_file_gets_every_documented_default() {
    let c = Config::parse(MINIMAL).expect("minimal file is valid");
    assert_eq!(c.release.candidate_tag, "acme-v{version}-rc.{rc}");
    assert_eq!(c.release.release_tag, "acme-v{version}");
    assert_eq!(c.release.branch_prefix, "release/");
    assert_eq!(c.release.max_rc_passes, 2);
    assert_eq!(c.release.later_passes, LaterPasses::Full);
    assert_eq!(c.release.go_policy, GoPolicy::Computed);
    assert_eq!(c.release.record, ".spl-asdlc/qa/{candidate}/");
    assert_eq!(c.release.release_pr_label, "autorelease: pending");
    assert_eq!(c.release.registry_path, "docs/business-rules.md");
    assert_eq!(
        c.release.release_please_config,
        "release-please-config.json"
    );
    assert_eq!(
        c.release.release_please_manifest,
        ".release-please-manifest.json"
    );
    assert_eq!(c.release.stagnation_hours, 2);
    assert_eq!(c.release.escalation_label, "blocked-by-human");
    assert!(!c.release.auto_candidate);
    assert!(c.release.bookkeeping_paths.is_empty());
    assert_eq!(c.release.gate_workflow, "release-gate.yml");
    assert_eq!(c.release.angles[0].title, "Security review");
    assert_eq!(c.review.merge_method, MergeMethod::Squash);
    assert!(c.review.require_up_to_date);
    assert_eq!(c.review.diff_size_extra_angle, 1500);
    assert_eq!(c.review.human_approval, HumanApproval::Never);
    assert_eq!(c.review.sensitive_paths, None);
    assert_eq!(c.review.sensitive_angle, "security");
    assert_eq!(c.bridge.poll_minutes, 5);
    assert_eq!(c.bridge.github_identity, GithubIdentity::App);
    assert_eq!(c.bridge.sweep_hours, 24);
    assert_eq!(c.tracker.link_fallback, LinkFallback::None);
    assert_eq!(c.tracker.statuses.len(), CANONICAL_STATUSES.len());
    assert_eq!(c.tracker.statuses["To Deploy"], "To Deploy");
    assert!(c.notify.is_empty());
}

#[test]
fn the_example_of_the_specification_is_a_valid_file() {
    let spec = include_str!("../../docs/spec/08-configuration.md");
    let start = spec
        .find("```toml\n")
        .expect("spec 08 carries a toml block")
        + "```toml\n".len();
    let end = start + spec[start..].find("```").expect("the toml block is closed");
    let c = Config::parse(&spec[start..end]).unwrap_or_else(|e| panic!("spec 08 example: {e}"));
    assert_eq!(c.notify.len(), 1);
}

#[test]
fn the_apps_whose_prs_need_no_ticket_are_read() {
    let spec = include_str!("../../docs/spec/08-configuration.md");
    let start = spec.find("```toml\n").unwrap() + "```toml\n".len();
    let end = start + spec[start..].find("```").unwrap();
    let c = Config::parse(&spec[start..end]).unwrap();
    assert_eq!(c.review.ticketless_authors, vec!["dependabot".to_string()]);
}

#[test]
fn an_unknown_key_is_refused_never_ignored() {
    let text = MINIMAL.replace("[bridge]", "").to_string() + "\n[bridge]\npoll_minute = 5\n";
    assert_refused_with(&text, "poll_minute");
}

#[test]
fn a_file_that_is_not_toml_is_refused() {
    assert_refused_with("schema = \n", "not a valid spl-asdlc.toml");
}

#[test]
fn the_schema_version_is_required_and_checked() {
    assert_refused_with(&MINIMAL.replace("schema = 1", ""), "schema: is required");
    assert_refused_with(
        &MINIMAL.replace("schema = 1", "schema = 2"),
        "version 2 is not read",
    );
}

#[test]
fn every_problem_is_reported_not_only_the_first() {
    let text = MINIMAL
        .replace("kind = \"linear\"", "kind = \"jira\"")
        .replace(
            "coordinators = [\"coordinator-login\"]\n\n[[release",
            "coordinators = []\n\n[[release",
        );
    let errs = errors(&text);
    assert!(
        errs.iter().any(|e| e.starts_with("tracker.kind")),
        "{errs:#?}"
    );
    assert!(
        errs.iter().any(|e| e.starts_with("release.coordinators")),
        "{errs:#?}"
    );
}

#[test]
fn an_unknown_choice_names_the_accepted_values() {
    assert_refused_with(
        &MINIMAL.replace("kind = \"linear\"", "kind = \"jira\""),
        "\"jira\" is not one of \"linear\", \"livela\"",
    );
}

#[test]
fn the_candidate_tag_needs_version_and_rc_exactly_once() {
    let base = MINIMAL.replace("[release]\n", "[release]\ncandidate_tag = \"TAG\"\n");
    assert_refused_with(
        &base.replace("TAG", "v{version}"),
        "must contain {rc} exactly once",
    );
    assert_refused_with(
        &base.replace("TAG", "v{version}-{version}-rc.{rc}"),
        "must contain {version} exactly once",
    );
    assert_refused_with(
        &base.replace("TAG", "v{version}-rc.{rc}{x}"),
        "unknown placeholder",
    );
    assert!(Config::parse(&base.replace("TAG", "v{version}-rc.{rc}")).is_ok());
}

#[test]
fn the_release_tag_carries_no_rc() {
    let text = MINIMAL.replace(
        "[release]\n",
        "[release]\nrelease_tag = \"v{version}-rc.{rc}\"\n",
    );
    assert_refused_with(&text, "must not contain {rc}");
}

#[test]
fn a_multi_component_product_names_its_version_component_and_gate_record() {
    let text = MINIMAL.replace(
        "[release]\n",
        "[release]\ncomponents = [\"app\", \"website\"]\n",
    );
    assert_refused_with(&text, "release.version_component: is required");
    assert_refused_with(&text, "release.gate_record: is required");
    let ok = MINIMAL.replace(
        "[release]\n",
        "[release]\ncomponents = [\"app\", \"website\"]\nversion_component = \"app\"\ngate_record = \"docs/releases/v{version}.md\"\n",
    );
    let c = Config::parse(&ok).expect("complete multi-component shape");
    assert!(c.release.multi_component());
}

#[test]
fn a_version_component_must_be_one_of_the_components() {
    let text = MINIMAL.replace(
        "[release]\n",
        "[release]\ncomponents = [\"app\"]\nversion_component = \"admin\"\ngate_record = \"r/v{version}.md\"\n",
    );
    assert_refused_with(&text, "is not one of release.components");
    let alone = MINIMAL.replace("[release]\n", "[release]\nversion_component = \"app\"\n");
    assert_refused_with(&alone, "release.components is empty");
}

#[test]
fn a_release_without_angles_is_refused() {
    let text = MINIMAL.replace("[[release.angles]]\nkey = \"security\"\n", "");
    assert_refused_with(&text, "release.angles: at least one angle is required");
}

#[test]
fn an_angle_key_is_a_branch_segment_and_unique() {
    let text = MINIMAL.replace("key = \"security\"", "key = \"Security Review\"");
    assert_refused_with(&text, "must be lower-case letters, digits and hyphens");
    let twice = MINIMAL.replace(
        "[[release.angles]]\nkey = \"security\"\n",
        "[[release.angles]]\nkey = \"security\"\n[[release.angles]]\nkey = \"security\"\n",
    );
    assert_refused_with(&twice, "\"security\" is configured twice");
}

#[test]
fn coordinators_and_release_please_authors_are_required() {
    assert_refused_with(
        &MINIMAL.replace("release_please_authors = [\"app/release-app\"]\n", ""),
        "release.release_please_authors: is required",
    );
    let no_review_coordinator = MINIMAL.replace(
        "[review]\ncoordinators = [\"coordinator-login\"]\n",
        "[review]\n",
    );
    assert_refused_with(&no_review_coordinator, "review.coordinators: is required");
}

#[test]
fn a_size_trigger_names_the_angle_it_adds_or_is_disabled() {
    let text = MINIMAL.replace("diff_size_angle = \"maintainability\"\n", "");
    assert_refused_with(&text, "review.diff_size_angle: is required while");
    let disabled = MINIMAL.replace(
        "diff_size_angle = \"maintainability\"\n",
        "diff_size_extra_angle = 0\n",
    );
    assert_eq!(
        Config::parse(&disabled)
            .unwrap()
            .review
            .diff_size_extra_angle,
        0
    );
}

#[test]
fn a_human_go_policy_needs_the_owner_github_login() {
    let text = MINIMAL.replace("[release]\n", "[release]\ngo_policy = \"human\"\n");
    assert_refused_with(
        &text,
        "owner.github_login: is required when release.go_policy",
    );
    let ok = text.replace(
        "tracker_user = \"owner-handle\"",
        "tracker_user = \"owner-handle\"\ngithub_login = \"owner-login\"",
    );
    assert_eq!(
        Config::parse(&ok).unwrap().release.go_policy,
        GoPolicy::Human
    );
}

#[test]
fn sensitive_path_approval_needs_the_sensitive_path_file() {
    let text = MINIMAL.replace(
        "[review]\n",
        "[review]\nhuman_approval = \"sensitive-paths\"\n",
    );
    assert_refused_with(&text, "review.sensitive_paths: is required");
}

#[test]
fn status_mapping_accepts_only_canonical_names_and_no_collision() {
    let unknown = MINIMAL.replace(
        "service_identity = \"spl-asdlc\"\n",
        "service_identity = \"spl-asdlc\"\n\n[tracker.statuses]\n\"Shipped\" = \"Done\"\n",
    );
    assert_refused_with(&unknown, "\"Shipped\" is not a canonical status");
    let collide = MINIMAL.replace(
        "service_identity = \"spl-asdlc\"\n",
        "service_identity = \"spl-asdlc\"\n\n[tracker.statuses]\n\"To Deploy\" = \"Done\"\n",
    );
    assert_refused_with(&collide, "both map to the tracker status \"Done\"");
    let renamed = MINIMAL.replace(
        "service_identity = \"spl-asdlc\"\n",
        "service_identity = \"spl-asdlc\"\n\n[tracker.statuses]\n\"To Deploy\" = \"To deploy\"\n",
    );
    assert_eq!(
        Config::parse(&renamed).unwrap().tracker.statuses["To Deploy"],
        "To deploy"
    );
}

#[test]
fn a_secret_is_a_reference_never_a_value() {
    let with = |target: &str| {
        format!(
            "{MINIMAL}\n[[notify]]\nkind = \"telegram\"\ntarget = \"{target}\"\ntoken = \"env:BOT_TOKEN\"\nevents = [\"release-note\"]\n"
        )
    };
    assert_refused_with(
        &with("a-chat-id"),
        "must be a secret reference `env:<VARIABLE>`",
    );
    assert_refused_with(&with("env:lower"), "must be a secret reference");
    let c = Config::parse(&with("env:RELEASE_CHAT")).unwrap();
    assert_eq!(c.notify[0].target, SecretRef::Env("RELEASE_CHAT".into()));
    assert_eq!(c.notify[0].target.to_string(), "env:RELEASE_CHAT");
}

#[test]
fn a_channel_this_build_does_not_ship_is_refused() {
    let text = format!(
        "{MINIMAL}\n[[notify]]\nkind = \"email\"\ntarget = \"env:TO\"\nevents = [\"escalation\"]\n"
    );
    assert_refused_with(&text, "ships the \"telegram\" channel only");
}

#[test]
fn a_notification_event_must_be_known() {
    let text = format!(
        "{MINIMAL}\n[[notify]]\nkind = \"telegram\"\ntarget = \"env:TO\"\ntoken = \"env:T\"\nevents = [\"gate-green\"]\n"
    );
    assert_refused_with(&text, "\"gate-green\" is not one of");
}

#[test]
fn non_positive_counts_are_refused_never_defaulted_silently() {
    assert_refused_with(
        &MINIMAL.replace("[release]\n", "[release]\nmax_rc_passes = 0\n"),
        "release.max_rc_passes: must be a positive integer, got 0",
    );
    assert_refused_with(
        &format!("{MINIMAL}\n[bridge]\npoll_minutes = -5\n"),
        "bridge.poll_minutes: must be a positive integer",
    );
}

#[test]
fn product_and_repository_shapes_are_checked() {
    assert_refused_with(
        &MINIMAL.replace("name = \"acme\"", "name = \"Acme\""),
        "product.name",
    );
    assert_refused_with(
        &MINIMAL.replace("repository = \"example-org/acme\"", "repository = \"acme\""),
        "must be `<owner>/<repo>`",
    );
}

#[test]
fn the_owner_table_is_required() {
    assert_refused_with(
        &MINIMAL.replace("[owner]\ntracker_user = \"owner-handle\"\n", ""),
        "the [owner] table is required",
    );
}

#[test]
fn a_duplicated_list_entry_is_refused() {
    let text = MINIMAL.replace(
        "release_please_authors = [\"app/release-app\"]",
        "release_please_authors = [\"app/release-app\", \"app/release-app\"]",
    );
    assert_refused_with(&text, "is listed twice");
}

#[test]
fn a_workflow_is_a_file_name() {
    let text = MINIMAL.replace(
        "[release]\n",
        "[release]\ngate_workflow = \".github/workflows/gate.yml\"\n",
    );
    assert_refused_with(&text, "must be a workflow file name");
}

#[test]
fn team_keys_are_capital_letters() {
    let text = MINIMAL.replace(
        "service_identity = \"spl-asdlc\"\n",
        "service_identity = \"spl-asdlc\"\nteam_keys = [\"abc\"]\n",
    );
    assert_refused_with(&text, "must be capital letters and digits");
    let digits = MINIMAL.replace(
        "service_identity = \"spl-asdlc\"\n",
        "service_identity = \"spl-asdlc\"\nteam_keys = [\"AB1\"]\n",
    );
    assert!(Config::parse(&digits).is_ok());
}

#[test]
fn a_bad_release_value_does_not_hide_the_owner_requirement() {
    let text = MINIMAL.replace(
        "[release]\n",
        "[release]\ngo_policy = \"human\"\nlater_passes = \"some\"\n",
    );
    assert_refused_with(&text, "release.later_passes");
    assert_refused_with(
        &text,
        "owner.github_login: is required when release.go_policy",
    );
}
