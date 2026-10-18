//! The `spl-asdlc` command line (`docs/reference/07-the-bridge.md`, Verbs).
//!
//! Exit codes: 0 success, 1 a judgement says no (invalid configuration,
//! incomplete contract, no GO), 2 the command could not run (usage, I/O).

use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use crate::config::{self, Config};

#[derive(Debug, Parser)]
#[command(name = "spl-asdlc", version, about = "The SPL-ASDLC bridge")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Configuration commands.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Ticket commands.
    Ticket {
        #[command(subcommand)]
        command: TicketCommand,
    },
    /// One pass of the bridge over the repository of each configuration.
    Tick {
        /// A configuration (`.spl-asdlc/config.toml`); repeat for several repositories.
        #[arg(long = "file", default_value = config::DEFAULT_PATH)]
        files: Vec<PathBuf>,
        /// The instance overlay of a single `--file` (default: `config.instance.toml`
        /// beside each file, when present).
        #[arg(long)]
        instance: Option<PathBuf>,
        /// Read for real, write nothing: print what the tick would do.
        #[arg(long)]
        dry_run: bool,
        /// The action log (default: the user's state directory).
        #[arg(long)]
        log: Option<PathBuf>,
    },
    /// The release cycle's verbs (`docs/reference/09-human-gestures.md`).
    Release {
        #[command(subcommand)]
        command: ReleaseCommand,
        /// The configuration of the product.
        #[arg(long, global = true, default_value = config::DEFAULT_PATH)]
        file: PathBuf,
        /// The instance overlay (default: `config.instance.toml` beside the file, when present).
        #[arg(long, global = true)]
        instance: Option<PathBuf>,
        /// The product, checked against the configuration.
        #[arg(long, global = true)]
        product: Option<String>,
    },
    /// Verify the setup of the repository a configuration names.
    Doctor {
        #[arg(long, default_value = config::DEFAULT_PATH)]
        file: PathBuf,
        /// The instance overlay (default: `config.instance.toml` beside the file, when present).
        #[arg(long)]
        instance: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
enum ReleaseCommand {
    /// The GO decision and its inputs (exit 1 when there is no GO), or that
    /// no cycle is in flight once the last one closed (exit 0).
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Dispatch the cut workflow: the next candidate, or a new cycle.
    Cut {
        #[arg(long)]
        open: bool,
    },
    /// Prepare the next release in a checkout of the release branch: compute
    /// its version from the commits since the last release, write it into the
    /// version files and the CHANGELOG, and write its notes. Prints
    /// `version=`, `tag=` and `previous-tag=` lines; exit 1 when there is
    /// nothing to release. Reads the repository's file only, never an
    /// instance overlay, and talks to no service.
    Prepare {
        /// Force this version (`X.Y.Z`) instead of the computed one.
        #[arg(long = "version")]
        forced: Option<String>,
        /// Write the release notes (the CHANGELOG section) to this file.
        #[arg(long)]
        notes: Option<PathBuf>,
        /// The repository's working tree.
        #[arg(long, default_value = ".")]
        dir: PathBuf,
        /// `<owner>/<repo>` for the links of the notes (default: `product.repository`).
        #[arg(long)]
        repository: Option<String>,
        /// The date of the CHANGELOG heading, `YYYY-MM-DD` (default: today, UTC).
        #[arg(long)]
        date: Option<String>,
        /// Compute and print; write no file of the working tree.
        #[arg(long)]
        dry_run: bool,
    },
    /// Change whether a finding blocks the release.
    Requalify {
        ticket: String,
        /// `true`: the finding blocks the release; `false`: it is logged for later.
        #[arg(long, action = clap::ArgAction::Set)]
        blocking: bool,
        #[arg(long)]
        reason: String,
    },
    /// Abandon the cycle in flight.
    Abandon,
    /// Retract a release whose publication failed before anything was
    /// deployed: delete its GitHub release and its tag, say so on the release
    /// issue; the next candidate takes the version. Refused when a deploy or
    /// a merge back ran, or when the publication run cannot be read.
    Retract {
        /// The release tag (`release.release_tag`).
        tag: String,
    },
}

#[derive(Debug, Subcommand)]
enum TicketCommand {
    /// The refinement gate (`docs/reference/01-ticket-contract.md`): says whether a
    /// ticket's contract is complete, or lists what is missing (exit 1).
    Check {
        /// The ticket's identifier in the configured tracker.
        item: Option<String>,
        /// Judge a description read from this file instead (`-` for stdin).
        #[arg(long, conflicts_with = "item")]
        description: Option<PathBuf>,
        /// The configuration naming the tracker.
        #[arg(long, default_value = config::DEFAULT_PATH)]
        file: PathBuf,
        /// The instance overlay (default: `config.instance.toml` beside the file, when present).
        #[arg(long)]
        instance: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    /// Validate a configuration; exits 1 and lists every problem when it is invalid.
    Check {
        /// The file to check.
        #[arg(long, default_value = config::DEFAULT_PATH)]
        file: PathBuf,
        /// The instance overlay (default: `config.instance.toml` beside the file, when present).
        #[arg(long)]
        instance: Option<PathBuf>,
    },
}

/// Run the command line; `args` includes the program name.
pub fn run<I, T>(args: I) -> ExitCode
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(e) => {
            let _ = e.print();
            return if e.use_stderr() {
                ExitCode::from(2)
            } else {
                ExitCode::SUCCESS
            };
        }
    };
    let mut out = std::io::stdout().lock();
    let mut err = std::io::stderr().lock();
    match cli.command {
        Command::Config {
            command: ConfigCommand::Check { file, instance },
        } => config_check(&file, instance.as_deref(), &mut out, &mut err),
        Command::Ticket {
            command:
                TicketCommand::Check {
                    item,
                    description,
                    file,
                    instance,
                },
        } => ticket_check(
            item,
            description,
            &file,
            instance.as_deref(),
            &mut out,
            &mut err,
        ),
        Command::Tick {
            files,
            instance,
            dry_run,
            log,
        } => tick(
            &files,
            instance.as_deref(),
            dry_run,
            log,
            &mut out,
            &mut err,
        ),
        Command::Release {
            command,
            file,
            instance,
            product,
        } => release(
            command,
            &file,
            instance.as_deref(),
            product.as_deref(),
            &mut out,
            &mut err,
        ),
        Command::Doctor { file, instance } => {
            doctor(&file, instance.as_deref(), &mut out, &mut err)
        }
    }
}

