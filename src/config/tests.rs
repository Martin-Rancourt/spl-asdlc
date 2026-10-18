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
    assert_eq!(c.release.fix_assignee, None);
    assert_eq!(c.release.record, ".spl-asdlc/qa/{candidate}/");
    assert!(c.release.version_files.is_empty());
    assert_eq!(c.release.changelog_sections, default_changelog_sections());
    assert_eq!(c.release.stagnation_hours, 2);
    assert_eq!(c.release.escalation_label, "blocked-by-human");
    assert_eq!(c.release.hold_label, "hold");
    assert!(!c.release.auto_candidate);
    assert!(c.release.bookkeeping_paths.is_empty());
    assert_eq!(c.release.gate_workflow, "release-gate.yml");
    assert_eq!(c.release.gate_rerun_workflow, "release-gate-auto-rerun.yml");
    assert_eq!(c.release.publish_workflow, "release-publish.yml");
    assert_eq!(c.release.gate_rerun_wait_minutes, 30);
    assert_eq!(c.release.angles[0].title, "Security review");
    assert_eq!(c.review.merge_method, MergeMethod::Squash);
    assert!(c.review.require_up_to_date);
    assert_eq!(c.review.diff_size_extra_angle, 0, "no size trigger");
    assert_eq!(c.review.human_approval, HumanApproval::Never);
    assert!(c.review.angles[0].paths.is_empty(), "on every PR");
    assert_eq!(c.bridge.poll_minutes, 5);
    assert_eq!(c.bridge.github_identity, GithubIdentity::App);
    assert_eq!(c.bridge.sweep_hours, 24);
    assert_eq!(c.tracker.link_fallback, LinkFallback::None);
    assert_eq!(c.tracker.no_deploy_label, "no-deploy");
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
    assert_eq!(
        c.review.angles[0].notes.as_deref(),
        Some(".spl-asdlc/angles/maintainability.md")
    );
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
    assert_refused_with("schema = \n", "not a valid configuration");
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
fn a_multi_component_product_is_refused_until_the_cycle_supports_it() {
    for key in [
        "components = [\"app\", \"website\"]",
        "version_component = \"app\"",
        "gate_record = \"docs/releases/v{version}.md\"",
    ] {
        let text = MINIMAL.replace("[release]\n", &format!("[release]\n{key}\n"));
        assert_refused_with(&text, "a multi-component product is not supported");
    }
    // An empty list says nothing more than its absence.
    let empty = MINIMAL.replace("[release]\n", "[release]\ncomponents = []\n");
    Config::parse(&empty).expect("no component");
}

#[test]
fn the_release_please_keys_are_unknown_keys() {
    for key in [
        "release_pr_label = \"autorelease: pending\"",
        "release_please_authors = [\"app/release-app\"]",
        "release_please_workflow = \"release-please.yml\"",
        "release_please_config = \"release-please-config.json\"",
        "release_please_manifest = \".release-please-manifest.json\"",
    ] {
        let text = MINIMAL.replace("[release]\n", &format!("[release]\n{key}\n"));
        assert_refused_with(&text, "unknown field");
    }
}

#[test]
fn the_business_rules_have_a_fixed_place_and_no_key() {
    // `.spl-asdlc/business-rules/`, one file per rule (spec 02): a key that
    // moved it would be a second place to look.
    for key in [
        "registry_path = \"docs/business-rules.md\"",
        "business_rule_paths = [\"docs/business-rules.md\"]",
    ] {
        let text = MINIMAL.replace("[release]\n", &format!("[release]\n{key}\n"));
        assert_refused_with(&text, "unknown field");
    }
}

/// MINIMAL with `entries` written under `[release]`, after its angles.
fn with_release_tables(entries: &str) -> String {
    MINIMAL.replace("\n[review]", &format!("\n{entries}\n[review]"))
}

