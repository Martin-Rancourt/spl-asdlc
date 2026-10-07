use std::process::ExitCode;

fn main() -> ExitCode {
    spl_asdlc::cli::run(std::env::args_os())
}
