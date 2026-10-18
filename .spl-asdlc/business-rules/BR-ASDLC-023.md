# BR-ASDLC-023 — spl-asdlc is the one project outside its own method: it releases with release-please and its tickets are Done at merge, so that a bug of the flow never blocks the release that fixes it.

Status: Enforced

## Rule

This repository runs no bridge and no release cycle of its own: it has no `.spl-asdlc/config.toml`, no caller of the release workflows, and its releases are release-please's on `main`. Its tickets keep the method's statuses up to In Review; the session that merges a PR moves its ticket to Done at the merge, with nothing in between, since this repository has no release cycle to follow. Its own process instructions live in its own repository (`CLAUDE.md`, `.spl-asdlc/development.md`), never in the method's pages. Every other project, an agent runtime's included, follows the method and gets its process instructions from it.

## Evidence

The owner's decision of 2026-10-18 (SID-1191), after tickets shipped in v0.2.0 and v1.0.0 sat in To Deploy because nothing closed them: releasing the method with its own bridge makes a loop where a bug of the flow can block the release that fixes it.

## Test

`tests/workflows/own.rs`: `this_repository_has_no_method_configuration`, `this_repositorys_workflows_are_its_callers_and_binaries`, `release_please_runs_on_main_as_the_app_with_this_repositorys_files`
