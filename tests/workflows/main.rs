//! The workflow shape tests (`docs/requirements/ci.md`, REQ-CI-012): the
//! YAML this repository ships is parsed, the repository lints run on it, and
//! the scripts of its steps are executed as shipped (never a copy), with the
//! untrusted values in the environment.
//!
//! The lints run everywhere; the executions need `bash`, `git` and `jq`, so
//! they run on Unix only.

mod harness;
mod lints;
#[cfg(unix)]
mod pr_guards;
#[cfg(unix)]
mod pr_title;
#[cfg(unix)]
mod release_cut;
#[cfg(unix)]
mod release_gate;
#[cfg(unix)]
mod release_publish;
#[cfg(unix)]
mod required_check;
