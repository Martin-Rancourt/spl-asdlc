//! SPL-ASDLC, the bridge: one deterministic process that turns GitHub facts
//! into tracker state and tracker state into GitHub actions
//! (`docs/reference/07-the-bridge.md`). It contains no model call.

pub mod action_log;
pub mod cli;
pub mod config;
pub mod doctor;
pub mod dry;
pub mod embed;
pub mod github;
pub mod host;
pub mod http;
pub mod job;
pub mod lock;
pub mod method;
pub mod notify;
pub mod prepare;
pub mod quota;
pub mod setup;
pub mod sync;
pub mod tick;
pub mod tracker;
pub mod verbs;
