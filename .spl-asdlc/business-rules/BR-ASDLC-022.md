# BR-ASDLC-022 — A release QA finding is blocking or not: a blocking one becomes an urgent fix ticket assigned to the release fixer and blocks the GO until fixed in a later candidate; a non-blocking one becomes a ticket logged for later.

Status: Proposed

## Rule

An angle reviews the candidate from its angle, classifies each finding as blocking or non-blocking and documents it (what was observed, where, in what context), and fixes nothing in its branch: its angle PR carries its mandatory report only (`.spl-asdlc/qa/<candidate>/<angle>.json` by default, `item` its sub-item's key). From a merged report, the bridge files each blocking finding as an urgent fix ticket assigned to `release.fix_assignee`, the one key the gate-fix tickets also use (BR-ASDLC-016; unassigned, taken by role, when the key is unset), which blocks the release issue: while it is open there is no GO, and its fix merged to the release branch makes the next candidate. Each non-blocking finding becomes an ordinary ticket, logged for later. The coordinator confirms or rejects each finding and may change whether it is blocking, with a reason. At a PR review nothing changes: the coordinator applies the fixes it retains at the PR.

## Evidence

The owner's decision on QA findings (2026-10-14), replacing the three-severity model of the release QA (`blocker`, `should-fix`, `nit`), the fixes made in angle branches, the shared nits ticket and `release.nits`.

## Test

None yet: the code and its tests still follow the three-severity model (BR-ASDLC-012, BR-ASDLC-013, BR-ASDLC-016, BR-ASDLC-020); `docs/spec/` is aligned later.