/// What a verb needs: the configuration and the two clients.
struct Connected {
    config: Config,
    /// The instance overlay merged into the configuration, if any.
    instance: Option<config::Source>,
    github: crate::github::RestGitHub,
    tracker: crate::tracker::linear::Linear,
    repo: crate::github::RepoSlug,
    login: String,
}

/// The configuration of `file`, merged with its instance overlay.
fn load(
    file: &std::path::Path,
    instance: Option<&std::path::Path>,
) -> Result<(Config, Option<config::Source>), String> {
    Config::load_with(file, instance).map_err(|errors| {
        format!(
            "{}: invalid configuration\n{}",
            file.display(),
            errors
                .0
                .iter()
                .map(|e| format!("  {e}"))
                .collect::<Vec<_>>()
                .join("\n")
        )
    })
}

fn connect(
    file: &std::path::Path,
    instance: Option<&std::path::Path>,
) -> Result<Connected, String> {
    let (config, instance) = load(file, instance)?;
    let env = crate::host::process_env;
    let github = crate::host::github(&config, &env)?;
    let tracker = crate::host::linear(&config, &env)?;
    let repo = crate::github::RepoSlug::parse(&config.product.repository)
        .ok_or("product.repository is not <owner>/<repo>")?;
    let login = crate::github::GitHub::identity(&github)
        .map_err(|e| format!("GitHub identity: {e}"))?
        .login();
    Ok(Connected {
        config,
        instance,
        github,
        tracker,
        repo,
        login,
    })
}

