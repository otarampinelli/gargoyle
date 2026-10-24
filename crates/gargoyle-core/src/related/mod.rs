//! related code: where the functions and types a diff touches are used, which tests use them
//! and which imports the diff adds, it's reference for the reviewer, never reviewed itself
//!
//! everything comes from git and plain text rules, no parser and no index,
//! so the same diff always gives the same context

mod imports;
mod symbols;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use gargoyle_git::{FileDiff, GitError, GrepHit, LineKind, Repo};
use globset::GlobSet;

use crate::planner::{BUILT_IN_EXCLUDES, estimate_tokens, glob_set};
use crate::{Config, RepoPath};

use imports::{ImportChange, import_changes, import_target, modules_imported};
use symbols::{Change, changed_symbols, definition_name, is_comment, is_identifier_char};

/// related code one task gets, the same as its code, about one token of context
/// per token of code like coderabbit, the cap only keeps a huge change from
/// making every prompt huge
pub(crate) const MAX_TOKENS: u32 = crate::planner::CODE_TOKENS;
/// a name with more uses than this is listed as a count only
const MAX_USES: usize = 60;
/// places listed per name, the rest are counted
const MAX_PLACES: usize = 8;
/// files read per folder to compare a new import with
const MAX_NEIGHBORS: usize = 200;

/// related code for a whole diff, collected once and rendered per task
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RelatedCode {
    symbols: Vec<SymbolUses>,
    imports: Vec<ImportNote>,
}

/// a function or type and every place that uses it
#[derive(Debug, Clone, PartialEq, Eq)]
struct SymbolUses {
    name: String,
    /// the changed file that defines it, none for a lookup by name
    path: Option<String>,
    change: Option<Change>,
    signature: Option<String>,
    definitions: Vec<Use>,
    uses: Vec<Use>,
    tests: Vec<Use>,
    /// more than `MAX_USES` places use it, so none are listed
    too_many: bool,
}

/// one line that uses a name
#[derive(Debug, Clone, PartialEq, Eq)]
struct Use {
    path: String,
    line: u32,
    function: Option<String>,
    text: String,
    /// the diff shows this line, so a task with this file already has it
    shown: bool,
    /// this change added the line
    added: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ImportNote {
    change: ImportChange,
    /// for an added import, how many other files in the same folder import the same module
    neighbors: usize,
}

/// the new side lines each file of the diff shows, and the ones it added
type Shown = BTreeMap<String, (BTreeSet<u32>, BTreeSet<u32>)>;

impl RelatedCode {
    /// looks up the related code of every reviewable file in the diff
    pub fn collect(repo: &Repo, files: &[FileDiff], config: &Config) -> Result<Self, GitError> {
        let exclude = excluded(config);
        let files: Vec<&FileDiff> = files
            .iter()
            .filter(|file| !file.binary && !exclude.is_match(file.path()))
            .collect();
        let changed = changed_symbols(&files);
        let names: Vec<&str> = changed.iter().map(|s| s.name.as_str()).collect();
        let hits = repo.grep_words(&names)?;
        let shown = shown_lines(&files);
        let mut symbols = Vec::new();
        for symbol in changed {
            let mut uses = uses_of(
                &symbol.name,
                Some(symbol.path.as_str()),
                &hits,
                &shown,
                &exclude,
            );
            uses.path = Some(symbol.path);
            uses.change = Some(symbol.change);
            uses.signature = symbol.signature;
            symbols.push(uses);
        }

        let imports = import_notes(repo, &files);
        Ok(Self { symbols, imports })
    }

    /// where each name is defined, used and tested, for `gargoyle context`
    pub fn lookup(repo: &Repo, names: &[String], config: &Config) -> Result<String, GitError> {
        let exclude = excluded(config);
        let words: Vec<&str> = names.iter().map(String::as_str).collect();
        let hits = repo.grep_words(&words)?;
        let mut out = String::new();
        for name in names {
            // compare with code in the language it's defined in
            let defined_in = hits
                .iter()
                .find(|hit| definition_name(&hit.text).as_deref() == Some(name.as_str()))
                .map(|hit| hit.path.as_str());
            let symbol = uses_of(name, defined_in, &hits, &Shown::new(), &exclude);
            out.push_str(&render_symbol(&symbol, &[]));
        }
        Ok(out)
    }

