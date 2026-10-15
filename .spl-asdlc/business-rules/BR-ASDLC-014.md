# BR-ASDLC-014 — A setting is defined in exactly one place: a key present in both the repository's `.spl-asdlc/config.toml` and the instance overlay is an error, never an override; the overlay carries instance identities only.

Status: Enforced

## Rule

The overlay (`.spl-asdlc/config.instance.toml` on the host that runs the bridge, outside the repository) may carry only the instance keys spec 08 lists (the repository, the owner, the tracker's team and service identity, coordinators and other logins, an angle's assignee, the channels); any other key in it is refused with its line, and an assignee for an angle the repository's file does not declare is refused too. Every rule is checked on the merged result, and each error names the file its value comes from (`docs/reference/08-configuration.md`, "The instance overlay").

## Evidence

The owner's decision of 2026-10-07: this repository then ran its own method and was to be public, so its configuration must carry no instance data (BR-ASDLC-001); a silent override would let a file outside review hide a reviewed setting.

## Test

`config::tests::a_key_in_both_files_is_refused_never_overridden`, `config::tests::an_angle_assignee_and_the_channels_are_set_in_one_file_only`, `config::tests::a_key_that_is_not_an_instance_key_is_refused_with_its_line`, `config::tests::an_assignee_for_an_angle_the_repository_does_not_declare_is_refused`, `config::tests::a_neutral_file_and_its_overlay_read_as_the_single_file`, `config::tests::every_rule_is_checked_on_the_merge_and_names_the_file_of_its_value`, `cli_config_check::a_key_in_both_files_exits_one_and_names_both`
