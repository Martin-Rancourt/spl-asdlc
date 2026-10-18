//! The method, as pure functions over facts (`docs/reference/`): no network, no
//! tracker, no clock unless one is passed in. Everything the bridge decides
//! is decided here; the `tick` only reads the facts and applies the answers.

pub mod business_rules;
pub mod checks;
pub mod contract;
pub mod done_gate;
pub mod glob;
pub mod markdown;
pub mod pin;
pub mod release;
pub mod review;
pub mod rule_tests;
pub mod texts;
pub mod ticket_state;
pub mod vocabulary;
pub mod workflow_callers;
