//! `gargoyle report`, merges every answer of a run into one report

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::io::{ErrorKind, Write as _};

use anyhow::Context;
use clap::ValueEnum;
use gargoyle_core::{Report, ReviewOutput, build_report};

use crate::exit::Exit;
use crate::markdown::{self, checks, counts, location};
use crate::output::print_json;
use crate::ui;
use crate::workspace::Workspace;

#[derive(Debug, Clone, Copy, Default, ValueEnum)]
pub(crate) enum Format {
    /// for people
    #[default]
    Pretty,
    /// for agents to show in chat: a table, then one section per finding
    #[value(alias = "md")]
    Markdown,
    /// for agents and scripts, see `gargoyle schema report`
    Json,
}

pub(crate) fn run(run: &str, format: Format) -> anyhow::Result<Exit> {
    let ws = Workspace::open()?;
    let plan = ws.load_plan(run)?;

    let mut results: BTreeMap<_, ReviewOutput> = BTreeMap::new();
    for task in &plan.tasks {
        let path = ws.path(&plan.run_id.result_path(&task.id));
        match fs::read_to_string(&path) {
            Ok(text) => {
                let output = serde_json::from_str(&text)
                    .with_context(|| format!("reading {}", path.display()))?;
                results.insert(task.id.clone(), output);
            }
            Err(err) if err.kind() == ErrorKind::NotFound => {}
            Err(err) => return Err(err).with_context(|| format!("reading {}", path.display())),
        }
    }

    // fingerprints hash the code around each finding as it is on disk now
    let report = build_report(&plan, &results, |path| {
        fs::read_to_string(ws.path(path.as_str())).ok()
    });
    let path = ws.path(&plan.run_id.report_path());
    let json = serde_json::to_string_pretty(&report)? + "\n";
    fs::write(&path, json).with_context(|| format!("writing {}", path.display()))?;

    match format {
        Format::Json => print_json(&report)?,
        Format::Pretty => std::io::stdout()
            .lock()
            .write_all(pretty(&report, ui::stdout_width()).as_bytes())?,
        Format::Markdown => std::io::stdout()
            .lock()
            .write_all(markdown::render(&report).as_bytes())?,
    }

    let fail_on = ws.config.review.fail_on;
    let fails = report
        .findings
        .iter()
        .any(|f| fail_on.fails(f.finding.severity));
    Ok(if fails { Exit::Findings } else { Exit::Clean })
}

/// the report for a terminal, `width` set when stdout is one: colors, a dot per
/// finding and text wrapped under its title, piped it's the plain text scripts read
fn pretty(report: &Report, width: Option<usize>) -> String {
    let fancy = width.is_some();
    let indent = if fancy { "           " } else { "         " };
    let wrap = |text: &str| match width {
        Some(width) => ui::wrap(text, width, indent),
        None => text.to_owned(),
    };
    let mut out = String::new();

    header(&mut out, report, fancy);

    for reported in &report.findings {
        let finding = &reported.finding;
        let severity = format!("{:<8}", finding.severity.as_str());
        if fancy {
            let style = ui::severity_style(finding.severity);
            let _ = writeln!(
                out,
                "{} {} {}",
                style.apply_to("●"),
                style.apply_to(severity),
                console::style(wrap(&finding.title)).bold()
            );
        } else {
            let _ = writeln!(out, "{severity} {}", finding.title);
        }
        let place = format!("{} · {}", location(reported), checks(reported));
        let _ = writeln!(out, "{indent}{}", console::style(place).dim());
        let _ = writeln!(out, "{indent}{}", wrap(&finding.message));
        if let Some(fix) = &finding.fix {
            // wrapped with its label, so the label's width counts on the first line
            let line = wrap(&format!("fix: {}", fix.description));
            let rest = line.strip_prefix("fix:").unwrap_or(&line);
            let _ = writeln!(out, "{indent}{}{rest}", console::style("fix:").green());
        }
        out.push('\n');
    }

    footer(&mut out, report, fancy);
    out
}

/// what was found and against what, the counts lead at a terminal
fn header(out: &mut String, report: &Report, fancy: bool) {
    if fancy {
        let found = match report.findings.len() {
            0 => "no problems".to_owned(),
            1 => format!("1 problem · {}", counts(report)),
            n => format!("{n} problems · {}", counts(report)),
        };
        let _ = writeln!(
            *out,
            "{} {}\n",
            console::style(format!("gargoyle · {found}")).bold(),
            console::style(format!(
                "· against {} · run {}",
                report.base.name, report.run_id
            ))
            .dim()
        );
    } else {
        let _ = writeln!(
            *out,
            "gargoyle · run {} · compared against {}\n",
            report.run_id, report.base.name
        );
    }
}

/// the checks that passed, then anything the review didn't cover
fn footer(out: &mut String, report: &Report, fancy: bool) {
    let passed: Vec<_> = report
        .checks
        .iter()
        .filter(|check| check.passed)
        .map(|check| check.check_id.as_str())
        .collect();
    if !passed.is_empty() {
        let _ = writeln!(
            *out,
            "{} {}",
            console::style("passed:").green(),
            passed.join(", ")
        );
    }

    // at a terminal the counts are already at the top
    if !fancy {
        match report.findings.len() {
            0 => out.push_str("no findings\n"),
            1 => {
                let _ = writeln!(*out, "1 finding: {}", counts(report));
            }
            n => {
                let _ = writeln!(*out, "{n} findings: {}", counts(report));
            }
        }
    }

    let dim = |text: String| console::style(text).dim().to_string();
    for skipped in &report.skipped {
        let _ = writeln!(
            *out,
            "{}",
            dim(format!("skipped {}: {}", skipped.path, skipped.reason))
        );
    }
    if report.low_confidence > 0 {
        let n = report.low_confidence;
        let noun = if n == 1 { "finding" } else { "findings" };
        let _ = writeln!(
            *out,
            "{}",
            dim(format!("left out {n} {noun} under min_confidence"))
        );
    }
    if !report.missing_tasks.is_empty() {
        let tasks: Vec<_> = report
            .missing_tasks
            .iter()
            .map(gargoyle_core::TaskId::as_str)
            .collect();
        let _ = writeln!(
            *out,
            "{} no answer yet for: {}",
            console::style("incomplete,").yellow(),
            tasks.join(", ")
        );
    }
}
