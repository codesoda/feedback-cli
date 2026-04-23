use std::process::ExitCode;

use clap::Parser;
use discuss::cli::Args;

#[tokio::main]
async fn main() -> ExitCode {
    match discuss::run(Args::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(discuss::exit_code_for_error(&error) as u8)
        }
    }
}
