# BR-ASDLC-034 — Adding or retiring a method angle is a normal change of the method; a retired angle stays available to projects pinned on an older version and is reported at their update.

Status: Enforced

## Rule

A new angle or a retirement is a ticket and a PR on spl-asdlc, shipped in a version like any change. A retired angle's file is removed and its key is listed in the `## Retired angles` table of `docs/agents/angles/README.md`, with the version that retired it. A project pinned on an older version keeps the angle's text. Once its pin reaches that version, `spl-asdlc doctor` reports the angle `invalid`, and the angle's items say the method retired it; the configuration stays valid, so the bridge keeps serving the project until it drops the angle or keeps it as its own under another key. Never ignored silently. `docs/reference/08-configuration.md`, "Angles".

## Evidence

The owner's decision in the angle brainstorm of 2026-10-18 (SID-974), implemented by SID-1192.

## Test

`doctor::tests::a_retired_angle_is_reported_once_the_pin_reaches_its_retirement`, `method::texts::protocol::tests::a_retired_angle_still_configured_is_said_in_its_item`, `method::texts::protocol::tests::the_library_lists_every_angle_and_its_retired_table_reads_as_rows`
