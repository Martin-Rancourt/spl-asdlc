# Working in spl-asdlc

This repository runs the SPL-ASDLC method on itself, at a published, pinned version (BR-ASDLC-023). Follow `.spl-asdlc/process.md` for every ticket, branch, PR, review and release: it wins over this file. Then read `.spl-asdlc/development.md`: how this repository is built, released and tested, how the bridge that serves it is updated, and the manual fallback.

- Repository text is in English.
- Build, lint and unit tests: `cargo build --locked`, `cargo fmt --all --check`, `cargo clippy --all-targets --locked -- -D warnings`, `cargo test --locked`.
- Never call a method unit from this commit (`./.github/workflows/asdlc-*`, `./actions/*`) in this repository's own workflows: they name the pinned line (`tests/workflows/own.rs`).
- Never edit `.spl-asdlc/process.md`; `docs/agents/process.md` is its source, for the next release.
- Rule zero: no name of a person, product or installation that uses the method, no e-mail, home path or identifier in the repository (`scripts/check-no-instance-names.sh`, BR-ASDLC-001).
- The method itself is `docs/method.md`; the agents' protocols are `docs/agents/`.