#[test]
fn version_files_name_a_path_and_a_pattern_with_one_version() {
    let c = Config::parse(&with_release_tables(
        "[[release.version_files]]\npath = \"web/package.json\"\npattern = '\"version\": \"{version}\"'\n",
    ))
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        c.release.version_files,
        vec![VersionFile {
            path: "web/package.json".into(),
            pattern: "\"version\": \"{version}\"".into(),
        }]
    );
    assert_refused_with(
        &with_release_tables(
            "[[release.version_files]]\npath = \"web/package.json\"\npattern = \"no version\"\n",
        ),
        "must contain {version} exactly once",
    );
    assert_refused_with(
        &with_release_tables(
            "[[release.version_files]]\npath = \"../x\"\npattern = \"v{version}\"\n",
        ),
        "must be a path inside the repository",
    );
    assert_refused_with(
        &with_release_tables("[[release.version_files]]\npattern = \"v{version}\"\n"),
        "path: is required",
    );
}

#[test]
fn changelog_sections_name_each_type_once() {
    let c = Config::parse(&with_release_tables(
        "[[release.changelog_sections]]\ntype = \"feat\"\nsection = \"New\"\n",
    ))
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        c.release.changelog_sections,
        vec![ChangelogSection {
            kind: "feat".into(),
            section: "New".into()
        }]
    );
    assert_refused_with(
        &with_release_tables(
            "[[release.changelog_sections]]\ntype = \"Feat\"\nsection = \"New\"\n",
        ),
        "must be a Conventional Commit type",
    );
    assert_refused_with(
        &with_release_tables(
            "[[release.changelog_sections]]\ntype = \"fix\"\nsection = \"A\"\n[[release.changelog_sections]]\ntype = \"fix\"\nsection = \"B\"\n",
        ),
        "\"fix\" is listed twice",
    );
    assert_refused_with(
        &MINIMAL.replace("[release]\n", "[release]\nchangelog_sections = []\n"),
        "must name at least one type",
    );
}

