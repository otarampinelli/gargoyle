//! how commands talk to people: the clack look at a terminal, plain lines for agents,
//! pipes and CI, so nothing that reads gargoyle's output ever sees a color code or waits
//! on a prompt
//!
//! two kinds of output, picked separately:
//! - messages and prompts go to stderr, fancy when someone can answer a prompt
//! - data (a diff, a report, a table) goes to stdout, styled only when stdout is a terminal,
//!   `console::style` already drops the colors when it isn't

use std::fmt::Display;
use std::io::IsTerminal;

use console::{Style, Term};
use gargoyle_core::Severity;

use crate::output::out;

/// turns colors off everywhere when `NO_COLOR` is set, call once before any output
pub(crate) fn setup() {
    // https://no-color.org: set and not empty turns colors off
    if std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty()) {
        console::set_colors_enabled(false);
        console::set_colors_enabled_stderr(false);
    }
}

/// where a command's messages go, picked once per run
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ui {
    /// a person at a terminal: symbols, colors and prompts, on stderr
    Fancy,
    /// anything else: plain lines, the same text agents and tests have always read
    Plain,
}

impl Ui {
    /// fancy only when someone can answer a prompt, both stdin and stderr are a terminal
    pub(crate) fn detect() -> Self {
        if std::io::stdin().is_terminal() && std::io::stderr().is_terminal() {
            Self::Fancy
        } else {
            Self::Plain
        }
    }

    pub(crate) fn intro(self, title: impl Display) -> anyhow::Result<()> {
        if self == Self::Fancy {
            cliclack::intro(console::style(title).bold())?;
        }
        Ok(())
    }

    /// something the command did, plain on stdout
    pub(crate) fn done(self, message: impl Display) -> anyhow::Result<()> {
        match self {
            Self::Fancy => cliclack::log::success(message)?,
            Self::Plain => out!("{message}")?,
        }
        Ok(())
    }

    /// something it left alone, or worth knowing, plain on stdout
    pub(crate) fn note(self, message: impl Display) -> anyhow::Result<()> {
        match self {
            Self::Fancy => cliclack::log::info(console::style(message).dim())?,
            Self::Plain => out!("{message}")?,
        }
        Ok(())
    }

    /// progress while the real output goes to stdout, plain on stderr
    pub(crate) fn step(self, fancy: impl Display, plain: impl Display) -> anyhow::Result<()> {
        match self {
            Self::Fancy => cliclack::log::step(fancy)?,
            Self::Plain => eprintln!("{plain}"),
        }
        Ok(())
    }

    pub(crate) fn warn(self, message: impl Display) -> anyhow::Result<()> {
        match self {
            Self::Fancy => cliclack::log::warning(message)?,
            Self::Plain => eprintln!("warning: {message}"),
        }
        Ok(())
    }

    /// a spinner at a terminal, `plain` on stderr otherwise when there's one
    pub(crate) fn spinner(self, fancy: impl Display, plain: Option<&str>) -> Spinner {
        match self {
            Self::Fancy => {
                let spinner = cliclack::spinner();
                spinner.start(fancy);
                Spinner(Some(spinner))
            }
            Self::Plain => {
                if let Some(plain) = plain {
                    eprintln!("{plain}");
                }
                Spinner(None)
            }
        }
    }

    /// a framed block, like the tasks of a plan
    pub(crate) fn block(self, title: impl Display, body: impl Display) -> anyhow::Result<()> {
        match self {
            Self::Fancy => cliclack::note(title, body)?,
            Self::Plain => out!("{title}\n{body}")?,
        }
        Ok(())
    }

    /// the last line, what to do next
    pub(crate) fn outro(self, message: impl Display) -> anyhow::Result<()> {
        match self {
            Self::Fancy => cliclack::outro(message)?,
            Self::Plain => out!("{message}")?,
        }
        Ok(())
    }
}

pub(crate) struct Spinner(Option<cliclack::ProgressBar>);

impl std::fmt::Debug for Spinner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.is_some() {
            "Spinner"
        } else {
            "Spinner(plain)"
        })
    }
}

impl Spinner {
    /// ends it, `plain` goes to stderr when there's no terminal and something to say
    pub(crate) fn stop(self, fancy: impl Display, plain: Option<&str>) {
        match self.0 {
            Some(spinner) => spinner.stop(fancy),
            None => {
                if let Some(plain) = plain {
                    eprintln!("{plain}");
                }
            }
        }
    }

    pub(crate) fn fail(self, message: impl Display) {
        match self.0 {
            Some(spinner) => spinner.error(message),
            None => eprintln!("warning: {message}"),
        }
    }
}

/// stdout is a terminal, so data can wrap to its width and show symbols
pub(crate) fn stdout_is_terminal() -> bool {
    std::io::stdout().is_terminal()
}

/// the width to wrap stdout text at, none when it isn't a terminal
pub(crate) fn stdout_width() -> Option<usize> {
    if !stdout_is_terminal() {
        return None;
    }
    Term::stdout()
        .size_checked()
        .map(|(_, columns)| usize::from(columns).min(120))
}

pub(crate) fn severity_style(severity: Severity) -> Style {
    match severity {
        Severity::Error => Style::new().red().bold(),
        Severity::Warning => Style::new().yellow().bold(),
        Severity::Info => Style::new().blue().bold(),
    }
}

/// `text` wrapped to `width` columns, every line after the first starts with `indent`,
/// a word longer than a line stays whole
pub(crate) fn wrap(text: &str, width: usize, indent: &str) -> String {
    let room = width.saturating_sub(indent.chars().count()).max(20);
    let mut out = String::new();
    let mut line = 0;
    for word in text.split_whitespace() {
        let len = console::measure_text_width(word);
        if line > 0 && line + 1 + len > room {
            out.push('\n');
            out.push_str(indent);
            line = 0;
        } else if line > 0 {
            out.push(' ');
            line += 1;
        }
        out.push_str(word);
        line += len;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_with_a_hanging_indent() {
        let text = "status and sort go straight into the SQL string";
        assert_eq!(
            wrap(text, 30, "    "),
            "status and sort go\n    straight into the SQL\n    string"
        );
    }

    #[test]
    fn a_long_word_stays_whole() {
        let word = "a".repeat(40);
        assert_eq!(wrap(&format!("x {word}"), 30, ""), format!("x\n{word}"));
    }
}
