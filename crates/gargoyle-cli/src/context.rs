//! `gargoyle context`, where functions or types are defined, used and tested

use std::io::Write;

use gargoyle_core::RelatedCode;

use crate::exit::Exit;
use crate::workspace::{Workspace, git_error};

pub(crate) fn run(names: &[String]) -> anyhow::Result<Exit> {
    let ws = Workspace::open()?;
    let text = RelatedCode::lookup(&ws.repo, names, &ws.config).map_err(git_error)?;
    std::io::stdout().lock().write_all(text.as_bytes())?;
    Ok(Exit::Clean)
}
