//! cli interface

// the binary is the only place allowed to write to the terminal
#![allow(clippy::print_stdout, clippy::print_stderr)]

mod exit;

use std::process::ExitCode;

use clap::{CommandFactory, Parser};
use tracing_subscriber::EnvFilter;

use crate::exit::Exit;

/// provider agnostic code reviewer,
#[derive(Debug, Parser)]
#[command(name = "gargoyle", version, about)] // standard
struct Cli {
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    init_tracing(cli.verbose);

    match run() {
        Ok(exit) => exit.into(),
        Err(err) => {
            // {:#} prints the whole context chain
            eprintln!("error: {err:#}");
            Exit::Runtime.into()
        }
    }
}

fn run() -> anyhow::Result<Exit> {
    Cli::command().print_help()?;
    Ok(Exit::Clean)
}

/// logs go to stderr so stdout stays clean for JSON output
fn init_tracing(verbose: u8) {
    let default_level = match verbose {
        0 => "warn",
        1 => "debug",
        _ => "trace",
    };

    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_level));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .without_time()
        .init();
}
