# 5KSA — Only a release writes its changelog, one file per version.

Status: Enforced

## Rule

The release cut writes one file per version in a changelog folder (`release.changelog_dir`, `<dir>/<X.Y.Z>.md`), never an earlier version's, and refuses a version whose file exists. A project may opt in; an app showing release notes assembles those files at build time.

## Evidence

Owner’s decisions of 2026-10-27, ASDLC-NN5K (parallel PRs, phase 1).

## Test

`prepare::tests::a_changelog_folder_gets_one_file_per_version_written_by_its_release_only` (unit test job).
