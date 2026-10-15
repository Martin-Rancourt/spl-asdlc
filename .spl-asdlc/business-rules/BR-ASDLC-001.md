# BR-ASDLC-001 — The repository carries no instance data.

Status: Enforced

## Rule

No name of a product, agent, person, organisation member or installation that uses the method appears anywhere in the repository (code, docs, tests, workflows, fixtures), and no identifier of an installation does either: e-mail addresses, `/home/<user>` or `/Users/<user>` paths, 9+-digit account or chat ids, hexadecimal uids, GitHub App bot logins. Allowed: product values that hold for every install (this product's repository slug and crate name), and the names of the integrations the method supports as adapters or hosts (trackers such as Linear, Jira or Livela; runtimes such as AgentForge, Claude Code or Codex; hosts such as GitHub). The line is: an integration is named, an installation is not.

## Evidence

The method must be reusable by anyone, and publishable, without a scrub.

## Test

`scripts/check-no-instance-names.sh`: scans every tracked file for the identifier forms above, and for the words listed in the environment variable `SPL_ASDLC_FORBIDDEN_WORDS` (comma-separated; kept outside the repository, in a CI secret or a local untracked file, precisely so that the list itself never enters the repository). The product's own slug, read from `GITHUB_REPOSITORY` or the `origin` remote's GitHub URL, is removed from a hit line before the word is looked for again: the allowed slug passes, the owner name on its own does not (`tests/rule_zero.rs`). It runs in CI on every pull request (`.github/workflows/pr.yml`, job `rule-zero`). Zero hits, no baseline, no exemption list.
