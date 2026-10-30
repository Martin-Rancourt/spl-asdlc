//! Optional wake hints. No event field is passed to a tick.
use crate::config::{Bridge, SecretRef};
use std::time::Duration;

pub fn valid_url(url: &str) -> bool {
    regex::Regex::new(r"^https://[A-Za-z0-9](?:[A-Za-z0-9.-]*[A-Za-z0-9])?(?::[0-9]{1,5})?$")
        .expect("constant regex")
        .is_match(url)
}

/// Periodic deadlines never move because of webhook traffic.
pub struct Schedule {
    interval: Duration,
    next: Duration,
}
impl Schedule {
    pub fn new(interval: Duration) -> Self {
        Self {
            interval,
            next: Duration::ZERO,
        }
    }
    pub fn due(&mut self, now: Duration, wake: bool) -> bool {
        if now >= self.next {
            self.next = now + self.interval;
            true
        } else {
            wake
        }
    }
    pub fn remaining(&self, now: Duration) -> Duration {
        self.next.saturating_sub(now)
    }
}

fn credentials(
    bridge: &Bridge,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<(String, String), String> {
    let url = bridge
        .queue_url
        .as_deref()
        .filter(|u| valid_url(u))
        .ok_or("invalid queue URL")?;
    let Some(SecretRef::Env(name)) = &bridge.queue_token else {
        return Err("missing queue_token reference".into());
    };
    let token = env(name)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("set {name}"))?;
    Ok((url.to_owned(), token))
}

fn request(
    bridge: &Bridge,
    path: &str,
    timeout: Duration,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<String, String> {
    let (url, token) = credentials(bridge, env)?;
    // Never follow redirects with the queue credential; never report response bodies or URLs.
    let config = ureq::Agent::config_builder()
        .max_redirects(0)
        .timeout_global(Some(timeout.max(Duration::from_millis(1))))
        .build();
    let agent = ureq::Agent::new_with_config(config);
    let mut response = agent
        .get(format!("{url}{path}"))
        .header("Authorization", format!("Bearer {token}"))
        .call()
        .map_err(|_| "queue request failed (check connectivity and credentials)")?;
    if response.status() != 200 {
        return Err("unexpected queue status".into());
    }
    response
        .body_mut()
        .with_config()
        .limit(65536)
        .read_to_string()
        .map_err(|_| "invalid queue response".into())
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Event {
    repository: String,
    kind: String,
}
fn wake(body: &str, repository: &str) -> Result<bool, String> {
    let events: Vec<Event> = serde_json::from_str(body).map_err(|_| "invalid queue events")?;
    if events.len() > 128 {
        return Err("too many queue events".into());
    }
    Ok(events.iter().any(|e| {
        e.repository.eq_ignore_ascii_case(repository) && !e.kind.is_empty() && e.kind.len() <= 64
    }))
}

/// A destructive pull is sufficient: a lost hint is recovered by periodic polling.
pub fn pull(
    bridge: &Bridge,
    repository: &str,
    wait: Duration,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<bool, String> {
    if crate::github::RepoSlug::parse(repository).is_none()
        || repository
            .bytes()
            .any(|b| !b.is_ascii_alphanumeric() && !b"/._-".contains(&b))
    {
        return Err("invalid repository".into());
    }
    let seconds = wait.as_secs().min(25);
    let body = request(
        bridge,
        &format!("/events?repository={repository}&wait={seconds}"),
        wait + Duration::from_secs(2),
        env,
    )?;
    wake(&body, repository)
}

/// Read-only health check; does not consume pending hints.
pub fn health(
    bridge: &Bridge,
    repository: &str,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<(), String> {
    if repository
        .bytes()
        .any(|b| !b.is_ascii_alphanumeric() && !b"/._-".contains(&b))
    {
        return Err("invalid repository".into());
    }
    request(
        bridge,
        &format!("/health?repository={repository}"),
        Duration::from_secs(5),
        env,
    )
    .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_queue_credentials_fail_before_connecting_and_do_not_stop_the_schedule() {
        let mut bridge = crate::config::spec_example::config().bridge;
        bridge.queue_url = Some("https://relay.example.org".into());
        bridge.queue_token = Some(SecretRef::Env("QUEUE_TOKEN".into()));
        assert_eq!(
            pull(&bridge, "example-org/acme", Duration::ZERO, &|_| None).unwrap_err(),
            "set QUEUE_TOKEN"
        );
        let mut schedule = Schedule::new(Duration::from_secs(60));
        assert!(schedule.due(Duration::ZERO, false));
        assert!(!schedule.due(Duration::from_secs(59), false));
        assert!(schedule.due(Duration::from_secs(60), false));
        assert!(schedule.due(Duration::from_secs(180), false));
    }

    #[test]
    fn hints_never_change_periodic_deadlines() {
        let mut s = Schedule::new(Duration::from_secs(60));
        assert!(s.due(Duration::ZERO, false));
        for t in 1..60 {
            assert!(s.due(Duration::from_secs(t), true));
        }
        assert_eq!(s.remaining(Duration::from_secs(59)), Duration::from_secs(1));
        assert!(s.due(Duration::from_secs(60), false));
        assert!(!s.due(Duration::from_secs(61), false));
    }
    #[test]
    fn only_matching_minimal_hints_wake_a_tick() {
        assert!(wake(r#"[{"repository":"ORG/repo","kind":"push"}]"#, "org/repo").unwrap());
        assert!(!wake(r#"[{"repository":"else/repo","kind":"push"}]"#, "org/repo").unwrap());
        assert!(!wake("[]", "org/repo").unwrap());
        assert!(
            wake(
                r#"[{"repository":"org/repo","kind":"push","merge":true}]"#,
                "org/repo"
            )
            .is_err()
        );
        assert!(wake("not json", "org/repo").is_err());
    }
    #[test]
    fn credentials_cannot_be_sent_to_an_embedded_user_or_path() {
        for u in [
            "http://example.org",
            "https://user@localhost",
            "https://example.org/path",
            "https://example.org?q=x",
            "https://example.org#x",
        ] {
            assert!(!valid_url(u));
        }
        assert!(valid_url("https://relay.example.org"));
    }
}
