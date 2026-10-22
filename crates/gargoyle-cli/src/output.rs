//! helpers for writting results to stdout

use std::io::Write;

use serde::Serialize;

/// writes pretty json plus a trailing newline to stdout
pub(crate) fn print_json(value: &impl Serialize) -> anyhow::Result<()> {
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer_pretty(&mut stdout, value)?;
    writeln!(stdout)?;
    Ok(())
}
