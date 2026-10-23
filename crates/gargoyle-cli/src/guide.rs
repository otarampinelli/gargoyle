//! `gargoyle guide`, the review workflow for agents
//!
//! it lives in the binary so the steps always match the commands of the installed version

use std::io::Write;

use crate::exit::Exit;

const GUIDE: &str = include_str!("../templates/guide.md");

pub(crate) fn run() -> anyhow::Result<Exit> {
    std::io::stdout().lock().write_all(GUIDE.as_bytes())?;
    Ok(Exit::Clean)
}
