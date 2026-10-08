use super::*;
use spec_example::source;

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
    assert_eq!(c.release.nits, NitsGrouping::Shared);
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
fn the_example_of_the_specification_is_valid_once_merged() {
    let c = spec_example::config();
    assert_eq!(c.notify.len(), 1);
    assert_eq!(c.product.repository, "example-org/acme");
    assert_eq!(
        c.release.angles[0].assignee.as_deref(),
        Some("reviewer-handle")
    );
    assert_eq!(
        c.release.angles[0].notes.as_deref(),
        Some(".spl-asdlc/angles/security.md")
    );
    assert_eq!(c.release.angles[1].assignee, None);
}

#[test]
fn the_repository_file_of_the_example_lacks_instance_keys_only() {
    let errs = match Config::parse(spec_example::repository()) {
        Ok(_) => panic!("the neutral file names no repository, no owner, no coordinator"),
        Err(e) => e.0,
    };
    assert!(
        errs.iter().any(|e| e.key == "product.repository"),
        "{errs:#?}"
    );
    for e in &errs {
        assert!(overlay::is_instance_key(&e.key), "not an instance key: {e}");
        assert!(e.message.contains("instance overlay"), "{e}");
    }
}

#[test]
fn the_apps_whose_prs_need_no_ticket_are_read() {
    let c = spec_example::config();
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

#[test]
fn the_nits_grouping_is_shared_or_per_nit_and_nothing_else() {
    let per_nit = MINIMAL.replace("[release]\n", "[release]\nnits = \"per-nit\"\n");
    assert_eq!(
        Config::parse(&per_nit).unwrap().release.nits,
        NitsGrouping::PerNit
    );
    let shared = MINIMAL.replace("[release]\n", "[release]\nnits = \"shared\"\n");
    assert_eq!(
        Config::parse(&shared).unwrap().release.nits,
        NitsGrouping::Shared
    );
    let other = MINIMAL.replace("[release]\n", "[release]\nnits = \"none\"\n");
    assert_refused_with(
        &other,
        "release.nits: \"none\" is not one of \"shared\", \"per-nit\"",
    );
}

// ---- the instance overlay (spec 08, BR-ASDLC-014) ----

/// MINIMAL without its instance keys.
const NEUTRAL: &str = r#"
schema = 1

[product]
name = "acme"

[tracker]
kind = "linear"

[[release.angles]]
key = "security"

[review]
diff_size_angle = "maintainability"

[[review.angles]]
key = "maintainability"
"#;

/// The instance keys of MINIMAL.
const OVERLAY: &str = r#"
schema = 1

[product]
repository = "example-org/acme"

[owner]
tracker_user = "owner-handle"

[tracker]
team = "ABC"
service_identity = "spl-asdlc"

[release]
release_please_authors = ["app/release-app"]
coordinators = ["coordinator-login"]

[review]
coordinators = ["coordinator-login"]
"#;

fn merged(repository: &str, instance: &str) -> Result<Config, Vec<String>> {
    Config::parse_with(
        &source(FILE_NAME, repository),
        Some(&source(INSTANCE_FILE_NAME, instance)),
    )
    .map_err(|e| e.0.iter().map(|e| e.to_string()).collect())
}

fn assert_merge_refused_with(repository: &str, instance: &str, needle: &str) {
    match merged(repository, instance) {
        Ok(_) => panic!("expected the merge to be refused ({needle:?})"),
        Err(errs) => assert!(
            errs.iter().any(|e| e.contains(needle)),
            "no error contains {needle:?}; got {errs:#?}"
        ),
    }
}

#[test]
fn a_neutral_file_and_its_overlay_read_as_the_single_file() {
    let single = Config::parse(MINIMAL).unwrap();
    assert_eq!(merged(NEUTRAL, OVERLAY).unwrap(), single);
    let none = Config::parse_with(&source(FILE_NAME, MINIMAL), None).unwrap();
    assert_eq!(none, single, "without an overlay the file stands alone");
}

#[test]
fn a_key_in_both_files_is_refused_never_overridden() {
    let both = NEUTRAL.replace(
        "name = \"acme\"\n",
        "name = \"acme\"\nrepository = \"example-org/acme\"\n",
    );
    assert_merge_refused_with(
        &both,
        OVERLAY,
        "product.repository: is set in both spl-asdlc.toml and spl-asdlc.instance.toml (line 5)",
    );
    let other = NEUTRAL.replace(
        "[review]\n",
        "[review]\ncoordinators = [\"another-login\"]\n",
    );
    assert_merge_refused_with(&other, OVERLAY, "review.coordinators: is set in both");
}

#[test]
fn an_angle_assignee_and_the_channels_are_set_in_one_file_only() {
    let assigned = NEUTRAL.replace(
        "key = \"security\"\n",
        "key = \"security\"\nassignee = \"reviewer-handle\"\n",
    );
    let overlay = format!(
        "{OVERLAY}\n[[release.angles]]\nkey = \"security\"\nassignee = \"reviewer-handle\"\n"
    );
    assert_merge_refused_with(
        &assigned,
        &overlay,
        "release.angles[0].assignee: is set in both",
    );
    let channel = "\n[[notify]]\nkind = \"telegram\"\ntarget = \"env:TO\"\ntoken = \"env:T\"\nevents = [\"escalation\"]\n";
    assert_merge_refused_with(
        &format!("{NEUTRAL}{channel}"),
        &format!("{OVERLAY}{channel}"),
        "notify: is set in both",
    );
    let c = merged(NEUTRAL, &format!("{overlay}{channel}")).unwrap();
    assert_eq!(
        c.release.angles[0].assignee.as_deref(),
        Some("reviewer-handle")
    );
    assert_eq!(c.notify.len(), 1);
}

#[test]
fn a_key_that_is_not_an_instance_key_is_refused_with_its_line() {
    let shape = OVERLAY.replace(
        "repository = \"example-org/acme\"\n",
        "repository = \"example-org/acme\"\nname = \"other\"\n",
    );
    assert_merge_refused_with(
        NEUTRAL,
        &shape,
        "spl-asdlc.instance.toml: product.name: line 6: not an instance key",
    );
    let table = format!("{OVERLAY}\n[bridge]\npoll_minutes = 1\n");
    assert_merge_refused_with(NEUTRAL, &table, "bridge: line 21: not an instance key");
    let angle = format!("{OVERLAY}\n[[review.angles]]\nkey = \"maintainability\"\ntitle = \"T\"\n");
    assert_merge_refused_with(
        NEUTRAL,
        &angle,
        "review.angles[0].title: line 23: not an instance key",
    );
}

#[test]
fn an_overlay_value_of_the_wrong_type_is_refused_with_its_line() {
    let text = OVERLAY.replace("team = \"ABC\"", "team = 5");
    let errs = merged(NEUTRAL, &text).unwrap_err();
    assert!(
        errs[0].starts_with("spl-asdlc.instance.toml: not a valid instance overlay")
            && errs[0].contains("line 11"),
        "{errs:#?}"
    );
}

#[test]
fn an_assignee_for_an_angle_the_repository_does_not_declare_is_refused() {
    let text = format!(
        "{OVERLAY}\n[[release.angles]]\nkey = \"design\"\nassignee = \"reviewer-handle\"\n"
    );
    assert_merge_refused_with(
        NEUTRAL,
        &text,
        "release.angles[0].key: \"design\" is not an angle of spl-asdlc.toml",
    );
    let bare = format!("{OVERLAY}\n[[release.angles]]\nkey = \"security\"\n");
    assert_merge_refused_with(NEUTRAL, &bare, "release.angles[0].assignee: is required");
}

#[test]
fn the_overlay_carries_its_schema_version() {
    assert_merge_refused_with(
        NEUTRAL,
        &OVERLAY.replace("schema = 1", ""),
        "spl-asdlc.instance.toml: schema: is required",
    );
    assert_merge_refused_with(
        NEUTRAL,
        &OVERLAY.replace("schema = 1", "schema = 2"),
        "version 2 is not read",
    );
}

#[test]
fn every_rule_is_checked_on_the_merge_and_names_the_file_of_its_value() {
    let repository = NEUTRAL.replace("kind = \"linear\"", "kind = \"jira\"");
    let instance = OVERLAY.replace(
        "[release]\nrelease_please_authors = [\"app/release-app\"]\ncoordinators = [\"coordinator-login\"]",
        "[release]\nrelease_please_authors = [\"app/release-app\"]\ncoordinators = []",
    );
    let errs = merged(&repository, &instance).unwrap_err();
    assert!(
        errs.iter()
            .any(|e| e.starts_with("spl-asdlc.toml: tracker.kind")),
        "{errs:#?}"
    );
    assert!(
        errs.iter()
            .any(|e| e.starts_with("spl-asdlc.instance.toml: release.coordinators")),
        "{errs:#?}"
    );
    let missing = OVERLAY.replace("[owner]\ntracker_user = \"owner-handle\"\n", "");
    assert_merge_refused_with(
        NEUTRAL,
        &missing,
        "spl-asdlc.toml: owner: the [owner] table is required",
    );
}

#[test]
fn the_overlay_is_the_one_named_or_else_the_sibling_when_present() {
    let dir = std::env::temp_dir().join(format!("spl-asdlc-overlay-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join(FILE_NAME);
    std::fs::write(&file, NEUTRAL).unwrap();
    assert_eq!(
        instance_path(&file, None).unwrap(),
        None,
        "no sibling, no overlay"
    );
    assert!(
        Config::load(&file).is_err(),
        "a neutral file alone is incomplete"
    );
    let named = dir.join("elsewhere.toml");
    assert!(
        instance_path(&file, Some(&named)).is_err(),
        "a named overlay that does not exist is an error"
    );
    let sibling = dir.join(INSTANCE_FILE_NAME);
    std::fs::write(&sibling, OVERLAY).unwrap();
    assert_eq!(instance_path(&file, None).unwrap(), Some(sibling.clone()));
    assert_eq!(
        Config::load(&file).unwrap(),
        Config::parse(MINIMAL).unwrap()
    );
    std::fs::write(&named, OVERLAY).unwrap();
    std::fs::write(&sibling, "schema = 1\n[product]\nname = \"x\"\n").unwrap();
    let (c, read) =
        Config::load_with(&file, Some(&named)).expect("the named overlay wins over the sibling");
    assert_eq!(c.product.repository, "example-org/acme");
    assert_eq!(read.unwrap().label, named.display().to_string());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn two_configurations_in_one_directory_never_share_an_overlay() {
    let dir = std::env::temp_dir().join(format!("spl-asdlc-overlay-two-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let acme = dir.join("acme.toml");
    let beta = dir.join("beta.toml");
    std::fs::write(&acme, NEUTRAL).unwrap();
    std::fs::write(&beta, NEUTRAL).unwrap();
    std::fs::write(dir.join(INSTANCE_FILE_NAME), OVERLAY).unwrap();
    assert_eq!(
        instance_path(&beta, None).unwrap(),
        None,
        "spl-asdlc.instance.toml belongs to spl-asdlc.toml only"
    );
    assert!(
        Config::load(&beta).is_err(),
        "beta takes nobody's identities"
    );
    std::fs::write(dir.join("acme.instance.toml"), OVERLAY).unwrap();
    assert_eq!(
        instance_path(&acme, None).unwrap(),
        Some(dir.join("acme.instance.toml"))
    );
    assert_eq!(instance_path(&beta, None).unwrap(), None);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn something_at_the_siblings_path_that_is_not_a_file_is_an_error() {
    let dir = std::env::temp_dir().join(format!("spl-asdlc-overlay-odd-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join(FILE_NAME);
    std::fs::write(&file, NEUTRAL).unwrap();
    std::fs::create_dir(dir.join(INSTANCE_FILE_NAME)).unwrap();
    assert!(instance_path(&file, None).is_err(), "a directory");
    #[cfg(unix)]
    {
        std::fs::remove_dir(dir.join(INSTANCE_FILE_NAME)).unwrap();
        std::os::unix::fs::symlink(dir.join("gone.toml"), dir.join(INSTANCE_FILE_NAME)).unwrap();
        assert!(instance_path(&file, None).is_err(), "a dangling link");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