/// What `release prepare` reads in CI, where no overlay exists: the shape
/// alone, with the same rules, and no instance key required.
#[test]
fn the_preparation_reads_the_shape_of_a_neutral_file() {
    let p = Config::preparation(&with_release_tables(
        "[[release.version_files]]\npath = \"VERSION\"\npattern = \"{version}\"\n",
    ))
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(p.product, "acme");
    assert_eq!(p.repository.as_deref(), Some("example-org/acme"));
    assert_eq!(p.release_tag, "acme-v{version}");
    assert_eq!(p.version_files.len(), 1);
    assert_eq!(p.changelog_sections, default_changelog_sections());
    let neutral = Config::preparation(
        "schema = 1\n[product]\nname = \"acme\"\n[release]\nrelease_tag = \"v{version}\"\nangles = []\n",
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(neutral.repository, None);
    assert_eq!(neutral.release_tag, "v{version}");
    let errs = |text: &str| {
        Config::preparation(text)
            .unwrap_err()
            .0
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert!(
        errs("schema = 1\n[product]\nname = \"acme\"\n")
            .contains("the [release] section is required"),
        "every project has the release cycle"
    );
    assert!(
        errs("schema = 1\n[product]\nname = \"acme\"\n[release]\nrelease_please_authors = []\n")
            .contains("unknown field")
    );
    assert!(
        errs("schema = 1\n[product]\nname = \"acme\"\n[release]\nrelease_tag = \"v1\"\n")
            .contains("must contain {version} exactly once")
    );
}

// ---- spec 04 §11: a project may configure no PR review ----

/// MINIMAL with no PR review: no review angle, written explicitly, and
/// nothing else (no size threshold is the default), no coordinator.
fn no_review() -> String {
    let text = MINIMAL.replace(
        "[review]\ncoordinators = [\"coordinator-login\"]\ndiff_size_angle = \"maintainability\"\n\n[[review.angles]]\nkey = \"maintainability\"\n",
        "[review]\nangles = []\n",
    );
    assert_ne!(text, MINIMAL, "the replacement applied");
    text
}

#[test]
fn an_explicit_empty_angle_list_is_no_pr_review_and_needs_no_coordinator() {
    let c = Config::parse(&no_review()).expect("no PR review is a valid choice");
    assert!(c.review.angles.is_empty());
    assert!(c.review.coordinators.is_empty());
    assert!(!c.review.angle_possible());
    // A coordinator may still be named; it is never required.
    let named = no_review().replace(
        "angles = []\n",
        "angles = []\ncoordinators = [\"coordinator-login\"]\n",
    );
    assert!(!Config::parse(&named).unwrap().review.angle_possible());
}

#[test]
fn absent_base_angles_are_refused_never_read_as_none() {
    let text = MINIMAL.replace("[[review.angles]]\nkey = \"maintainability\"\n", "");
    assert_refused_with(&text, "review.angles: is required");
    let absent = no_review().replace("angles = []\n", "");
    assert_refused_with(&absent, "review.angles: is required");
    assert_refused_with(&absent, "`angles = []`");
}

#[test]
fn the_coordinators_are_required_while_some_angle_can_be_added() {
    let text = no_review();
    let cases = [
        (
            "an angle by size",
            text.replace(
                "angles = []\n",
                "angles = []\ndiff_size_extra_angle = 1500\ndiff_size_angle = \"maintainability\"\n",
            ),
        ),
        (
            "an angle on its paths",
            text.replace(
                "angles = []\n",
                "angles = [{ key = \"design\", paths = [\"ui/**\"] }]\n",
            ),
        ),
        (
            "an angle on every PR",
            text.replace(
                "angles = []\n",
                "angles = [{ key = \"maintainability\" }]\n",
            ),
        ),
    ];
    for (adds, file) in cases {
        let errs = errors(&file);
        assert!(
            errs.iter()
                .any(|e| e.starts_with("review.coordinators: is required")
                    && e.contains("while a PR can get an angle")),
            "{adds}: {errs:#?}"
        );
    }
}

#[test]
fn a_human_approval_with_no_possible_angle_is_accepted() {
    let text = no_review().replace(
        "angles = []\n",
        "angles = []\nhuman_approval = \"always\"\n",
    );
    let c = Config::parse(&text).expect("a human approval is then the only one");
    assert_eq!(c.review.human_approval, HumanApproval::Always);
    assert!(!c.review.angle_possible());
}

#[test]
fn a_release_without_angles_is_refused() {
    let text = MINIMAL.replace("[[release.angles]]\nkey = \"security\"\n", "");
    assert_refused_with(&text, "release.angles: at least one angle is required");
}

/// MINIMAL with `angles = []` under [release] and no release coordinator.
fn no_qa_angle() -> String {
    let text = MINIMAL.replace(
        "coordinators = [\"coordinator-login\"]\n\n[[release.angles]]\nkey = \"security\"\n",
        "angles = []\n",
    );
    assert_ne!(text, MINIMAL);
    text
}

#[test]
fn a_release_with_no_qa_angle_is_written_explicitly_and_needs_no_coordinator() {
    let c = Config::parse(&no_qa_angle()).unwrap();
    assert!(c.release.angles.is_empty());
    assert!(c.release.coordinators.is_empty());
    let with = no_qa_angle().replace(
        "angles = []\n",
        "angles = []\ncoordinators = [\"coordinator-login\"]\n",
    );
    assert_eq!(
        Config::parse(&with).unwrap().release.coordinators,
        vec!["coordinator-login"],
        "a coordinator named anyway is read"
    );
}

#[test]
fn an_absent_angle_list_is_refused_and_says_how_to_write_none() {
    let text = MINIMAL.replace("[[release.angles]]\nkey = \"security\"\n", "");
    assert_refused_with(&text, "or `angles = []` written under [release]");
}

#[test]
fn the_release_coordinators_are_required_while_there_is_a_release_angle() {
    let text = MINIMAL.replace(
        "coordinators = [\"coordinator-login\"]\n\n[[release.angles]]",
        "\n[[release.angles]]",
    );
    assert_refused_with(&text, "release.coordinators: is required");
    let empty = MINIMAL.replace(
        "coordinators = [\"coordinator-login\"]\n\n[[release.angles]]",
        "coordinators = []\n\n[[release.angles]]",
    );
    assert_refused_with(&empty, "release.coordinators: must name at least one entry");
}

#[test]
fn a_release_with_no_qa_angle_reads_the_same_through_the_overlay() {
    let repository = NEUTRAL.replace(
        "[[release.angles]]\nkey = \"security\"\n",
        "[release]\nangles = []\n",
    );
    assert_ne!(repository, NEUTRAL);
    let overlay = OVERLAY.replace(
        "coordinators = [\"coordinator-login\"]\n\n[review]",
        "\n[review]",
    );
    assert_ne!(overlay, OVERLAY);
    let c = merged(&repository, &overlay).unwrap();
    assert!(c.release.angles.is_empty());
    assert!(c.release.coordinators.is_empty());
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
fn coordinators_are_required() {
    assert_refused_with(
        &MINIMAL.replace(
            "coordinators = [\"coordinator-login\"]\n\n[[release.angles]]",
            "\n[[release.angles]]",
        ),
        "release.coordinators: is required",
    );
    let no_review_coordinator = MINIMAL.replace(
        "[review]\ncoordinators = [\"coordinator-login\"]\n",
        "[review]\n",
    );
    assert_refused_with(&no_review_coordinator, "review.coordinators: is required");
}

#[test]
fn a_size_trigger_is_off_by_default_and_names_the_angle_it_adds() {
    // Off unless asked for: no threshold, no angle to name.
    let off = MINIMAL.replace("diff_size_angle = \"maintainability\"\n", "");
    assert_eq!(Config::parse(&off).unwrap().review.diff_size_extra_angle, 0);
    // A threshold above 0 names the angle it adds.
    let unnamed = off.replace("[review]\n", "[review]\ndiff_size_extra_angle = 1500\n");
    assert_refused_with(&unnamed, "review.diff_size_angle: is required while");
    let on = MINIMAL.replace("[review]\n", "[review]\ndiff_size_extra_angle = 1500\n");
    let r = Config::parse(&on).unwrap().review;
    assert_eq!(r.diff_size_extra_angle, 1500);
    assert_eq!(r.diff_size_angle.as_deref(), Some("maintainability"));
    // 0 written explicitly stays valid.
    let zero = off.replace("[review]\n", "[review]\ndiff_size_extra_angle = 0\n");
    assert_eq!(
        Config::parse(&zero).unwrap().review.diff_size_extra_angle,
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
fn the_sensitive_paths_mode_and_its_keys_are_refused() {
    let text = MINIMAL.replace(
        "[review]\n",
        "[review]\nhuman_approval = \"sensitive-paths\"\n",
    );
    assert_refused_with(
        &text,
        "review.human_approval: \"sensitive-paths\" is not one of \"never\", \"always\"",
    );
    for key in [
        "sensitive_paths = \".github/review-paths.json\"",
        "sensitive_angle = \"security\"",
    ] {
        let text = MINIMAL.replace("[review]\n", &format!("[review]\n{key}\n"));
        assert_refused_with(&text, key.split(' ').next().unwrap());
    }
    let path_angle =
        format!("{MINIMAL}\n[[review.path_angles]]\npaths = [\"ui/**\"]\nangle = \"design\"\n");
    assert_refused_with(&path_angle, "path_angles");
}

#[test]
fn a_review_angle_carries_its_paths_or_runs_on_every_pr() {
    let text = MINIMAL.replace(
        "[[review.angles]]\nkey = \"maintainability\"\n",
        "[[review.angles]]\nkey = \"maintainability\"\n\n[[review.angles]]\nkey = \"security\"\npaths = [\"src/auth/**\", \".github/workflows/**\"]\n",
    );
    let c = Config::parse(&text).unwrap();
    assert!(c.review.angles[0].paths.is_empty());
    assert_eq!(
        c.review.angles[1].paths,
        vec![
            "src/auth/**".to_string(),
            ".github/workflows/**".to_string()
        ]
    );
    assert_refused_with(
        &text.replace(
            "paths = [\"src/auth/**\", \".github/workflows/**\"]",
            "paths = []",
        ),
        "review.angles[1].paths: must name at least one glob",
    );
    assert_refused_with(
        &text.replace(".github/workflows/**", "src/auth/**"),
        "review.angles[1].paths[1]: \"src/auth/**\" is listed twice",
    );
    // A release angle reviews the whole candidate: it takes no paths.
    assert_refused_with(
        &MINIMAL.replace(
            "[[release.angles]]\nkey = \"security\"\n",
            "[[release.angles]]\nkey = \"security\"\npaths = [\"src/**\"]\n",
        ),
        "paths",
    );
}

#[test]
fn the_hands_off_label_is_refused_as_unknown() {
    assert_refused_with(
        &format!("{MINIMAL}\n[bridge]\nhands_off_label = \"by-hand\"\n"),
        "hands_off_label",
    );
}

#[test]
fn a_tracker_status_mapping_is_refused_as_unknown() {
    let mapped = MINIMAL.replace(
        "service_identity = \"spl-asdlc\"\n",
        "service_identity = \"spl-asdlc\"\n\n[tracker.statuses]\n\"To Deploy\" = \"To deploy\"\n",
    );
    let errs = errors(&mapped);
    assert!(
        errs.len() == 1
            && errs[0].contains("at line 16")
            && errs[0].contains("unknown field `statuses`"),
        "{errs:#?}"
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
        &MINIMAL.replace("[release]\n", "[release]\ngate_rerun_wait_minutes = 0\n"),
        "release.gate_rerun_wait_minutes: must be a positive integer, got 0",
    );
    assert_refused_with(
        &format!("{MINIMAL}\n[bridge]\npoll_minutes = -5\n"),
        "bridge.poll_minutes: must be a positive integer",
    );
}

#[test]
fn the_no_deploy_label_is_configurable_and_never_empty() {
    let text = MINIMAL.replace(
        "service_identity = \"spl-asdlc\"\n",
        "service_identity = \"spl-asdlc\"\nno_deploy_label = \"docs-only\"\n",
    );
    let c = Config::parse(&text).expect("a no-deploy label is valid");
    assert_eq!(c.tracker.no_deploy_label, "docs-only");
    assert_refused_with(
        &MINIMAL.replace(
            "service_identity = \"spl-asdlc\"\n",
            "service_identity = \"spl-asdlc\"\nno_deploy_label = \"\"\n",
        ),
        "tracker.no_deploy_label: must not be empty when given",
    );
}

#[test]
fn the_hold_label_is_configurable_and_never_the_escalation_label() {
    let with = |line: &str| MINIMAL.replacen("[release]\n", &format!("[release]\n{line}\n"), 1);
    let c = Config::parse(&with("hold_label = \"on-hold\"")).expect("a hold label is valid");
    assert_eq!(c.release.hold_label, "on-hold");
    assert_refused_with(
        &with("hold_label = \"\""),
        "release.hold_label: must not be empty when given",
    );
    assert_refused_with(
        &with("hold_label = \"Blocked-By-Human\""),
        "release.hold_label: \"Blocked-By-Human\" is also release.escalation_label",
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
    let text = MINIMAL.replacen(
        "coordinators = [\"coordinator-login\"]",
        "coordinators = [\"coordinator-login\", \"coordinator-login\"]",
        1,
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
fn the_nits_grouping_is_gone_and_refused_as_unknown() {
    for value in ["shared", "per-nit"] {
        let text = MINIMAL.replace("[release]\n", &format!("[release]\nnits = \"{value}\"\n"));
        assert_refused_with(&text, "unknown field `nits`");
    }
}

#[test]
fn the_release_fixer_is_an_optional_instance_key_set_in_one_file_only() {
    let single = MINIMAL.replace(
        "[release]\n",
        "[release]\nfix_assignee = \"fixer-handle\"\n",
    );
    assert_eq!(
        Config::parse(&single)
            .unwrap()
            .release
            .fix_assignee
            .as_deref(),
        Some("fixer-handle")
    );
    let overlay = OVERLAY.replace(
        "[release]\n",
        "[release]\nfix_assignee = \"fixer-handle\"\n",
    );
    let c = merged(NEUTRAL, &overlay).unwrap();
    assert_eq!(c.release.fix_assignee.as_deref(), Some("fixer-handle"));
    let both = NEUTRAL.replace(
        "[[release.angles]]\n",
        "[release]\nfix_assignee = \"another-handle\"\n\n[[release.angles]]\n",
    );
    assert_merge_refused_with(&both, &overlay, "release.fix_assignee: is set in both");
}

#[test]
fn the_tracker_project_is_an_optional_instance_key_set_in_one_file_only() {
    assert_eq!(Config::parse(MINIMAL).unwrap().tracker.project, None);
    let single = MINIMAL.replace(
        "service_identity = \"spl-asdlc\"\n",
        "service_identity = \"spl-asdlc\"\nproject = \"acme\"\n",
    );
    assert_eq!(
        Config::parse(&single).unwrap().tracker.project.as_deref(),
        Some("acme")
    );
    let empty = MINIMAL.replace(
        "service_identity = \"spl-asdlc\"\n",
        "service_identity = \"spl-asdlc\"\nproject = \" \"\n",
    );
    assert_refused_with(&empty, "tracker.project");
    let overlay = OVERLAY.replace(
        "service_identity = \"spl-asdlc\"\n",
        "service_identity = \"spl-asdlc\"\nproject = \"acme\"\n",
    );
    let c = merged(NEUTRAL, &overlay).unwrap();
    assert_eq!(c.tracker.project.as_deref(), Some("acme"));
    let both = NEUTRAL.replace(
        "kind = \"linear\"\n",
        "kind = \"linear\"\nproject = \"acme\"\n",
    );
    assert_eq!(
        merged(&both, OVERLAY).unwrap().tracker.project.as_deref(),
        Some("acme"),
        "in the repository's file alone"
    );
    assert_merge_refused_with(&both, &overlay, "tracker.project: is set in both");
    // A Livela tracker accepts it too.
    let livela = single.replace("kind = \"linear\"", "kind = \"livela\"");
    assert_eq!(
        Config::parse(&livela).unwrap().tracker.project.as_deref(),
        Some("acme")
    );
}

// ---- every project has the release cycle (spec 05, spec 08 rule 11) ----

/// MINIMAL without its `[release]` section.
fn without_release(text: &str) -> String {
    let release = "[release]\ncoordinators = [\"coordinator-login\"]\n\n[[release.angles]]\nkey = \"security\"\n";
    assert!(
        text.contains(release),
        "MINIMAL carries its [release] section"
    );
    text.replace(release, "")
}

#[test]
fn a_file_without_a_release_section_is_refused() {
    assert_refused_with(
        &without_release(MINIMAL),
        "release: the [release] section is required: every project has a release cycle",
    );
    let c = Config::parse(MINIMAL).unwrap();
    assert_eq!(c.release_machinery(), "release/");
    // A section present is judged in full.
    assert_refused_with(
        &MINIMAL.replace("[[release.angles]]\nkey = \"security\"\n", ""),
        "release.angles",
    );
    // `release prepare` refuses it too, writing nothing.
    let errs = Config::preparation(&without_release(MINIMAL)).unwrap_err();
    assert!(
        errs.to_string()
            .contains("the [release] section is required"),
        "{errs}"
    );
}

#[test]
fn an_overlay_never_creates_the_release_section() {
    let neutral = NEUTRAL.replace("[[release.angles]]\nkey = \"security\"\n", "");
    // The overlay's release keys complete nothing: the file is refused for
    // its missing section, named as the repository's.
    assert_merge_refused_with(
        &neutral,
        OVERLAY,
        "config.toml: release: the [release] section is required",
    );
    assert_merge_refused_with(
        &neutral,
        &format!(
            "{OVERLAY}\n[[release.angles]]\nkey = \"security\"\nassignee = \"reviewer-handle\"\n"
        ),
        "the [release] section is required",
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
        "product.repository: is set in both config.toml and config.instance.toml (line 5)",
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
fn the_coordinator_assignees_are_instance_keys_set_in_one_file_only() {
    let overlay = OVERLAY
        .replace(
            "[release]\n",
            "[release]\ncoordinator_assignee = \"coordinator-handle\"\n",
        )
        .replace(
            "[review]\n",
            "[review]\ncoordinator_assignee = \"coordinator-handle\"\n",
        );
    let c = merged(NEUTRAL, &overlay).unwrap();
    assert_eq!(
        c.release.coordinator_assignee.as_deref(),
        Some("coordinator-handle")
    );
    assert_eq!(
        c.review.coordinator_assignee.as_deref(),
        Some("coordinator-handle")
    );
    let single = MINIMAL.replace(
        "[review]\n",
        "[review]\ncoordinator_assignee = \"coordinator-handle\"\n",
    );
    let c = Config::parse(&single).unwrap();
    assert_eq!(
        c.review.coordinator_assignee.as_deref(),
        Some("coordinator-handle")
    );
    assert_eq!(c.release.coordinator_assignee, None, "none by default");
    let both = NEUTRAL.replace(
        "[review]\n",
        "[review]\ncoordinator_assignee = \"another-handle\"\n",
    );
    assert_merge_refused_with(
        &both,
        &overlay,
        "review.coordinator_assignee: is set in both",
    );
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
        "config.instance.toml: product.name: line 6: not an instance key",
    );
    let table = format!("{OVERLAY}\n[bridge]\npoll_minutes = 1\n");
    assert_merge_refused_with(NEUTRAL, &table, "bridge: line 20: not an instance key");
    let angle = format!("{OVERLAY}\n[[review.angles]]\nkey = \"maintainability\"\ntitle = \"T\"\n");
    assert_merge_refused_with(
        NEUTRAL,
        &angle,
        "review.angles[0].title: line 22: not an instance key",
    );
}

#[test]
fn an_overlay_value_of_the_wrong_type_is_refused_with_its_line() {
    let text = OVERLAY.replace("team = \"ABC\"", "team = 5");
    let errs = merged(NEUTRAL, &text).unwrap_err();
    assert!(
        errs[0].starts_with("config.instance.toml: not a valid instance overlay")
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
        "release.angles[0].key: \"design\" is not an angle of config.toml",
    );
    let bare = format!("{OVERLAY}\n[[release.angles]]\nkey = \"security\"\n");
    assert_merge_refused_with(NEUTRAL, &bare, "release.angles[0].assignee: is required");
}

#[test]
fn the_overlay_carries_its_schema_version() {
    assert_merge_refused_with(
        NEUTRAL,
        &OVERLAY.replace("schema = 1", ""),
        "config.instance.toml: schema: is required",
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
        "[release]\ncoordinators = [\"coordinator-login\"]",
        "[release]\ncoordinators = []",
    );
    let errs = merged(&repository, &instance).unwrap_err();
    assert!(
        errs.iter()
            .any(|e| e.starts_with("config.toml: tracker.kind")),
        "{errs:#?}"
    );
    assert!(
        errs.iter()
            .any(|e| e.starts_with("config.instance.toml: release.coordinators")),
        "{errs:#?}"
    );
    let missing = OVERLAY.replace("[owner]\ntracker_user = \"owner-handle\"\n", "");
    assert_merge_refused_with(
        NEUTRAL,
        &missing,
        "config.toml: owner: the [owner] table is required",
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
        "config.instance.toml belongs to config.toml only"
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
