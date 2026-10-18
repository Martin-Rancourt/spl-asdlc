# BR-ASDLC-027 — ASDLC has one procedure, not flows: configurations differ only by which steps are present, a step absent from the configuration is skipped, and no configuration changes how a present step works.

Status: Enforced

## Rule

The method has one procedure, described on one page, `docs/agents/process.md`. A project's `.spl-asdlc/config.toml` says which optional steps are present (PR review, release QA angles); the release cycle is always present. The agent reads the configuration and skips the steps it leaves out. A configuration with more steps enables more of the procedure; it never changes how a step works. There is no mode, level or flow to choose, and no page of the method speaks of a light, standard or full flow.

## Evidence

The owner's decision of 2026-10-18 (SID-1193): the setup docs spoke of a light, a standard and a full level, read as flows, and agents did not know which flow applied to which project.

## Test

`the_process_page_names_every_optional_step_by_its_configuration_key` and `no_page_of_the_method_speaks_of_flows_or_levels` (`tests/setup_files.rs`)
