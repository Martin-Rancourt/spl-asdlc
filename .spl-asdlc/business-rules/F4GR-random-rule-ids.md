# F4GR — Rules have independent files with random short ids and no index.

Status: Enforced

## Rule

Each new rule has a random four-character id (capital letters and the digits 2 to 9, at least one letter and one digit, so no word in capitals reads as an id) and a short filename slug in .spl-asdlc/business-rules/. References use the id alone. Never allocate sequential ids or maintain a committed index. Legacy ids remain readable while projects migrate: a renamed rule keeps its old id as an alias at the start of its slug (`K7QF-br-acme-073-signed-exports.md` registers `K7QF` and `BR-ACME-073`), so tickets citing the old id still find it.

## Evidence

Owner’s decisions of 2026-10-27, ASDLC-NN5K (parallel PRs, phase 1).

## Test

`method::business_rules::tests::random_four_character_rule_ids_have_a_slug_and_are_referenced_by_id_alone`, `method::business_rules::tests::a_word_in_capitals_or_a_ticket_id_is_never_read_as_a_random_rule_id`, `method::business_rules::tests::a_renamed_rule_keeps_its_legacy_id_as_an_alias_in_its_file_name` (unit test job).
