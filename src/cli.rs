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
                "{}: valid — product {} ({}), tracker {:?} team {}, {} release angle(s), {} review angle(s)",
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
