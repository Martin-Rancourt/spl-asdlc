# BR-ASDLC-010 — The public `spl-asdlc` crate depends on no consumer (no `livela-core`): each runtime/tracker writes its own wrapper on its side, depending on `spl-asdlc` by version. The dependency arrow always points toward ASDLC, never outward.

Status: Enforced

## Rule

The Livela runner's `BridgeJob` and its store glue live in the runner's code; this crate offers the pass, the adapter and the `LivelaStore` slice they need (`docs/reference/livela-contract.md` §3). No dependency of the manifest, `[patch]` or `[replace]` included, is a Livela crate or a Git source, and `Cargo.lock` holds neither.

## Evidence

The owner's decision of 2026-10-07 (`.spl-asdlc/open-points.md` #11): the Livela repository is private and unlicensed, a Git dependency would name an account in `Cargo.lock` (BR-ASDLC-001), and a second copy of the core would give `BridgeJob` a trait the runner does not know.

## Test

`tests/dependency_arrow.rs`: `the_manifest_depends_on_no_consumer_and_on_no_git_source`, `the_lockfile_holds_no_consumer_and_no_git_source`, `a_consumer_is_recognised_however_it_is_spelled`
