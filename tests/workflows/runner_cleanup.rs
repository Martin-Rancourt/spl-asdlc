//! A CI run leaves nothing on a self-hosted runner (ASDLC-3YRA): each job
//! keeps its temporary files, and Cargo's build, in the runner's temp folder,
//! which the runner empties when the job ends whatever its outcome; a sweep
//! removes what earlier runs left in the system's temporary folder.

use crate::harness::{Scratch, files, load, relative, root, run_owned, steps};

use crate::harness::SCRATCH as STEP;

/// The workflows whose jobs may run on a self-hosted runner: all but
/// binaries.yml, whose matrix is GitHub's hosted runners only (Windows among
/// them, where the step's shell is not bash).
fn workflows() -> Vec<std::path::PathBuf> {
    files()
        .into_iter()
        .filter(|p| relative(p).starts_with(".github/workflows/"))
        .filter(|p| relative(p) != ".github/workflows/binaries.yml")
        .collect()
}

#[test]
fn every_job_starts_by_moving_its_temporary_files_to_the_runners_temp() {
    let mut seen = 0;
    for f in workflows() {
        let doc = load(&f);
        let Some(jobs) = doc["jobs"].as_hash() else {
            continue;
        };
        for (id, job) in jobs {
            let Some(first) = job["steps"].as_vec().and_then(|s| s.first()) else {
                continue;
            };
            let id = id.as_str().unwrap_or("?");
            assert_eq!(
                first["name"].as_str(),
                Some(STEP),
                "{} ({id}): the first step is not `{STEP}`",
                relative(&f)
            );
            let body = first["run"].as_str().unwrap_or_default();
            assert!(
                body.contains(r#"echo "TMPDIR=$RUNNER_TEMP" >> "$GITHUB_ENV""#),
                "{} ({id}): `{STEP}` does not set TMPDIR to $RUNNER_TEMP",
                relative(&f)
            );
            assert_eq!(
                first["shell"].as_str(),
                Some("bash"),
                "{} ({id}): `{STEP}` runs in another shell than bash",
                relative(&f)
            );
            assert!(
                first["if"].is_badvalue(),
                "{} ({id}): `{STEP}` has a condition",
                relative(&f)
            );
            seen += 1;
        }
    }
    assert!(seen >= 20, "{seen} jobs");
}

#[test]
fn a_job_that_runs_cargo_builds_in_the_runners_temp_and_caches_it_there() {
    for f in workflows() {
        let doc = load(&f);
        let Some(jobs) = doc["jobs"].as_hash() else {
            continue;
        };
        for (id, job) in jobs {
            let id = id.as_str().unwrap_or("?");
            let Some(all) = job["steps"].as_vec() else {
                continue;
            };
            let cargo = all.iter().any(|s| {
                s["run"]
                    .as_str()
                    .is_some_and(|r| r.lines().any(|l| l.trim_start().starts_with("cargo ")))
            });
            if !cargo {
                continue;
            }
            let first = &all[0];
            let body = first["run"].as_str().unwrap_or_default();
            // `cargo fmt` builds nothing.
            let builds = all.iter().any(|s| {
                s["run"].as_str().is_some_and(|r| {
                    r.lines().any(|l| {
                        let l = l.trim_start();
                        l.starts_with("cargo ") && !l.starts_with("cargo fmt")
                    })
                })
            });
            if builds {
                assert!(
                    body.contains(
                        r#"echo "CARGO_TARGET_DIR=$RUNNER_TEMP/target" >> "$GITHUB_ENV""#
                    ),
                    "{} ({id}): cargo builds outside the runner's temp",
                    relative(&f)
                );
                assert_eq!(
                    first["id"].as_str(),
                    Some("scratch"),
                    "{} ({id})",
                    relative(&f)
                );
            }
            for s in all {
                if s["uses"]
                    .as_str()
                    .is_some_and(|u| u.starts_with("Swatinem/rust-cache@"))
                {
                    assert_eq!(
                        s["with"]["workspaces"].as_str(),
                        Some(". -> ${{ steps.scratch.outputs.target }}"),
                        "{} ({id}): the rust cache does not read the build in the runner's temp",
                        relative(&f)
                    );
                }
            }
        }
    }
}

#[test]
fn nothing_reads_cargos_build_in_the_workspace() {
    for f in workflows() {
        let doc = load(&f);
        for (job, s) in steps(&doc) {
            let body = s["run"].as_str().unwrap_or_default();
            assert!(
                !body.contains("\"target/") && !body.contains(" target/"),
                "{} ({job}): reads target/ in the workspace, where cargo no longer builds",
                relative(&f)
            );
        }
    }
}

#[test]
fn the_scratch_step_gives_the_build_relative_to_the_workspace() {
    let doc = load(&root().join(".github/workflows/pr.yml"));
    let step = crate::harness::step(&doc, "test", "scratch");
    let s = Scratch::new("cleanup-scratch");
    let workspace = s.0.join("_work/repo/repo");
    let temp = s.0.join("_work/_temp");
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::create_dir_all(&temp).unwrap();
    let env = s.0.join("env");
    std::fs::write(&env, "").unwrap();
    let ran = run_owned(
        step["run"].as_str().unwrap(),
        &[
            ("RUNNER_TEMP", temp.display().to_string()),
            ("GITHUB_WORKSPACE", workspace.display().to_string()),
            ("GITHUB_ENV", env.display().to_string()),
        ],
        &s.0,
    );
    assert_eq!(ran.code, 0, "{}", ran.log);
    assert_eq!(ran.output("target").as_deref(), Some("../../_temp/target"));
    let written = std::fs::read_to_string(&env).unwrap();
    assert!(
        written.contains(&format!("TMPDIR={}\n", temp.display())),
        "{written}"
    );
    assert!(
        written.contains(&format!("CARGO_TARGET_DIR={}/target\n", temp.display())),
        "{written}"
    );
}

/// Runs the sweep on `dir`.
fn sweep(dir: &std::path::Path) -> crate::harness::Ran {
    run_owned(
        &format!(
            "bash {}",
            root().join("scripts/sweep-runner-tmp.sh").display()
        ),
        &[("SPL_ASDLC_SWEEP_DIR", dir.display().to_string())],
        &root(),
    )
}

/// Ages a path by two days.
fn age(path: &std::path::Path) {
    let old = ["20000101", "0000"].concat();
    let ok = std::process::Command::new("touch")
        // POSIX shape accepted by both GNU and BSD touch. `-d` is GNU-only.
        .args(["-h", "-t"])
        .arg(old)
        .arg(path)
        .status()
        .unwrap()
        .success();
    assert!(ok);
}

#[test]
fn the_sweep_removes_old_leftovers_and_leaves_a_run_in_progress() {
    let s = Scratch::new("cleanup-sweep");
    let d = &s.0;
    // Old leftovers of each kind.
    for name in ["tmp.OldDir123", "spl-asdlc-wf-run-1-2"] {
        let p = d.join(name);
        std::fs::create_dir_all(p.join("inner")).unwrap();
        std::fs::write(p.join("inner/file"), "x").unwrap();
        age(&p.join("inner/file"));
        age(&p.join("inner"));
        age(&p);
    }
    let old_file = d.join(".tmpAbC12");
    std::fs::write(&old_file, "x").unwrap();
    age(&old_file);
    // A recent entry: a run in progress.
    let recent = d.join("tmp.Recent456");
    std::fs::create_dir_all(&recent).unwrap();
    // An old entry holding a recent file: a run in progress too.
    let busy = d.join("tmp.Busy789");
    std::fs::create_dir_all(&busy).unwrap();
    std::fs::write(busy.join("live"), "x").unwrap();
    age(&busy);
    // An old entry of another name: not ours.
    let other = d.join("other-project-xyz");
    std::fs::create_dir_all(&other).unwrap();
    age(&other);

    let ran = sweep(d);
    assert_eq!(ran.code, 0, "{}", ran.log);
    assert!(!d.join("tmp.OldDir123").exists(), "{}", ran.log);
    assert!(!d.join("spl-asdlc-wf-run-1-2").exists(), "{}", ran.log);
    assert!(!old_file.exists(), "{}", ran.log);
    assert!(recent.exists(), "a recent entry was removed");
    assert!(busy.join("live").exists(), "an entry in use was removed");
    assert!(other.exists(), "an entry of another name was removed");
    assert!(
        ran.log.contains("3 leftover(s) of earlier runs removed"),
        "{}",
        ran.log
    );
}

#[test]
fn the_sweep_never_fails_the_job() {
    let s = Scratch::new("cleanup-missing");
    let ran = sweep(&s.0.join("absent"));
    assert_eq!(ran.code, 0, "{}", ran.log);
}