    /// the related code for a task on `paths`, in at most `max_tokens`,
    /// and the names left out to fit
    pub(crate) fn render(&self, paths: &[RepoPath], max_tokens: u32) -> (String, Vec<String>) {
        let in_task = |path: &str| paths.iter().any(|p| p.as_str() == path);
        let mut out = String::new();
        let mut left_out = Vec::new();

        let notes: Vec<&ImportNote> = self
            .imports
            .iter()
            .filter(|note| in_task(&note.change.path))
            .collect();
        let imports = render_imports(&notes);
        if estimate_tokens(&imports) <= max_tokens {
            out.push_str(&imports);
        } else {
            left_out.push("the imports".to_owned());
        }

        for symbol in &self.symbols {
            if !symbol.path.as_deref().is_some_and(in_task) {
                continue;
            }
            let card = render_symbol(symbol, paths);
            if estimate_tokens(&out) + estimate_tokens(&card) <= max_tokens {
                out.push_str(&card);
            } else {
                left_out.push(format!("`{}`", symbol.name));
            }
        }
        (out, left_out)
    }
}

/// every line in `hits` that uses `name`, split into definitions, uses and tests,
/// `family_of` is a file in the language to compare with, uses in other languages are dropped
fn uses_of(
    name: &str,
    family_of: Option<&str>,
    hits: &[GrepHit],
    shown: &Shown,
    exclude: &GlobSet,
) -> SymbolUses {
    let mut definitions = Vec::new();
    let mut uses = Vec::new();
    let mut tests = Vec::new();
    for hit in hits {
        if !contains_word(&hit.text, name)
            || exclude.is_match(&hit.path)
            || family_of.is_some_and(|path| family(path) != family(&hit.path))
            || is_comment(&hit.text)
            || import_target(&hit.text).is_some()
        {
            continue;
        }
        let (shown, added) = shown
            .get(&hit.path)
            .map_or((false, false), |(shown, added)| {
                (shown.contains(&hit.line), added.contains(&hit.line))
            });
        let found = Use {
            path: hit.path.clone(),
            line: hit.line,
            function: hit.function.clone(),
            text: hit.text.trim().to_owned(),
            shown,
            added,
        };
        if definition_name(&hit.text).as_deref() == Some(name) {
            definitions.push(found);
        } else if is_test(&hit.path, hit.function.as_deref()) {
            tests.push(found);
        } else {
            uses.push(found);
        }
    }
    let too_many = uses.len() + tests.len() > MAX_USES;
    if too_many {
        uses.clear();
        tests.clear();
    }
    SymbolUses {
        name: name.to_owned(),
        path: None,
        change: None,
        signature: None,
        definitions,
        uses,
        tests,
        too_many,
    }
}

/// one card: what changed, where it's used and which tests use it,
/// lines a task on `paths` already shows are left out
fn render_symbol(symbol: &SymbolUses, paths: &[RepoPath]) -> String {
    let in_task = |path: &str| paths.iter().any(|p| p.as_str() == path);
    let mut out = format!("### `{}`", symbol.name);
    if let (Some(path), Some(change)) = (&symbol.path, symbol.change) {
        let what = match change {
            Change::Removed => "removed from",
            Change::Signature => "signature changed in",
            Change::Added => "added in",
            Change::Body => "changed in",
        };
        let _ = write!(out, ", {what} `{path}`");
    }
    out.push_str("\n\n");
    if let Some(signature) = &symbol.signature {
        let _ = writeln!(out, "`{}`\n", shorten(signature, 160));
    }
    if symbol.change.is_none() {
        if symbol.definitions.is_empty() {
            out.push_str("No definition found, it may come from a dependency.\n");
        }
        for def in &symbol.definitions {
            let _ = writeln!(
                out,
                "Defined in `{}` line {}: `{}`",
                def.path,
                def.line,
                shorten(&def.text, 160)
            );
        }
    }

    if symbol.too_many {
        let _ = writeln!(
            out,
            "Used in more than {MAX_USES} places, too many to list.\n"
        );
        return out;
    }
    let hide = |found: &&Use| !(found.shown && in_task(&found.path));

    let uses: Vec<&Use> = symbol.uses.iter().filter(hide).collect();
    let removed = symbol.change == Some(Change::Removed);
    match (uses.len(), removed) {
        (0, true) => out.push_str("Nothing uses it anymore.\n"),
        (0, false) => out.push_str("Nothing outside the changes above uses it.\n"),
        (n, true) => {
            let _ = writeln!(out, "Still used in {n} {}:", places(n));
        }
        (n, false) => {
            let _ = writeln!(out, "Used in {n} {}:", places(n));
        }
    }
    out.push_str(&render_places(&uses));
    let tests: Vec<&Use> = symbol.tests.iter().filter(hide).collect();
    if tests.is_empty() {
        if symbol.change != Some(Change::Removed) {
            out.push_str("No test uses it.\n");
        }
    } else {
        let _ = writeln!(
            out,
            "Tests that use it, {} {}:",
            tests.len(),
            places(tests.len())
        );
        out.push_str(&render_places(&tests));
    }
    out.push('\n');
    out
}

/// one line per file and function, with the first use's code and the other line numbers
fn render_places(uses: &[&Use]) -> String {
    let mut groups: BTreeMap<(&str, Option<&str>), Vec<&Use>> = BTreeMap::new();
    for found in uses {
        groups
            .entry((found.path.as_str(), found.function.as_deref()))
            .or_default()
            .push(found);
    }
    let mut out = String::new();
    let total = groups.len();
    for ((path, function), group) in groups.into_iter().take(MAX_PLACES) {
        let first = group[0];
        let place = match function {
            Some(function) => format!("in `{}`", shorten(function, 80)),
            None => "at the top level".to_owned(),
        };
        let _ = write!(
            out,
            "- `{path}` {place}, line {}: `{}`",
            first.line,
            shorten(&first.text, 100)
        );
        if group.len() > 1 {
            let more: Vec<_> = group[1..].iter().map(|u| u.line.to_string()).collect();
            let _ = write!(out, ", also lines {}", more.join(", "));
        }
        if group.iter().any(|u| u.added) {
            out.push_str(" (added by this change)");
        }
        out.push('\n');
    }
    if total > MAX_PLACES {
        let _ = writeln!(out, "- and {} more", total - MAX_PLACES);
    }
    out
}

fn render_imports(notes: &[&ImportNote]) -> String {
    if notes.is_empty() {
        return String::new();
    }
    let mut out = String::from("### Imports this change adds or removes\n\n");
    let mut common: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for note in notes {
        let change = &note.change;
        let dir = folder(&change.path);
        if !change.added {
            let _ = writeln!(
                out,
                "- `{}` no longer imports `{}`",
                change.path, change.target
            );
        } else if note.neighbors == 0 {
            let _ = writeln!(
                out,
                "- `{}` now imports `{}`. No other file in `{dir}` imports from `{}`.",
                change.path, change.target, change.module
            );
        } else {
            common
                .entry(change.path.as_str())
                .or_default()
                .push(change.target.as_str());
        }
    }
    for (path, targets) in common {
        let targets: Vec<_> = targets.iter().map(|t| format!("`{t}`")).collect();
        let _ = writeln!(
            out,
            "- `{path}` also imports {}, like other files in `{}`",
            targets.join(", "),
            folder(path)
        );
    }
    out.push('\n');
    out
}

/// the import changes, each added one compared with the other files in its folder
fn import_notes(repo: &Repo, files: &[&FileDiff]) -> Vec<ImportNote> {
    let mut folders: BTreeMap<String, Vec<(String, BTreeSet<String>)>> = BTreeMap::new();
    let mut notes = Vec::new();
    for change in import_changes(files) {
        let mut neighbors = 0;
        if change.added {
            let dir = folder(&change.path).to_owned();
            let others = folders
                .entry(dir.clone())
                .or_insert_with(|| folder_imports(repo, &dir, &change.path));
            neighbors = others
                .iter()
                .filter(|(path, modules)| *path != change.path && modules.contains(&change.module))
                .count();
        }
        notes.push(ImportNote { change, neighbors });
    }
    notes
}

/// what each file directly in `dir` imports, files in the language of `like` only
fn folder_imports(repo: &Repo, dir: &str, like: &str) -> Vec<(String, BTreeSet<String>)> {
    // a folder we can't list just has nothing to compare with
    let paths = repo.files_in(dir).unwrap_or_default();
    paths
        .into_iter()
        .filter(|path| folder(path) == dir && family(path) == family(like))
        .take(MAX_NEIGHBORS)
        .filter_map(|path| {
            let text = std::fs::read_to_string(repo.root().join(&path)).ok()?;
            let modules = modules_imported(&path, &text);
            Some((path, modules))
        })
        .collect()
}

/// the new side lines each file shows in the diff, and the ones it added
fn shown_lines(files: &[&FileDiff]) -> Shown {
    let mut shown = Shown::new();
    for file in files {
        let (lines, added) = shown.entry(file.path().to_owned()).or_default();
        for hunk in &file.hunks {
            let mut number = hunk.new_start;
            for line in &hunk.lines {
                match line.kind {
                    LineKind::Removed => continue,
                    LineKind::Added => {
                        added.insert(number);
                    }
                    LineKind::Context => {}
                }
                lines.insert(number);
                number += 1;
            }
        }
    }
    shown
}

/// the built-in excludes and `files.exclude`, the config already checked the globs
fn excluded(config: &Config) -> GlobSet {
    let globs = BUILT_IN_EXCLUDES
        .iter()
        .map(|(glob, _)| *glob)
        .chain(config.files.exclude.iter().map(String::as_str));
    glob_set(globs).unwrap_or_else(|_| GlobSet::empty())
}

/// whether `text` has `word` with no letter, digit or `_` right before or after it
fn contains_word(text: &str, word: &str) -> bool {
    text.match_indices(word).any(|(i, _)| {
        let before = text[..i].chars().next_back();
        let after = text[i + word.len()..].chars().next();
        !before.is_some_and(is_identifier_char) && !after.is_some_and(is_identifier_char)
    })
}

/// files whose code can call each other, by extension
fn family(path: &str) -> &str {
    let file = path.rsplit('/').next().unwrap_or(path);
    let ext = file.rsplit_once('.').map_or("", |(_, ext)| ext);
    match ext {
        "js" | "jsx" | "mjs" | "cjs" | "ts" | "tsx" | "mts" | "cts" | "vue" | "svelte" => "js",
        "c" | "h" | "cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" => "c",
        "java" | "kt" | "kts" | "scala" => "jvm",
        "py" | "pyi" => "py",
        other => other,
    }
}

/// a test file, or a hit inside a test block like Rust's `mod tests`
fn is_test(path: &str, function: Option<&str>) -> bool {
    let lower = path.to_ascii_lowercase();
    let mut segments: Vec<&str> = lower.split('/').collect();
    let file = segments.pop().unwrap_or_default();
    let in_test_dir = segments.iter().any(|segment| {
        matches!(
            *segment,
            "test" | "tests" | "__tests__" | "spec" | "specs" | "e2e" | "testing"
        )
    });
    let test_file = [".test.", ".spec.", "_test.", "_spec.", "_tests."]
        .iter()
        .any(|marker| file.contains(marker))
        || file.starts_with("test_");
    let test_block = function.is_some_and(|f| {
        f.starts_with("mod tests") || f.starts_with("describe(") || f.starts_with("test(")
    });
    in_test_dir || test_file || test_block
}

/// the folder a file is in, empty for the repo root
fn folder(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

fn places(n: usize) -> &'static str {
    if n == 1 { "place" } else { "places" }
}

/// at most `max` characters, with `…` when cut
fn shorten(text: &str, max: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let cut: String = text.chars().take(max).collect();
    format!("{cut}…")
}
