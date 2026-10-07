//! The conformance suite against a real Linear team, run by hand. It
//! WRITES (creates three items, then cancels them): use a test team.
//!
//! ```sh
//! SPL_ASDLC_TRACKER_TOKEN=… SPL_ASDLC_CONFORMANCE_TEAM=<team key> \
//!   cargo test --test live_linear_conformance -- --ignored --nocapture
//! ```
//!
//! `SPL_ASDLC_CONFORMANCE_STATUSES` may map canonical statuses to the team's
//! names, as `To Deploy=To deploy;Post Deploy=Post deploy`.

use std::collections::BTreeMap;

use spl_asdlc::tracker::linear::{ENV_TOKEN, Linear};
use spl_asdlc::tracker::{StatusTable, conformance};

#[test]
#[ignore = "writes to a real Linear team"]
fn linear_passes_the_conformance_suite() {
    let (Ok(token), Ok(team)) = (
        std::env::var(ENV_TOKEN),
        std::env::var("SPL_ASDLC_CONFORMANCE_TEAM"),
    ) else {
        // Run on purpose (`--ignored`): a missing setting is a failure, never a pass.
        panic!("{ENV_TOKEN} and SPL_ASDLC_CONFORMANCE_TEAM must be set");
    };
    let mut map = BTreeMap::new();
    for pair in std::env::var("SPL_ASDLC_CONFORMANCE_STATUSES")
        .unwrap_or_default()
        .split(';')
    {
        if let Some((k, v)) = pair.split_once('=') {
            map.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    let statuses = StatusTable::from_config(&map);
    let linear = Linear::new(&token, &team, statuses.clone());
    let run_id = time::OffsetDateTime::now_utc().unix_timestamp().to_string();
    match conformance::run(&linear, &statuses, &run_id) {
        Ok(report) => {
            for step in &report.passed {
                println!("ok  {step}");
            }
            println!("created and canceled: {}", report.created.join(", "));
        }
        Err(e) => panic!("{e}"),
    }
}
