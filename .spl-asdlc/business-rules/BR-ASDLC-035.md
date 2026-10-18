# BR-ASDLC-035 — Everything the bridge writes is in English, with no message catalogue.

Status: Proposed

## Rule

The texts the bridge writes, in the tracker (items, descriptions, comments), on GitHub (PR comments, reviews, release notes) and in its notices, are in English. There is no message catalogue and no language setting: a project that needs another language reads them as they are, or translates them on its side. The texts a project writes itself (its tickets, its notes, its `AGENTS.md`) are its own.

## Evidence

Plan §3.11 (2026-10-05): tracker messages and notifications are in English, with no catalogue. Recorded after the spec review of 2026-10-18 (SID-1195, gap 5), with the owner's agreement.

## Test

None yet: the rule holds by review of `src/method/texts.rs` and the notices, which carry no locale.
