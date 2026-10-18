# BR-ASDLC-011 — The bridge is the sole automated writer of item transitions and writes them regardless of any agent lock; an advisory lock protects an agent's work on the item's content, never its status.

Status: Proposed

## Rule

The bridge neither waits for a lock to be released nor notifies its holder of a transition (`docs/reference/livela-contract.md` §6).

## Evidence

The owner's decision of 2026-10-07 (`.spl-asdlc/open-points.md` #12): Livela locks are advisory and the core's writes do not check them; a transition held back by a lock would stall the method on an agent's session.

## Test

Enforced by review, not by code: the `LivelaStore` slice the adapter is written against has no lock surface, so the adapter (`src/tracker/livela.rs`) cannot read, wait on or answer a lock, and its in-memory test store does not model locks. A test would need the contract to expose locks to the adapter, which this rule says it must not use.
