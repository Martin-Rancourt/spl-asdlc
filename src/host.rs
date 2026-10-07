//! What a host gives the bridge: its configuration and its secrets, read
//! from the environment (`docs/setup/bridge.md`). This module turns them
//! into the clients the tick uses; it decides nothing.

use crate::config::{Config, GithubIdentity, TrackerKind};
use crate::github::RestGitHub;
use crate::github::auth::Credentials;
use crate::tracker::StatusTable;
use crate::tracker::linear::{ENV_TOKEN, Linear};

/// The process environment, as the clients read it.
pub fn process_env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

/// The GitHub client the configuration asks for: the App (the default) or
/// the personal-token fallback.
pub fn github(config: &Config, env: &dyn Fn(&str) -> Option<String>) -> Result<RestGitHub, String> {
    let credentials = match config.bridge.github_identity {
        GithubIdentity::App => Credentials::app_from_env(env)?,
        GithubIdentity::Token => Credentials::token_from_env(env)?,
    };
    Ok(RestGitHub::new(credentials))
}

/// The tracker client of a configuration, for the trackers reached over
/// the network. Livela is reached in process, from its runner's store
/// (`docs/setup/tracker-livela.md`): the binary alone cannot open it.
pub fn linear(config: &Config, env: &dyn Fn(&str) -> Option<String>) -> Result<Linear, String> {
    match config.tracker.kind {
        TrackerKind::Linear => {
            let token = env(ENV_TOKEN)
                .filter(|t| !t.trim().is_empty())
                .ok_or_else(|| format!("{ENV_TOKEN} is not set on this host"))?;
            Ok(Linear::new(
                &token,
                &config.tracker.team,
                StatusTable::from_config(&config.tracker.statuses),
            ))
        }
        TrackerKind::Livela => Err(
            "a Livela tracker is reached from the Livela runner, where the bridge runs as a daemon job; the binary cannot open it"
                .to_string(),
        ),
    }
}
