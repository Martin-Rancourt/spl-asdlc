//! The conformance suite against a real Linear team, run by hand. It
//! WRITES (creates three items, then cancels them): use a test team.
//!
//! ```sh
//! SPL_ASDLC_TRACKER_TOKEN=… SPL_ASDLC_CONFORMANCE_TEAM=<team key> \
//!   cargo test --test live_linear_conformance -- --ignored --nocapture
//! ```
//!
//! The team names its statuses exactly as the canonical ones.

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
    let statuses = StatusTable::identity();
    let linear = Linear::new(&token, &team, statuses);
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