fn tick(
    files: &[PathBuf],
    instance: Option<&std::path::Path>,
    dry_run: bool,
    log_path: Option<PathBuf>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> ExitCode {
    use crate::action_log::{self, ActionLog};
    use crate::lock::TickLock;
    if instance.is_some() && files.len() > 1 {
        let _ = writeln!(
            err,
            "--instance names the overlay of one configuration: with several --file, each file's overlay is the one beside it (<stem>.instance.toml, {} beside a {})",
            config::INSTANCE_FILE_NAME,
            config::FILE_NAME
        );
        return ExitCode::from(2);
    }
    let mut code = ExitCode::SUCCESS;
    for file in files {
        let c = match connect(file, instance) {
            Ok(c) => c,
            Err(e) => {
                // One repository that cannot start costs no other.
                let _ = writeln!(err, "{e}");
                code = ExitCode::from(2);
                continue;
            }
        };
        let env = crate::host::process_env;
        // One tick at a time per repository (spec 07 invariant 8), beside
        // its action log. The dry run writes nothing and takes no lock.
        let log_file = if dry_run {
            None
        } else {
            match log_path
                .clone()
                .or_else(|| ActionLog::default_path(&c.repo, &env))
            {
                Some(p) => Some(p),
                None => {
                    let _ = writeln!(err, "no state directory is known: pass --log <file>");
                    return ExitCode::from(2);
                }
            }
        };
        let _lock = match &log_file {
            Some(p) => {
                let lock_path = TickLock::beside(p);
                match TickLock::try_acquire(&lock_path) {
                    Ok(Some(l)) => Some(l),
                    Ok(None) => {
                        let _ = writeln!(
                            out,
                            "{}",
                            crate::lock::already_running(&c.repo.to_string(), &lock_path)
                        );
                        continue;
                    }
                    Err(e) => {
                        let _ = writeln!(
                            err,
                            "{}: the tick lock {} could not be taken: {e}",
                            c.repo,
                            lock_path.display()
                        );
                        code = ExitCode::from(2);
                        continue;
                    }
                }
            }
            None => None,
        };
        let tracker_name = crate::tracker::Tracker::identity(&c.tracker)
            .map(|u| u.name)
            .unwrap_or_else(|e| format!("unknown ({e})"));
        let _ = writeln!(
            out,
            "{}: GitHub as {}, {} as {}{}",
            c.repo,
            c.login,
            c.config.tracker.kind,
            tracker_name,
            if dry_run {
                " — dry run, nothing is written"
            } else {
                ""
            }
        );
        let signal = crate::job::channels(&c.config, &env);
        let intents = crate::dry::Intents::default();
        let dry_github = crate::dry::DryGitHub {
            inner: &c.github,
            intents: &intents,
        };
        let dry_tracker = crate::dry::DryTracker {
            inner: &c.tracker,
            intents: &intents,
        };
        let dry_signal = crate::dry::DrySignal { intents: &intents };
        let t = crate::tick::Tick {
            config: &c.config,
            repo: c.repo.clone(),
            github: if dry_run { &dry_github } else { &c.github },
            tracker: if dry_run { &dry_tracker } else { &c.tracker },
            signal: if dry_run {
                &dry_signal
            } else {
                signal.as_ref()
            },
            github_login: c.login.clone(),
            now: crate::github::Timestamp::now(),
            deadline: None,
            dry_run,
        };
        let mut log = match log_file {
            Some(p) => ActionLog::at(p),
            None => ActionLog::in_memory(),
        };
        let report = crate::tick::run(&t, &mut log);
        // A waiting note is said by the report's own line below.
        for e in log
            .entries()
            .iter()
            .filter(|e| e.outcome != action_log::Outcome::Waiting)
        {
            let _ = writeln!(out, "{}", action_log::describe(e));
        }
        if dry_run {
            for line in intents.lines() {
                let _ = writeln!(out, "  would write: {line}");
            }
        }
        if let Err(e) = log.flush() {
            let _ = writeln!(err, "the action log could not be written: {e}");
            code = ExitCode::from(1);
        }
        for f in &report.failures {
            let _ = writeln!(err, "{}: {f}", c.repo);
        }
        for line in report.waiting_lines() {
            let _ = writeln!(out, "{}: {line}", c.repo);
        }
        let _ = writeln!(out, "{}: {}", c.repo, report.summary());
        if !report.ok() {
            code = ExitCode::from(1);
        }
    }
    code
}

fn who() -> String {
    crate::host::process_env("USER")
        .or_else(|| crate::host::process_env("USERNAME"))
        .unwrap_or_else(|| "a human at the command line".to_string())
}

fn release(
    command: ReleaseCommand,
    file: &std::path::Path,
    instance: Option<&std::path::Path>,
    product: Option<&str>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> ExitCode {
    if let ReleaseCommand::Prepare {
        forced,
        notes,
        dir,
        repository,
        date,
        dry_run,
    } = command
    {
        let options = crate::prepare::Options {
            dir,
            version: forced,
            notes,
            date,
            repository,
            dry_run,
        };
        return prepare(file, instance, product, &options, out, err);
    }
    let c = match connect(file, instance) {
        Ok(c) => c,
        Err(e) => {
            let _ = writeln!(err, "{e}");
            return ExitCode::from(2);
        }
    };
    if let Some(p) = product.filter(|p| *p != c.config.product.name) {
        let _ = writeln!(
            err,
            "{} configures {}, not {p}",
            file.display(),
            c.config.product.name
        );
        return ExitCode::from(2);
    }
    let env = crate::host::process_env;
    let signal = crate::job::channels(&c.config, &env);
    let t = crate::tick::Tick {
        config: &c.config,
        repo: c.repo.clone(),
        github: &c.github,
        tracker: &c.tracker,
        signal: signal.as_ref(),
        github_login: c.login.clone(),
        now: crate::github::Timestamp::now(),
        deadline: None,
        dry_run: false,
    };
    let mut log = match crate::action_log::ActionLog::default_path(&c.repo, &env) {
        Some(p) => crate::action_log::ActionLog::at(p),
        None => crate::action_log::ActionLog::in_memory(),
    };
    let result = match command {
        ReleaseCommand::Status { json } => match crate::verbs::release_status(&t) {
            Ok(crate::verbs::CycleStatus::Over { release_tag, item }) => {
                if json {
                    let _ = writeln!(
                        out,
                        "{}",
                        serde_json::json!({"in_flight": false, "closed_with": release_tag, "release_item": item})
                    );
                } else {
                    let _ = writeln!(
                        out,
                        "no cycle in flight: the last cycle closed with {release_tag} ({item})"
                    );
                }
                return ExitCode::SUCCESS;
            }
            Ok(crate::verbs::CycleStatus::InFlight(cycle, d)) => {
                if json {
                    let _ = writeln!(out, "{}", d.as_json());
                } else {
                    let _ = writeln!(
                        out,
                        "{} {}: {}",
                        cycle.item.key,
                        cycle.candidate.tag,
                        if d.go { "GO" } else { "no GO" }
                    );
                    for r in &d.reasons {
                        let _ = writeln!(out, "  - {r}");
                    }
                    let _ = writeln!(out, "  Blockers-Carried: {}", d.blockers_carried());
                }
                return if d.go {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::from(1)
                };
            }
            Err(e) => Err(e),
        },
        ReleaseCommand::Cut { open } => crate::verbs::cut(&t, &mut log, open),
        ReleaseCommand::Prepare { .. } => unreachable!("prepared above, with no connection"),
        ReleaseCommand::Requalify {
            ticket,
            blocking,
            reason,
        } => crate::verbs::requalify(&t, &mut log, &ticket, blocking, &reason, &who()),
        ReleaseCommand::Abandon => crate::verbs::abandon(&t, &mut log, &who()),
        ReleaseCommand::Retract { tag } => crate::verbs::retract(&t, &mut log, &tag, &who()),
    };
    let _ = log.flush();
    match result {
        Ok(line) => {
            let _ = writeln!(out, "{line}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            let _ = writeln!(err, "{e}");
            ExitCode::from(1)
        }
    }
}

/// `release prepare`: the repository's file alone, read in a checkout; no
/// overlay, no client.
fn prepare(
    file: &std::path::Path,
    instance: Option<&std::path::Path>,
    product: Option<&str>,
    options: &crate::prepare::Options,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> ExitCode {
    if instance.is_some() {
        let _ = writeln!(
            err,
            "release prepare reads the repository's file only: --instance is not taken"
        );
        return ExitCode::from(2);
    }
    let text = match std::fs::read_to_string(file) {
        Ok(t) => t,
        Err(e) => {
            let _ = writeln!(err, "{} could not be read: {e}", file.display());
            return ExitCode::from(2);
        }
    };
    let prep = match Config::preparation(&text) {
        Ok(p) => p,
        Err(errors) => {
            let _ = writeln!(err, "{}: invalid configuration", file.display());
            for e in &errors.0 {
                let _ = writeln!(err, "  {e}");
            }
            return ExitCode::from(2);
        }
    };
    if let Some(p) = product.filter(|p| *p != prep.product) {
        let _ = writeln!(
            err,
            "{} configures {}, not {p}",
            file.display(),
            prep.product
        );
        return ExitCode::from(2);
    }
    match crate::prepare::run(&prep, options) {
        Ok(p) => {
            let _ = writeln!(out, "version={}", p.version);
            let _ = writeln!(out, "tag={}", p.tag);
            let _ = writeln!(
                out,
                "previous-tag={}",
                p.previous_tag.as_deref().unwrap_or("")
            );
            let _ = writeln!(
                err,
                "{} {} from {} ({} commit(s)); {} {}",
                if options.dry_run {
                    "Would prepare"
                } else {
                    "Prepared"
                },
                p.tag,
                p.previous_tag
                    .as_deref()
                    .unwrap_or("the repository's root (first release)"),
                p.commits,
                if options.dry_run {
                    "would write"
                } else {
                    "wrote"
                },
                p.files.join(", ")
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            let _ = writeln!(err, "release prepare refused: {e}");
            ExitCode::from(1)
        }
    }
}

fn doctor(
    file: &std::path::Path,
    instance: Option<&std::path::Path>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> ExitCode {
    let c = match connect(file, instance) {
        Ok(c) => c,
        Err(e) => {
            let _ = writeln!(err, "{e}");
            return ExitCode::from(2);
        }
    };
    let checks = crate::doctor::run(
        &c.config,
        c.instance.as_ref(),
        &c.github,
        &c.tracker,
        "spl-asdlc binary",
    );
    for check in &checks {
        let _ = writeln!(out, "{check}");
    }
    if checks
        .iter()
        .all(|c| c.verdict == crate::doctor::Verdict::Ok)
    {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn ticket_check(
    item: Option<String>,
    description: Option<PathBuf>,
    file: &std::path::Path,
    instance: Option<&std::path::Path>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> ExitCode {
    let (name, text) = match (item, description) {
        (_, Some(path)) => {
            let text = if path.as_os_str() == "-" {
                let mut s = String::new();
                std::io::Read::read_to_string(&mut std::io::stdin(), &mut s).map(|_| s)
            } else {
                std::fs::read_to_string(&path)
            };
            match text {
                Ok(t) => (path.display().to_string(), t),
                Err(e) => {
                    let _ = writeln!(err, "{} could not be read: {e}", path.display());
                    return ExitCode::from(2);
                }
            }
        }
        (Some(key), None) => {
            let config = match load(file, instance) {
                Ok((c, _)) => c,
                Err(e) => {
                    let _ = writeln!(err, "{e}");
                    return ExitCode::from(2);
                }
            };
            let tracker = match crate::host::linear(&config, &crate::host::process_env) {
                Ok(t) => t,
                Err(e) => {
                    let _ = writeln!(err, "{e}");
                    return ExitCode::from(2);
                }
            };
            match crate::tracker::Tracker::get(&tracker, &key) {
                Ok(item) => (format!("{} {}", item.key, item.title), item.description),
                Err(e) => {
                    let _ = writeln!(err, "{e}");
                    return ExitCode::from(2);
                }
            }
        }
        (None, None) => {
            let _ = writeln!(err, "name a ticket, or pass --description <file>");
            return ExitCode::from(2);
        }
    };
    let missing = crate::method::contract::missing_from_contract(&text);
    if missing.is_empty() {
        let _ = writeln!(out, "{name}: the contract is complete");
        ExitCode::SUCCESS
    } else {
        let _ = writeln!(out, "{name}: the contract is incomplete");
        for m in &missing {
            let _ = writeln!(out, "  - {m}");
        }
        ExitCode::from(1)
    }
}

fn config_check(
    file: &std::path::Path,
    instance: Option<&std::path::Path>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> ExitCode {
    if !file.is_file() {
        let _ = writeln!(err, "{} could not be read: no such file", file.display());
        return ExitCode::from(2);
    }
    let overlay = match config::instance_path(file, instance) {
        Ok(o) => o,
        Err(errors) => {
            let _ = writeln!(err, "{errors}");
            return ExitCode::from(2);
        }
    };
    let name = match &overlay {
        Some(o) => format!("{} with {}", file.display(), o.display()),
        None => file.display().to_string(),
    };
    match Config::load_with(file, overlay.as_deref()) {
        Ok((c, _)) => {
            let _ = writeln!(
                out,
                "{name}: valid — product {} ({}), tracker {} team {}, {} release angle(s), {}",
                c.product.name,
                c.product.repository,
                c.tracker.kind,
                c.tracker.team,
                c.release.angles.len(),
                if c.review.angle_possible() {
                    format!("{} review angle(s)", c.review.angles.len())
                } else {
                    "no PR review".to_string()
                },
            );
            ExitCode::SUCCESS
        }
        Err(errors) => {
            let _ = writeln!(
                err,
                "{name}: invalid — the bridge will not serve this repository"
            );
            for e in &errors.0 {
                let _ = writeln!(err, "  {e}");
            }
            ExitCode::from(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn requalify(args: &[&str]) -> Result<(String, bool, String), String> {
        let mut argv = vec!["spl-asdlc", "release", "requalify"];
        argv.extend_from_slice(args);
        match Cli::try_parse_from(argv)
            .map_err(|e| e.to_string())?
            .command
        {
            Command::Release {
                command:
                    ReleaseCommand::Requalify {
                        ticket,
                        blocking,
                        reason,
                    },
                ..
            } => Ok((ticket, blocking, reason)),
            other => Err(format!("{other:?}")),
        }
    }

    #[test]
    fn the_retract_verb_takes_the_release_tag_and_no_force() {
        let parse = |args: &[&str]| {
            let mut argv = vec!["spl-asdlc", "release", "retract"];
            argv.extend_from_slice(args);
            Cli::try_parse_from(argv).map(|c| c.command)
        };
        match parse(&["acme-v1.2.0"]) {
            Ok(Command::Release {
                command: ReleaseCommand::Retract { tag },
                ..
            }) => assert_eq!(tag, "acme-v1.2.0"),
            other => panic!("{other:?}"),
        }
        assert!(parse(&[]).is_err(), "the tag is required");
        assert!(parse(&["acme-v1.2.0", "--force"]).is_err(), "no override");
    }

    #[test]
    fn the_requalify_verb_says_whether_the_finding_blocks() {
        assert_eq!(
            requalify(&["ABC-7", "--blocking", "false", "--reason", "cosmetic"]),
            Ok(("ABC-7".into(), false, "cosmetic".into()))
        );
        assert_eq!(
            requalify(&["ABC-7", "--blocking", "true", "--reason", "data loss"]),
            Ok(("ABC-7".into(), true, "data loss".into()))
        );
        assert!(
            requalify(&["ABC-7", "--blocking", "--reason", "r"]).is_err(),
            "a value is required"
        );
        assert!(requalify(&["ABC-7", "--blocking", "maybe", "--reason", "r"]).is_err());
        assert!(
            requalify(&["ABC-7", "--severity", "nit", "--reason", "r"]).is_err(),
            "the severities are gone"
        );
    }
}
