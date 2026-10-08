# Acme business rules

Format: one row per rule (`docs/spec/02-business-rules.md`). A rule is a global, permanent invariant of the product. It is `Enforced` only when the test named in its row runs in CI; until then it is `Proposed`, and only `Enforced` rules block. Ids are `BR-ACME-NNN`, where `ACME` is the product prefix (two to six capital letters), and are never reused. A rule born in a ticket is added here in the same PR.

| ID | Rule | Evidence | Test | Status |
|---|---|---|---|---|
