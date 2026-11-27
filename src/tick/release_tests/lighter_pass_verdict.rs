//! A later pass under `later_passes = "gate-and-coordinator"` that frees a
//! blocking finding (ASDLC-DF2Q): the candidate offers no angle, but its GO
//! waits for the coordinator's verdict on it, so that a person judges that
//! the merged fix fixes the finding. A later pass that frees none still
//! needs no verdict.

use super::*;

/// The blocking finding of the release, seeded by these tests.
const FINDING: &str = "ABC-90";

fn lighter() -> Config {
    release_config(
        "later_passes = \"full\"",
        "later_passes = \"gate-and-coordinator\"",
    )
}

impl World {
    /// rc.1 reviewed in full, its angles and its verdict merged, then rc.2
    /// cut on `c2` with a green gate, under the lighter policy; the blocking
    /// finding `FINDING` of the release at To Deploy, its ticked boxes and
    /// its fix PR #40, recorded by the bridge, merged by `fix40` into the
    /// release branch, and `fix40` in rc.2.
    fn lighter_pass_freeing_a_finding() -> World {
        let mut w = World::new();
        w.config = lighter();
        w.tick();
        all_angles_merged(&w);
        w.cut_rc2();
        w.gate(vec![
            gate_run(2, CAND2, "c2", Some("success")),
            gate_run(1, CAND, "c1", Some("success")),
        ]);
        let release = w.release_item();
        let description = format!(
            "## Goal\nFix.\n\n## Acceptance criteria\n- [x] fixed\n\n## Business rules\nNo new rule.\n\n{}\n",
            crate::method::release::findings::blocker_marker(&release.key)
        );
        w.tracker
            .seed(FINDING, "Blocker", &description, Status::ToDeploy, &[LABEL]);
        w.tracker
            .relate(FINDING, &release.key, RelationKind::Blocks)
            .unwrap();
        let url = "https://github.com/example-org/acme/pull/40";
        w.tracker.link(FINDING, url);
        w.tracker
            .comment(
                FINDING,
                &texts::merged_comment(
                    url,
                    BRANCH,
                    &texts::MergedBy::Outside("someone"),
                    Status::ToDeploy.name(),
                    "-",
                ),
            )
            .unwrap();
        let mut pr = pull_request(40);
        pr.base_ref = BRANCH.into();
        pr.head_ref = "docs/abc-90-the-finding".into();
        pr.state = PrState::Merged;
        pr.merge_commit_sha = Some("fix40".into());
        {
            let mut s = w.github.state.borrow_mut();
            if let Some(Ok(merged)) = s.merged_pulls.get_mut(&repo()) {
                merged.push(pr);
            }
            s.compares.insert(
                (repo(), "fix40".into(), "c2".into()),
                cmp(CompareStatus::Ahead, &[], &[]),
            );
        }
        w
    }

    /// The consolidation item of rc.2.
    fn consolidation_of_rc2(&self) -> Option<crate::tracker::Item> {
        let release = self.release_item();
        self.tracker
            .children(&release.key)
            .unwrap()
            .into_iter()
            .find(|c| {
                texts::bridge_item(&c.description, c.parent.as_deref())
                    == Some(BridgeItem::Consolidation {
                        scope: CAND2.into(),
                    })
            })
    }

    /// The coordinator's verdict `go` on rc.2, merged.
    fn rc2_verdict_merged(&self) {
        let cons = self
            .consolidation_of_rc2()
            .expect("rc.2's consolidation item");
        let verdict = format!(
            r#"{{"schema":"spl-asdlc/qa-verdict/1","candidate":"{CAND2}","item":"{}","verdict":"go",
               "between_angles":"The fix of {FINDING} read against its finding.","findings":[]}}"#,
            cons.key
        );
        let mut pr = green(pull_request(33));
        pr.base_ref = BRANCH.into();
        pr.head_ref = format!("qa/{CAND2}/consolidation");
        pr.body = format!("Verdict, coordination item {}", cons.key);
        pr.author = Some(Actor {
            login: "coordinator-login".into(),
            is_bot: false,
        });
        pr.state = PrState::Merged;
        pr.merge_commit_sha = Some("merge-of-33".into());
        let path = format!(".spl-asdlc/qa/{CAND2}/consolidation.json");
        let mut s = self.github.state.borrow_mut();
        s.files
            .insert((repo(), "merge-of-33".into(), path), Ok(verdict));
        if let Some(Ok(merged)) = s.merged_pulls.get_mut(&repo()) {
            merged.push(pr);
        }
    }
}

