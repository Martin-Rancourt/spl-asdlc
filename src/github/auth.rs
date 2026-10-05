//! Who the bridge writes as on GitHub: a GitHub App (installation tokens
//! minted from the App's private key) or, where an App is not allowed, a
//! personal token (`docs/setup/github-app.md`).

use std::collections::HashMap;
use std::sync::Mutex;

use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde::Serialize;
use time::{Duration, OffsetDateTime};

use super::types::RepoSlug;

/// Environment variables the bridge reads its GitHub credentials from.
pub const ENV_APP_ID: &str = "SPL_ASDLC_APP_ID";
pub const ENV_APP_PRIVATE_KEY_PATH: &str = "SPL_ASDLC_APP_PRIVATE_KEY_PATH";
pub const ENV_APP_PRIVATE_KEY: &str = "SPL_ASDLC_APP_PRIVATE_KEY";
pub const ENV_TOKEN: &str = "SPL_ASDLC_GITHUB_TOKEN";

#[derive(Clone)]
pub enum Credentials {
    App {
        app_id: String,
        private_key_pem: Vec<u8>,
    },
    Token(String),
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Credentials::App { app_id, .. } => {
                write!(f, "App {{ app_id: {app_id}, private key: <redacted> }}")
            }
            Credentials::Token(_) => write!(f, "Token(<redacted>)"),
        }
    }
}

impl Credentials {
    /// Read the App credentials from the environment, naming what is missing.
    pub fn app_from_env(env: &dyn Fn(&str) -> Option<String>) -> Result<Credentials, String> {
        let app_id = env(ENV_APP_ID)
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| format!("{ENV_APP_ID} is not set"))?;
        let pem = match (env(ENV_APP_PRIVATE_KEY), env(ENV_APP_PRIVATE_KEY_PATH)) {
            (Some(pem), _) if !pem.trim().is_empty() => pem.into_bytes(),
            (_, Some(path)) if !path.trim().is_empty() => {
                std::fs::read(path.trim()).map_err(|e| {
                    format!("{ENV_APP_PRIVATE_KEY_PATH} ({path}) could not be read: {e}")
                })?
            }
            _ => {
                return Err(format!(
                    "neither {ENV_APP_PRIVATE_KEY_PATH} nor {ENV_APP_PRIVATE_KEY} is set"
                ));
            }
        };
        EncodingKey::from_rsa_pem(&pem)
            .map_err(|e| format!("the App private key is not an RSA PEM key: {e}"))?;
        Ok(Credentials::App {
            app_id: app_id.trim().to_string(),
            private_key_pem: pem,
        })
    }

    pub fn token_from_env(env: &dyn Fn(&str) -> Option<String>) -> Result<Credentials, String> {
        env(ENV_TOKEN)
            .filter(|s| !s.trim().is_empty())
            .map(|t| Credentials::Token(t.trim().to_string()))
            .ok_or_else(|| format!("{ENV_TOKEN} is not set"))
    }
}

#[derive(Serialize)]
struct Claims {
    iat: i64,
    exp: i64,
    iss: String,
}

/// A JWT that authenticates as the App itself (valid ten minutes, backdated
/// one minute against clock drift, as GitHub recommends).
pub fn app_jwt(
    app_id: &str,
    private_key_pem: &[u8],
    now: OffsetDateTime,
) -> Result<String, String> {
    let key = EncodingKey::from_rsa_pem(private_key_pem)
        .map_err(|e| format!("the App private key is not an RSA PEM key: {e}"))?;
    let claims = Claims {
        iat: (now - Duration::seconds(60)).unix_timestamp(),
        exp: (now + Duration::seconds(540)).unix_timestamp(),
        iss: app_id.to_string(),
    };
    jsonwebtoken::encode(&Header::new(Algorithm::RS256), &claims, &key)
        .map_err(|e| format!("the App JWT could not be signed: {e}"))
}

/// Installation tokens per repository, reused until five minutes before
/// they expire.
#[derive(Default)]
pub(super) struct TokenCache {
    tokens: Mutex<HashMap<RepoSlug, (String, OffsetDateTime)>>,
}

impl TokenCache {
    pub fn get(&self, repo: &RepoSlug, now: OffsetDateTime) -> Option<String> {
        let tokens = self.tokens.lock().ok()?;
        let (token, expires) = tokens.get(repo)?;
        (*expires - Duration::minutes(5) > now).then(|| token.clone())
    }

    pub fn put(&self, repo: &RepoSlug, token: String, expires: OffsetDateTime) {
        if let Ok(mut tokens) = self.tokens.lock() {
            tokens.insert(repo.clone(), (token, expires));
        }
    }
}
