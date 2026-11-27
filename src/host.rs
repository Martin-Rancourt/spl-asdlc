//! What a host gives the bridge: its configuration and its secrets, read
//! from the environment (`docs/setup/bridge.md`). This module turns them
//! into the clients the tick uses; it decides nothing.

use crate::config::{Config, GithubIdentity, TrackerKind};
use crate::github::RestGitHub;
use crate::github::auth::Credentials;
use crate::tracker::linear::{ENV_TOKEN, Linear};
use crate::tracker::{StatusTable, Tracker};

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

/// The App's client, its credentials read from `env`, for a host that
/// reads a project's configuration through it before it has one.
pub fn app_github(env: &dyn Fn(&str) -> Option<String>) -> Result<RestGitHub, String> {
    Ok(RestGitHub::new(Credentials::app_from_env(env)?))
}

/// The tracker client of a configuration: Linear over the network, Livela
/// through the Livela runner's local socket (`[tracker] socket`,
/// `docs/reference/livela-contract.md`, "Socket protocol"). The program
/// never opens the Livela store itself.
pub fn tracker(
    config: &Config,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<Box<dyn Tracker>, String> {
    match config.tracker.kind {
        TrackerKind::Linear => Ok(Box::new(linear(config, env)?)),
        TrackerKind::Livela => livela(config),
    }
}

#[cfg(unix)]
fn livela(config: &Config) -> Result<Box<dyn Tracker>, String> {
    use crate::tracker::livela::Livela;
    use crate::tracker::livela::socket::SocketStore;
    let socket = config.tracker.socket.as_deref().ok_or(NO_SOCKET)?;
    let store = SocketStore::connect(std::path::Path::new(socket))?;
    // The Livela project is `[tracker] project`; its prefix is the team key.
    let project = config
        .tracker
        .project
        .as_deref()
        .unwrap_or(&config.tracker.team);
    Ok(Box::new(Livela::new(
        store,
        project,
        StatusTable::identity(),
    )))
}

#[cfg(not(unix))]
fn livela(_config: &Config) -> Result<Box<dyn Tracker>, String> {
    Err(
        "a Livela tracker is reached through the Livela runner's Unix socket: this host has none"
            .to_string(),
    )
}

/// The refusal of a Livela tracker with no socket: it names the key to add.
pub const NO_SOCKET: &str = "a Livela tracker is reached through the Livela runner's local socket: add `socket = \"<path of the runner's socket>\"` under [tracker] (in the instance overlay, a host key)";

/// The Linear client of a configuration; an error for another tracker.
pub fn linear(config: &Config, env: &dyn Fn(&str) -> Option<String>) -> Result<Linear, String> {
    match config.tracker.kind {
        TrackerKind::Linear => {
            let token = env(ENV_TOKEN)
                .filter(|t| !t.trim().is_empty())
                .ok_or_else(|| format!("{ENV_TOKEN} is not set on this host"))?;
            Ok(
                Linear::new(&token, &config.tracker.team, StatusTable::identity())
                    .with_project(config.tracker.project.as_deref()),
            )
        }
        TrackerKind::Livela => {
            Err("a Livela tracker is not a Linear one: open it with `host::tracker`".to_string())
        }
    }
}