#[test]
fn a_lighter_later_pass_that_frees_a_blocking_finding_waits_for_a_coordinator_verdict_on_it() {
    // release_tests: under later_passes = gate-and-coordinator, a blocking
    // finding at To Deploy whose merged PR is in rc.2 does not let rc.2
    // reach a GO without a coordinator verdict on rc.2.
    let w = World::lighter_pass_freeing_a_finding();
    let (_, d) = crate::verbs::status(&w.tick_ctx()).unwrap();
    assert!(!d.go, "{d:?}");
    assert!(d.blockers.is_empty() && d.fix_gaps.is_empty(), "{d:?}");
    assert_eq!(
        d.reasons,
        vec!["the coordinator's verdict on the candidate is not merged"]
    );
    let r = w.tick();
    assert!(w.go_writes().is_empty(), "{r:?}");

    // No angle on rc.2, but its consolidation item, assigned to the
    // coordinator's verdict, says which finding it judges, once.
    let round = w.round(CAND2);
    assert_eq!(round.len(), 1, "the consolidation item alone: {round:?}");
    let cons = w.consolidation_of_rc2().expect("rc.2's consolidation item");
    let told = |w: &World| {
        w.tracker
            .comments(&cons.key)
            .unwrap()
            .into_iter()
            .filter(|c| {
                c.by_bridge && texts::has_marker(&c.body, &texts::lighter_verdict_marker(CAND2))
            })
            .map(|c| c.body)
            .collect::<Vec<_>>()
    };
    let said = told(&w);
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].contains(FINDING), "{}", said[0]);
    w.tick();
    assert_eq!(told(&w).len(), 1, "said once");
    assert!(w.go_writes().is_empty());

    // The coordinator's verdict on rc.2 merged: the GO.
    w.rc2_verdict_merged();
    let (_, d) = crate::verbs::status(&w.tick_ctx()).unwrap();
    assert!(d.go, "{:?}", d.reasons);
    let r = w.tick();
    assert_eq!(w.go_writes(), go_lines("c2"), "{r:?}");
}

#[test]
fn a_lighter_later_pass_whose_finding_is_held_by_a_hold_verdict_reaches_no_go() {
    let w = World::lighter_pass_freeing_a_finding();
    w.tick();
    let cons = w.consolidation_of_rc2().expect("rc.2's consolidation item");
    let verdict = format!(
        r#"{{"schema":"spl-asdlc/qa-verdict/1","candidate":"{CAND2}","item":"{}","verdict":"hold",
           "between_angles":"The fix does not fix the finding.","findings":[]}}"#,
        cons.key
    );
    let mut pr = green(pull_request(33));
    pr.base_ref = BRANCH.into();
    pr.head_ref = format!("qa/{CAND2}/consolidation");
    pr.author = Some(Actor {
        login: "coordinator-login".into(),
        is_bot: false,
    });
    pr.state = PrState::Merged;
    pr.merge_commit_sha = Some("merge-of-33".into());
    {
        let mut s = w.github.state.borrow_mut();
        s.files.insert(
            (
                repo(),
                "merge-of-33".into(),
                format!(".spl-asdlc/qa/{CAND2}/consolidation.json"),
            ),
            Ok(verdict),
        );
        if let Some(Ok(merged)) = s.merged_pulls.get_mut(&repo()) {
            merged.push(pr);
        }
    }
    let r = w.tick();
    assert!(w.go_writes().is_empty(), "{r:?}");
    let (_, d) = crate::verbs::status(&w.tick_ctx()).unwrap();
    assert!(!d.go);
}

#[test]
fn a_lighter_later_pass_that_frees_no_finding_still_asks_no_verdict() {
    let mut w = World::new();
    w.config = lighter();
    w.tick();
    all_angles_merged(&w);
    w.cut_rc2();
    w.gate(vec![
        gate_run(2, CAND2, "c2", Some("success")),
        gate_run(1, CAND, "c1", Some("success")),
    ]);
    let r = w.tick();
    assert!(w.consolidation_of_rc2().is_none());
    assert_eq!(w.go_writes(), go_lines("c2"), "{r:?}");
}
