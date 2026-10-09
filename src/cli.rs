//! The `spl-asdlc` command line (`docs/spec/07-the-bridge.md`, Verbs).
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
        /// A `spl-asdlc.toml`; repeat for several repositories.
        #[arg(long = "file", default_value = config::FILE_NAME)]
        files: Vec<PathBuf>,
        /// The instance overlay of a single `--file` (default: `spl-asdlc.instance.toml`
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
    /// The release cycle's verbs (`docs/spec/09-human-gestures.md`).
    Release {
        #[command(subcommand)]
        command: ReleaseCommand,
        /// The configuration of the product.
        #[arg(long, global = true, default_value = config::FILE_NAME)]
        file: PathBuf,
        /// The instance overlay (default: `spl-asdlc.instance.toml` beside the file, when present).
        #[arg(long, global = true)]
        instance: Option<PathBuf>,
        /// The product, checked against the configuration.
        #[arg(long, global = true)]
        product: Option<String>,
    },
    /// Verify the setup of the repository a configuration names.
    Doctor {
        #[arg(long, default_value = config::FILE_NAME)]
        file: PathBuf,
        /// The instance overlay (default: `spl-asdlc.instance.toml` beside the file, when present).
        #[arg(long)]
        instance: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
enum ReleaseCommand {
    /// The GO decision and its inputs (exit 1 when there is no GO).
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Dispatch the cut workflow: the next candidate, or a new cycle.
    Cut {
        #[arg(long)]
        open: bool,
    },
    /// Close a cycle by waiver: a component deliberately not deployed.
    Close {
        #[arg(long)]
        version: String,
        #[arg(long)]
        reason: String,
    },
    /// Change a finding's severity.
    Requalify {
        ticket: String,
        #[arg(long)]
        severity: String,
        #[arg(long)]
        reason: String,
    },
    /// Abandon the cycle in flight.
    Abandon,
}

#[derive(Debug, Subcommand)]
enum TicketCommand {
    /// The refinement gate (`docs/spec/01-ticket-contract.md`): says whether a
    /// ticket's contract is complete, or lists what is missing (exit 1).
    Check {
        /// The ticket's identifier in the configured tracker.
        item: Option<String>,
        /// Judge a description read from this file instead (`-` for stdin).
        #[arg(long, conflicts_with = "item")]
        description: Option<PathBuf>,
        /// The configuration naming the tracker.
        #[arg(long, default_value = config::FILE_NAME)]
        file: PathBuf,
        /// The instance overlay (default: `spl-asdlc.instance.toml` beside the file, when present).
        #[arg(long)]
        instance: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    /// Validate a `spl-asdlc.toml`; exits 1 and lists every problem when it is invalid.
    Check {
        /// The file to check.
        #[arg(long, default_value = config::FILE_NAME)]
        file: PathBuf,
        /// The instance overlay (default: `spl-asdlc.instance.toml` beside the file, when present).
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
    if instance.is_some() && files.len() > 1 {
        let _ = writeln!(
            err,
            "--instance names the overlay of one configuration: with several --file, each file's overlay is the one beside it ({} beside a {}, <stem>.instance.toml beside any other)",
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
        let env = crate::host::process_env;
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
        let mut log = if dry_run {
            ActionLog::in_memory()
        } else {
            match log_path
                .clone()
                .or_else(|| ActionLog::default_path(&c.repo, &env))
            {
                Some(p) => ActionLog::at(p),
                None => {
                    let _ = writeln!(err, "no state directory is known: pass --log <file>");
                    return ExitCode::from(2);
                }
            }
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
        ReleaseCommand::Status { json } => match crate::verbs::status(&t) {
            Ok((cycle, d)) => {
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
        ReleaseCommand::Close { version, reason } => {
            crate::verbs::close(&t, &mut log, &version, &reason, &who())
        }
        ReleaseCommand::Requalify {
            ticket,
            severity,
            reason,
        } => match crate::method::vocabulary::Severity::from_name(&severity) {
            Some(s) => crate::verbs::requalify(&t, &mut log, &ticket, s, &reason, &who()),
            None => Err(format!(
                "--severity must be blocker, should-fix or nit, not {severity}"
            )),
        },
        ReleaseCommand::Abandon => crate::verbs::abandon(&t, &mut log, &who()),
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
                "{name}: valid — product {} ({}), tracker {} team {}, {} release angle(s), {} review angle(s)",
                c.product.name,
                c.product.repository,
                c.tracker.kind,
                c.tracker.team,
                c.release.angles.len(),
                c.review.angles.len(),
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
