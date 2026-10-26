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
            .write_all(pretty(&report).as_bytes())?,
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

fn pretty(report: &Report) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "gargoyle · run {} · compared against {}\n",
        report.run_id, report.base.name
    );

    for reported in &report.findings {
        let finding = &reported.finding;
        let _ = writeln!(out, "{:<8} {}", finding.severity.as_str(), finding.title);
        let _ = writeln!(
            out,
            "         {} · {}",
            location(reported),
            checks(reported)
        );
        let _ = writeln!(out, "         {}", finding.message);
        if let Some(fix) = &finding.fix {
            let _ = writeln!(out, "         fix: {}", fix.description);
        }
        out.push('\n');
    }

    let passed: Vec<_> = report
        .checks
        .iter()
        .filter(|check| check.passed)
        .map(|check| check.check_id.as_str())
        .collect();
    if !passed.is_empty() {
        let _ = writeln!(out, "passed: {}", passed.join(", "));
    }

    match report.findings.len() {
        0 => out.push_str("no findings\n"),
        1 => {
            let _ = writeln!(out, "1 finding: {}", counts(report));
        }
        n => {
            let _ = writeln!(out, "{n} findings: {}", counts(report));
        }
    }

    for skipped in &report.skipped {
        let _ = writeln!(out, "skipped {}: {}", skipped.path, skipped.reason);
    }
    if report.low_confidence > 0 {
        let n = report.low_confidence;
        let noun = if n == 1 { "finding" } else { "findings" };
        let _ = writeln!(out, "left out {n} {noun} under min_confidence");
    }
    if !report.missing_tasks.is_empty() {
        let tasks: Vec<_> = report
            .missing_tasks
            .iter()
            .map(gargoyle_core::TaskId::as_str)
            .collect();
        let _ = writeln!(out, "incomplete, no answer yet for: {}", tasks.join(", "));
    }
    out
}
