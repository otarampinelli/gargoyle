//! the `gargoyle` command line interface

// the binary is the only place allowed to write to the terminal
#![allow(clippy::print_stdout, clippy::print_stderr)]

mod checks;
mod exit;
mod init;
mod output;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};
use gargoyle_core::{CONFIG_FILE, Config};
use tracing_subscriber::EnvFilter;

use crate::exit::Exit;
use crate::output::print_json;

/// provider agnostic AI code review, before you push
#[derive(Debug, Parser)]
#[command(name = "gargoyle", version, about)]
struct Cli {
    /// more log output (-v debug, -vv trace). `RUST_LOG` overrides this
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// list and validate review checks
    #[command(subcommand)]
    Checks(ChecksCommand),
    /// create gargoyle.toml and copy the built-in checks into .gargoyle/checks
    Init {
        /// overwrite an existing gargoyle.toml, checks are never overwritten
        #[arg(long)]
        force: bool,
    },
    /// print the JSON schema for one of gargoyle formats
    Schema {
        #[arg(value_enum)]
        name: SchemaName,
    },
}

#[derive(Debug, Subcommand)]
enum ChecksCommand {
    /// list the available checks
    List {
        #[command(flatten)]
        source: ChecksDir,
        /// print JSON instead of a table
        #[arg(long)]
        json: bool,
    },
    /// report every problem in the check files
    Validate {
        #[command(flatten)]
        source: ChecksDir,
    },
}

#[derive(Debug, Args)]
struct ChecksDir {
    /// directory containing the check files, defaults to `checks.dir` from gargoyle.toml
    #[arg(long)]
    dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum SchemaName {
    /// what a reviewer returns for one task
    ReviewOutput,
    /// the frontmatter of a check file
    Check,
    /// the gargoyle.toml config
    Config,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    init_tracing(cli.verbose);

    match run(&cli) {
        Ok(exit) => exit.into(),
        Err(err) => {
            // `{:#}` prints the whole context chain, like "failed to X: because Y"
            eprintln!("error: {err:#}");
            Exit::Runtime.into()
        }
    }
}

fn run(cli: &Cli) -> anyhow::Result<Exit> {
    let Some(command) = &cli.command else {
        Cli::command().print_help()?;
        return Ok(Exit::Clean);
    };

    match command {
        Command::Checks(ChecksCommand::List { source, json }) => match checks_dir(source) {
            Ok(dir) => checks::list(&dir, *json),
            Err(exit) => Ok(exit),
        },
        Command::Checks(ChecksCommand::Validate { source }) => match checks_dir(source) {
            Ok(dir) => Ok(checks::validate(&dir)),
            Err(exit) => Ok(exit),
        },
        Command::Init { force } => init::run(Path::new("."), *force),
        Command::Schema { name } => {
            let schema = match name {
                SchemaName::ReviewOutput => gargoyle_core::schema::review_output(),
                SchemaName::Check => gargoyle_core::schema::check(),
                SchemaName::Config => gargoyle_core::schema::config(),
            };
            print_json(&schema)?;
            Ok(Exit::Clean)
        }
    }
}

// --dir wins, otherwise checks.dir from gargoyle.toml
// the config is only read here, so schema and --version still work with a broken one
fn checks_dir(source: &ChecksDir) -> Result<PathBuf, Exit> {
    if let Some(dir) = &source.dir {
        return Ok(dir.clone());
    }
    match Config::load_or_default(Path::new(CONFIG_FILE)) {
        Ok(config) => Ok(config.checks.dir),
        Err(err) => {
            eprintln!("error: {err}");
            Err(Exit::Config)
        }
    }
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
