//! the `gargoyle` command line interface

// the binary is the only place allowed to write to the terminal
#![allow(clippy::print_stdout, clippy::print_stderr)]

mod checks;
mod diff;
mod exit;
mod guide;
mod ingest;
mod init;
mod markdown;
mod output;
mod prepare;
mod report;
mod workspace;

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};
use gargoyle_core::{CONFIG_FILE, Config};
use tracing_subscriber::EnvFilter;

use crate::exit::{ConfigError, Exit};
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
    /// print every change gargoyle would review, committed or not
    Diff {
        /// branch or commit to compare against, overrides `review.base`
        #[arg(long)]
        base: Option<String>,
    },
    /// print how an agent runs a review, step by step
    Guide,
    /// set up this repo: gargoyle.toml, the built-in checks and the /gargoyle skill
    Init {
        /// overwrite an existing gargoyle.toml, checks are never overwritten
        #[arg(long)]
        force: bool,
        /// don't add the /gargoyle skill, by default init adds it with `npx skills add`
        #[arg(long)]
        no_skill: bool,
    },
    /// validate a reviewer's answer for one task and keep it for the report
    Ingest {
        /// the run id `gargoyle prepare` printed
        run_id: String,
        /// the task the answer is for, like `security-review-1`
        task_id: String,
        /// file with the answer, reads stdin when missing or `-`
        file: Option<PathBuf>,
    },
    /// collect the changes, pick the checks and write one review prompt per task
    Prepare {
        /// branch or commit to compare against, overrides `review.base`
        #[arg(long)]
        base: Option<String>,
        /// only these checks, by id
        #[arg(long, value_delimiter = ',')]
        checks: Vec<String>,
        /// print the plan as JSON, see `gargoyle schema plan`
        #[arg(long)]
        json: bool,
    },
    /// merge every answer of a run into one report, exits with 1 when something fails `review.fail_on`
    Report {
        /// the run id `gargoyle prepare` printed
        run_id: String,
        #[arg(long, value_enum, default_value_t)]
        format: report::Format,
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
    /// a run's plan.json, what `gargoyle prepare --json` prints
    Plan,
    /// what `gargoyle report --format json` prints
    Report,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    init_tracing(cli.verbose);

    match run(&cli) {
        Ok(exit) => exit.into(),
        // `gargoyle diff | head` closes the pipe early, that's not an error
        Err(err) if is_broken_pipe(&err) => Exit::Clean.into(),
        Err(err) if err.is::<ConfigError>() => {
            eprintln!("error: {err:#}");
            Exit::Config.into()
        }
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
            Ok(dir) => checks::validate(&dir),
            Err(exit) => Ok(exit),
        },
        Command::Diff { base } => diff::run(base.as_deref()),
        Command::Ingest {
            run_id,
            task_id,
            file,
        } => ingest::run(run_id, task_id, file.as_deref()),
        Command::Guide => guide::run(),
        Command::Init { force, no_skill } => init::run(Path::new("."), *force, !*no_skill),
        Command::Prepare { base, checks, json } => prepare::run(base.as_deref(), checks, *json),
        Command::Report { run_id, format } => report::run(run_id, *format),
        Command::Schema { name } => {
            let schema = match name {
                SchemaName::ReviewOutput => gargoyle_core::schema::review_output(),
                SchemaName::Check => gargoyle_core::schema::check(),
                SchemaName::Config => gargoyle_core::schema::config(),
                SchemaName::Plan => gargoyle_core::schema::plan(),
                SchemaName::Report => gargoyle_core::schema::report(),
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

/// true when the reader of stdout went away, like `head` after enough lines
fn is_broken_pipe(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        let io = cause
            .downcast_ref::<std::io::Error>()
            .map(std::io::Error::kind);
        let json = cause
            .downcast_ref::<serde_json::Error>()
            .and_then(serde_json::Error::io_error_kind);
        io == Some(ErrorKind::BrokenPipe) || json == Some(ErrorKind::BrokenPipe)
    })
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
