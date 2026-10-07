//! SPL-ASDLC, the bridge: one deterministic process that turns GitHub facts
//! into tracker state and tracker state into GitHub actions
//! (`docs/spec/07-the-bridge.md`). It contains no model call.

pub mod cli;
pub mod config;
pub mod github;
pub mod http;
pub mod method;
pub mod notify;
pub mod tracker;
