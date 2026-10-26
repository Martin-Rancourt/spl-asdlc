//! The pace of a pass under GitHub's hourly quotas (ASDLC-5DDN). An App
//! has one installation for every repository it serves, so its passes share
//! one REST quota and one GraphQL quota. A pass reads them first
//! (`/rate_limit` costs none of them), says what is left in its line, and
//! spends less when little is left: what can wait is read at a later pass,
//! after the reset, rather than failing on "rate limit exceeded".

use crate::github::{GitHub, Quota, RepoSlug};

/// Under this share of a quota (in percent), the passes skip what can wait:
/// the GitHub sync, and the bridge's sweep of merged and closed PRs.
const LOW_PERCENT: u64 = 20;
/// Under this share, a bridge pass reads nothing until the reset.
const EXHAUSTED_PERCENT: u64 = 5;

/// How much a pass may read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pace {
    Normal,
    /// Only what cannot wait: the merge pass and the release cycle.
    Low,
    /// Nothing.
    Exhausted,
}

/// The quotas as a pass found them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reading {
    quotas: Vec<Quota>,
    /// Why they could not be read: the pass goes at its normal pace.
    error: Option<String>,
}

pub fn read(github: &dyn GitHub, repo: &RepoSlug) -> Reading {
    match github.quota(repo) {
        Ok(quotas) => Reading {
            quotas,
            error: None,
        },
        Err(e) => Reading {
            quotas: Vec::new(),
            error: Some(e.to_string()),
        },
    }
}

impl Reading {
    /// The pace the lowest quota allows.
    pub fn pace(&self) -> Pace {
        let under = |q: &Quota, percent: u64| q.remaining * 100 < q.limit * percent;
        if self.quotas.iter().any(|q| under(q, EXHAUSTED_PERCENT)) {
            Pace::Exhausted
        } else if self.quotas.iter().any(|q| under(q, LOW_PERCENT)) {
            Pace::Low
        } else {
            Pace::Normal
        }
    }

    /// `GitHub quota: core 4210/5000 until 13:45Z, graphql 120/5000 until
    /// 13:22Z`, for the pass's line; `None` when there is nothing to say.
    pub fn line(&self) -> Option<String> {
        if let Some(e) = &self.error {
            return Some(format!("GitHub quota not read: {e}"));
        }
        if self.quotas.is_empty() {
            return None;
        }
        let parts: Vec<String> = self
            .quotas
            .iter()
            .map(|q| {
                format!(
                    "{} {}/{} until {:02}:{:02}Z",
                    q.resource,
                    q.remaining,
                    q.limit,
                    q.reset.0.hour(),
                    q.reset.0.minute()
                )
            })
            .collect();
        Some(format!("GitHub quota: {}", parts.join(", ")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::Timestamp;
    use crate::github::fake::FakeGitHub;

    fn quota(resource: &str, remaining: u64) -> Quota {
        Quota {
            resource: resource.into(),
            limit: 5000,
            remaining,
            reset: Timestamp::parse("2026-10-26T13:22:00Z").unwrap(),
        }
    }

    fn reading(quotas: Vec<Quota>) -> Reading {
        let g = FakeGitHub::new();
        g.state.borrow_mut().quota = Some(Ok(quotas));
        read(&g, &RepoSlug::parse("example-org/acme").unwrap())
    }

    #[test]
    fn the_lowest_quota_sets_the_pace() {
        assert_eq!(
            reading(vec![quota("core", 5000), quota("graphql", 1000)]).pace(),
            Pace::Normal
        );
        assert_eq!(
            reading(vec![quota("core", 999), quota("graphql", 5000)]).pace(),
            Pace::Low
        );
        assert_eq!(
            reading(vec![quota("core", 4000), quota("graphql", 249)]).pace(),
            Pace::Exhausted
        );
        assert_eq!(reading(vec![quota("graphql", 0)]).pace(), Pace::Exhausted);
    }

    #[test]
    fn the_line_says_what_is_left_and_until_when() {
        assert_eq!(
            reading(vec![quota("core", 4210), quota("graphql", 120)])
                .line()
                .unwrap(),
            "GitHub quota: core 4210/5000 until 13:22Z, graphql 120/5000 until 13:22Z"
        );
        assert_eq!(reading(Vec::new()).line(), None);
    }

    #[test]
    fn an_unreadable_quota_keeps_the_normal_pace_and_says_so() {
        let g = FakeGitHub::new();
        g.state.borrow_mut().quota = Some(Err(crate::github::ReadError::unreadable(
            "the GitHub quota",
            "HTTP 502",
        )));
        let r = read(&g, &RepoSlug::parse("example-org/acme").unwrap());
        assert_eq!(r.pace(), Pace::Normal);
        assert!(r.line().unwrap().starts_with("GitHub quota not read: "));
    }
}
