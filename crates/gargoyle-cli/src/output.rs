//! helpers for writing results to stdout

use std::io::Write;

use serde::Serialize;

/// like `println!`, but a closed pipe comes back as an error instead of a panic,
/// so `gargoyle checks list | head -1` exits cleanly
macro_rules! out {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        writeln!(std::io::stdout().lock(), $($arg)*)
    }};
}
pub(crate) use out;

/// writes pretty json plus a trailing newline to stdout
pub(crate) fn print_json(value: &impl Serialize) -> anyhow::Result<()> {
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer_pretty(&mut stdout, value)?;
    writeln!(stdout)?;
    Ok(())
}
