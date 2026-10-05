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
    },
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    /// Validate a `spl-asdlc.toml`; exits 1 and lists every problem when it is invalid.
    Check {
        /// The file to check.
        #[arg(long, default_value = config::FILE_NAME)]
        file: PathBuf,
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
            command: ConfigCommand::Check { file },
        } => config_check(&file, &mut out, &mut err),
        Command::Ticket {
            command:
                TicketCommand::Check {
                    item,
                    description,
                    file,
                },
        } => ticket_check(item, description, &file, &mut out, &mut err),
    }
}

fn ticket_check(
    item: Option<String>,
    description: Option<PathBuf>,
    file: &std::path::Path,
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
            let config = match Config::load(file) {
                Ok(c) => c,
                Err(errors) => {
                    let _ = writeln!(err, "{}: invalid configuration", file.display());
                    for e in &errors.0 {
                        let _ = writeln!(err, "  {e}");
                    }
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

fn config_check(file: &std::path::Path, out: &mut dyn Write, err: &mut dyn Write) -> ExitCode {
    if !file.is_file() {
        let _ = writeln!(err, "{} could not be read: no such file", file.display());
        return ExitCode::from(2);
    }
    match Config::load(file) {
        Ok(c) => {
            let _ = writeln!(
                out,
                "{}: valid — product {} ({}), tracker {} team {}, {} release angle(s), {} review angle(s)",
                file.display(),
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
                "{}: invalid — the bridge will not serve this repository",
                file.display()
            );
            for e in &errors.0 {
                let _ = writeln!(err, "  {e}");
            }
            ExitCode::from(1)
        }
    }
}
