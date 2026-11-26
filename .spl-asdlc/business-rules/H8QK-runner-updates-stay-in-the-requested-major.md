# H8QK — Automatic runner updates stay in the host's requested major and never downgrade.

Status: Enforced

## Rule

The runner installs only stable releases of the major explicitly chosen in its host configuration (or passed by its host to `update --major`). It accepts newer patches and minors, but never a version older than or equal to the installed version, even when the user requests an older major. Publishing another major cannot change the selection. Failed verification leaves the installed binary available.

## Evidence

ASDLC-AE9F: fixes and improvements reach standalone and dependent hosts without an implicit breaking upgrade.

## Test

`update::tests::updates_patches_and_minors_without_crossing_the_requested_major`, `update::tests::never_downgrades_even_when_config_requests_an_older_major`, `update::tests::runner_config_requires_an_explicit_major`, `update::tests::checksum_must_name_the_exact_archive_once`, `update::tests::installation_checks_version_and_preserves_a_rollback_binary` (Unix).
