use std::process::ExitCode;

use aetelier_ledger_cli::cli::Cli;
use aetelier_ledger_cli::config::{MAINNET_TREASURY, pinned_treasury};
use aetelier_ledger_cli::error::{CliError, Exit, Outcome};
use aetelier_ledger_cli::flow::TerminalConfirm;
use aetelier_ledger_cli::{Context, run};
use clap::Parser;
use clap::error::ErrorKind;

const MAX_WARNING: usize = 512;

fn emit(outcome: &Outcome) -> ExitCode {
    println!("{}", outcome.body);
    ExitCode::from(outcome.exit.code())
}

fn printable(message: &str) -> String {
    message
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_WARNING)
        .collect()
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) if matches!(e.kind(), ErrorKind::DisplayHelp | ErrorKind::DisplayVersion) => {
            print!("{e}");
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            let message = e.to_string();
            return emit(&Outcome::from_error(CliError::usage(
                "usage",
                message.trim(),
            )));
        }
    };
    let treasury = match pinned_treasury(MAINNET_TREASURY) {
        Ok(treasury) => treasury,
        Err(e) => return emit(&Outcome::from_error(e)),
    };
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            return emit(&Outcome::from_error(CliError::new(
                Exit::Transient,
                "runtime",
                e.to_string(),
            )));
        }
    };
    let mut confirm = TerminalConfirm;
    let mut warn = |message: &str| eprintln!("warning: {}", printable(message));
    let outcome = runtime.block_on(run(
        cli,
        Context {
            api_key: std::env::var("AETELIER_API_KEY").ok(),
            mainnet_treasury: treasury,
            confirm: &mut confirm,
            warn: &mut warn,
        },
    ));
    emit(&outcome)
}
