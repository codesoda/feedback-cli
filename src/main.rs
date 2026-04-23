use std::process::ExitCode;

use clap::Parser;
use discuss::cli::Args;

fn main() -> ExitCode {
    match discuss::run(Args::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(discuss::exit_code_for_error(&error) as u8)
        }
    }
}
