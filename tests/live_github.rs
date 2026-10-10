//! Read-only checks against the real GitHub, run by hand:
//!
//! ```sh
//! SPL_ASDLC_LIVE_REPO=<owner>/<repo> SPL_ASDLC_APP_ID=… SPL_ASDLC_APP_PRIVATE_KEY_PATH=… \
//!   cargo test --test live_github -- --ignored --nocapture
//! ```
//!
//! (`SPL_ASDLC_GITHUB_TOKEN` instead of the App variables uses a personal
//! token.) Nothing is written.

use spl_asdlc::github::auth::Credentials;
use spl_asdlc::github::{GitHub, RepoSlug, RestGitHub};

fn client() -> Option<(RestGitHub, RepoSlug)> {
    let repo = RepoSlug::parse(&std::env::var("SPL_ASDLC_LIVE_REPO").ok()?)?;
    let env = |k: &str| std::env::var(k).ok();
    let credentials = Credentials::app_from_env(&env)
        .or_else(|_| Credentials::token_from_env(&env))
        .expect("App or token credentials in the environment");
    Some((RestGitHub::new(credentials), repo))
}

#[test]
#[ignore = "needs the network and credentials"]
fn reads_a_real_repository() {
    let Some((gh, repo)) = client() else {
        eprintln!("SPL_ASDLC_LIVE_REPO is not set: nothing to do");
        return;
    };
    println!("identity: {}", gh.identity().expect("identity"));
    let default = gh.default_branch(&repo).expect("default branch");
    println!("default branch: {default}");
    let head = gh.commit(&repo, &default).expect("head commit");
    println!("head: {} {}", &head.sha[..12], head.subject());
    println!(
        "required checks of {default}: {:?}",
        gh.required_checks(&repo, &default)
    );
    println!("rulesets: {:?}", gh.rulesets(&repo));
    println!(
        "release branches: {:?}",
        gh.branches(&repo, "release/").map(|b| b.len())
    );
    let tags = gh.tags(&repo, "").expect("tags");
    println!("tags: {}", tags.len());
    if let Some(t) = tags
        .iter()
        .rev()
        .find(|t| t.object_type == spl_asdlc::github::ObjectType::Tag)
    {
        let object = gh.tag_object(&repo, &t.sha).expect("tag object");
        println!(
            "annotated tag {}: {:?}",
            object.name,
            object.message.lines().next()
        );
    }
    let pulls = gh.open_pulls(&repo, None).expect("open PRs");
    for p in &pulls {
        println!(
            "#{} {} [{}] base {} head {} checks {} reviews {} threads {} truncated {:?}",
            p.number,
            p.title,
            p.merge_state,
            p.base_ref,
            p.head_ref,
            p.checks.len(),
            p.reviews.len(),
            p.threads.len(),
            p.truncated
        );
    }
    let week =
        spl_asdlc::github::Timestamp(time::OffsetDateTime::now_utc() - time::Duration::days(7));
    let merged = gh.merged_pulls(&repo, None, Some(week), 200);
    println!("merged PRs read: {:?}", merged.as_ref().map(|m| m.len()));
    if let Some(p) = pulls.first() {
        println!(
            "files of #{}: {:?}",
            p.number,
            gh.pull_files(&repo, p.number).map(|f| f.len())
        );
        println!(
            "comments of #{}: {:?}",
            p.number,
            gh.issue_comments(&repo, p.number).map(|c| c.len())
        );
    }
    println!(
        "release-please file: {:?}",
        gh.file(&repo, "release-please-config.json", &default)
            .map(|t| t.len())
    );
}
