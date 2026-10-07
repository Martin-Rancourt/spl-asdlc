//! Read the shipped YAML and run a step's script.

// The executions are Unix-only; on Windows only the lints use this module.
#![cfg_attr(not(unix), allow(dead_code))]

use std::path::{Path, PathBuf};

use yaml_rust2::{Yaml, YamlLoader};

pub fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Every workflow and composite action of the repository.
pub fn files() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for dir in [".github/workflows", "actions"] {
        collect(&root().join(dir), &mut out);
    }
    out.sort();
    out
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, out);
        } else if matches!(p.extension().and_then(|x| x.to_str()), Some("yml" | "yaml")) {
            out.push(p);
        }
    }
}

pub fn load(path: &Path) -> Yaml {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut docs =
        YamlLoader::load_from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert_eq!(docs.len(), 1, "{}: one document", path.display());
    docs.remove(0)
}

pub fn relative(path: &Path) -> String {
    path.strip_prefix(root())
        .unwrap_or(path)
        .display()
        .to_string()
}

pub fn workflow(rel: &str) -> Yaml {
    load(&root().join(rel))
}

/// Whether the file is a reusable workflow (`on: workflow_call`).
pub fn is_reusable(doc: &Yaml) -> bool {
    !doc["on"]["workflow_call"].is_badvalue()
}

/// Every step, with the job (or `runs` for a composite action) it is in.
pub fn steps(doc: &Yaml) -> Vec<(String, Yaml)> {
    let mut out = Vec::new();
    if let Some(jobs) = doc["jobs"].as_hash() {
        for (id, job) in jobs {
            let id = id.as_str().unwrap_or("?").to_string();
            for s in job["steps"].as_vec().into_iter().flatten() {
                out.push((id.clone(), s.clone()));
            }
        }
    }
    for s in doc["runs"]["steps"].as_vec().into_iter().flatten() {
        out.push(("runs".to_string(), s.clone()));
    }
    out
}

/// The step of `job` whose `id` (or, failing that, `name`) is `id`.
pub fn step(doc: &Yaml, job: &str, id: &str) -> Yaml {
    steps(doc)
        .into_iter()
        .find(|(j, s)| j == job && (s["id"].as_str() == Some(id) || s["name"].as_str() == Some(id)))
        .map(|(_, s)| s)
        .unwrap_or_else(|| panic!("no step {id} in job {job}"))
}

/// The `run:` body of a step.
pub fn run_body(step: &Yaml) -> String {
    step["run"]
        .as_str()
        .expect("the step has a run body")
        .to_string()
}

/// A scratch directory of its own, removed on drop.
pub struct Scratch(pub PathBuf);

impl Scratch {
    pub fn new(tag: &str) -> Scratch {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        let p = std::env::temp_dir().join(format!(
            "spl-asdlc-wf-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Scratch(p)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// What a step's script did.
#[derive(Debug)]
pub struct Ran {
    pub code: i32,
    /// stdout and stderr, in that order.
    pub log: String,
    /// What it wrote to `$GITHUB_OUTPUT`.
    pub outputs: String,
}

impl Ran {
    pub fn output(&self, key: &str) -> Option<String> {
        self.outputs
            .lines()
            .find_map(|l| l.strip_prefix(&format!("{key}=")))
            .map(str::to_string)
    }
}

/// Run a step's script the way the runner does (`bash --noprofile --norc -eo
/// pipefail`), in `cwd`, with only `env` and a PATH.
#[cfg(unix)]
pub fn run(script: &str, env: &[(&str, &str)], cwd: &Path) -> Ran {
    let scratch = Scratch::new("run");
    let file = scratch.0.join("step.sh");
    std::fs::write(&file, script).unwrap();
    let outputs = scratch.0.join("output");
    std::fs::write(&outputs, "").unwrap();
    let mut cmd = std::process::Command::new("bash");
    cmd.args(["--noprofile", "--norc", "-eo", "pipefail"])
        .arg(&file)
        .current_dir(cwd)
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", &scratch.0)
        .env("GITHUB_OUTPUT", &outputs);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("bash runs");
    Ran {
        code: out.status.code().unwrap_or(-1),
        log: format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
        outputs: std::fs::read_to_string(&outputs).unwrap(),
    }
}

/// A git repository with an initial commit, for the detectors.
#[cfg(unix)]
pub struct Repo {
    pub dir: Scratch,
}

#[cfg(unix)]
impl Repo {
    pub fn new() -> Repo {
        let r = Repo {
            dir: Scratch::new("git"),
        };
        r.git(&["init", "-q", "-b", "main"]);
        r.git(&["config", "user.email", "test@localhost"]);
        r.git(&["config", "user.name", "test"]);
        r.git(&["config", "commit.gpgsign", "false"]);
        r.write("README.md", "hello\n");
        r.write("src/lib.rs", "pub fn f() {}\n");
        r.commit("initial");
        r
    }

    pub fn git(&self, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(&self.dir.0)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    pub fn write(&self, path: &str, text: &str) {
        let p = self.dir.0.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    pub fn commit(&self, message: &str) -> String {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "--allow-empty", "-m", message]);
        self.git(&["rev-parse", "HEAD"])
    }
}
