# Angle: security

Parts: LLM instructions only; this angle ships no workflow. A project's notes add its own surfaces (where its authentication lives, its secrets, its admin screens); they never replace this text.

## Looks for

- Identity and privilege: who can do what, at every entry point.
- Data and privileged capability: storage, secrets, row-level access or its equivalent, admin surfaces.
- Input and cost: validation, injection, quotas, abuse.
- Ingress and response: headers, redirects, errors that leak.
- Delivery chain: CI, dependencies, tokens, workflows.

## Grading

- `blocker`: an attacker reaches data or a capability they should not, or a required check of the project's checklist ends `fail` or `not_run`.
- `should-fix`: a weakness that needs another flaw to be exploited.
- `nit`: hardening with no attack behind it today.

At a release candidate a `blocker` is blocking and the others are not (`review-release.md`).

## Evidence

A finding names the surface, the attacker, the impact and what would fix it. When the project keeps a checklist for this angle (`checklist-template.md`), each of its checks ends in `pass`, `fail`, `not_run` or `not_applicable`.
