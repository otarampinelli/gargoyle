//! related code for one task: where the code it changes is used, the tests
//! that use it, and what the changes call, all found with the code index
//!
//! it's reference for the reviewer, never reviewed itself

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::{self, Write};

use gargoyle_git::{FileDiff, Hunk, LineKind};
use gargoyle_index::{Definition, Index, Symbols, grammar_of};

use crate::ContextConfig;
use crate::planner::estimate_tokens;

/// lines shown before and after a use
const AROUND: u32 = 3;

/// a name defined in more places than this is too common to tell which one the code calls
const MAX_SAME_NAME: usize = 3;

/// what related code is built from
pub struct Sources<'a> {
    pub index: &'a Index,
    /// what each changed file defined at the base, by its path there
    pub before: &'a BTreeMap<String, Symbols>,
    /// reads a file from the working tree, none when it's gone or isn't text
    pub read: &'a dyn Fn(&str) -> Option<String>,
}

// the index and the reader have no useful debug output
impl fmt::Debug for Sources<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Sources")
            .field("before", &self.before.keys())
            .finish_non_exhaustive()
    }
}

/// the hunks of one file a task shows
#[derive(Debug, Clone, Copy)]
pub(crate) struct Shown<'a> {
    pub(crate) path: &'a str,
    pub(crate) file: &'a FileDiff,
    pub(crate) hunks: &'a [Hunk],
}

/// how a definition changed, in the order it uses it
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Change {
    Removed,
    Signature,
    Body,
    Added,
}

/// a definition the task's changes touch
#[derive(Debug)]
struct Changed<'a> {
    path: &'a str,
    name: &'a str,
    change: Change,
    before: Option<&'a Definition>,
    after: Option<&'a Definition>,
}

/// a place outside the task's changes that u
#[derive(Debug)]
struct Use<'a> {
    path: &'a str,
    line: u32,
    within: Option<&'a Definition>,
    test: bool,
}

/// a changed definition, where it's used, an
#[derive(Debug)]
struct Block<'a> {
    changed: Changed<'a>,
    uses: Vec<Use<'a>>,
    picked: Vec<String>,
}

/// the related code section for one task, empty when there's nothing to show,
/// it gets at most `budget` tokens, the size of a task
pub(crate) fn related(
    sources: &Sources<'_>,
    shown: &[Shown<'_>],
    switches: &ContextConfig,
    budget: u32,
) -> String {
    let lines = ShownLines::new(shown);
    let mut files = HashMap::new();
    let mut left = budget;

    let changed = changed_definitions(sources, shown);
    let callees = callees(sources, shown, &changed, &lines);

    let (mut urgent, mut rest): (Vec<_>, Vec<_>) = changed
        .into_iter()
        .map(|changed| Block {
            uses: uses(sources, &changed, &lines, switches),
            changed,
            picked: Vec::new(),
        })
        .partition(|block| block.changed.change <= Change::Signature);

    // code that uses a removed or reshaped definition may be broken, it goes first,
    // then what the changes call, then the uses of definitions that kept their shape
    fill(&mut urgent, sources, &mut files, &mut left);

    let mut called = String::new();

    for line in callees {
        let cost = estimate_tokens(&line);

        if cost <= left {
            left -= cost;
            called.push_str(&line);
        }
    }

    fill(&mut rest, sources, &mut files, &mut left);

    let mut out = String::new();

    for block in urgent.iter().filter(|b| !b.picked.is_empty()) {
        out.push_str(&render_block(block));
    }

    if !called.is_empty() {
        let _ = write!(out, "### What the changes call\n\n{called}\n");
    }

    for block in rest.iter().filter(|b| !b.picked.is_empty()) {
        out.push_str(&render_block(block));
    }

    out
}

// one use from each block per round, so a common name can't take the whole budget
fn fill<'a>(
    blocks: &mut [Block<'a>],
    sources: &Sources<'a>,
    files: &mut HashMap<&'a str, Option<String>>,
    left: &mut u32,
) {
    for round in 0.. {
        let mut more = false;
        for block in blocks.iter_mut() {
            let Some(found) = block.uses.get(round) else {
                continue;
            };
            more = true;
            let text = files
                .entry(found.path)
                .or_insert_with(|| (sources.read)(found.path));
            let rendered = render_use(found, text.as_deref());
            let mut cost = estimate_tokens(&rendered);
            if block.picked.is_empty() {
                cost += estimate_tokens(&header(&block.changed));
            }
            if cost <= *left {
                *left -= cost;
                block.picked.push(rendered);
            }
        }
        if !more {
            break;
        }
    }
}

/// every definition the shown hunks add, remove or change, removed ones first
fn changed_definitions<'a>(sources: &Sources<'a>, shown: &[Shown<'a>]) -> Vec<Changed<'a>> {
    let mut changed: Vec<Changed<'a>> = Vec::new();
    for s in shown {
        if grammar_of(s.path).is_none() {
            continue;
        }
        let after = sources.index.file(s.path);
        let before = sources
            .before
            .get(s.file.old_path.as_deref().unwrap_or(s.path));
        let (added, removed) = changed_lines(s.hunks);

        let mut push = |before: Option<&'a Definition>, after: Option<&'a Definition>| {
            let Some(name) = after.or(before).map(|d| d.name.as_str()) else {
                return;
            };
            if changed.iter().any(|c| c.path == s.path && c.name == name) {
                return;
            }
            let change = match (before, after) {
                (None, _) => Change::Added,
                (_, None) => Change::Removed,
                (Some(b), Some(a)) if b.signature != a.signature => Change::Signature,
                _ => Change::Body,
            };
            changed.push(Changed {
                path: s.path,
                name,
                change,
                before,
                after,
            });
        };
        for a in touched(after, &added) {
            push(before.and_then(|b| same(b, a)), Some(a));
        }
        for b in touched(before, &removed) {
            push(Some(b), after.and_then(|a| same(a, b)));
        }
    }
    // stable, so the order of files stays within each kind of change
    changed.sort_by_key(|c| c.change);
    changed
}

/// where a changed name is used outside the task's changes, one use per function
fn uses<'a>(
    sources: &Sources<'a>,
    changed: &Changed<'a>,
    lines: &ShownLines<'_>,
    switches: &ContextConfig,
) -> Vec<Use<'a>> {
    let family = grammar_of(changed.path).map(|g| g.family);
    let mut seen = HashSet::new();
    let mut found = Vec::new();
    for (path, reference) in sources.index.references(changed.name) {
        if grammar_of(path).map(|g| g.family) != family || lines.contains(path, reference.line) {
            continue;
        }
        let symbols = sources.index.file(path);
        let within = symbols.and_then(|s| s.enclosing(reference.line));
        // a definition using its own name, like a recursive call
        if path == changed.path && within.is_some_and(|w| w.name == changed.name) {
            continue;
        }
        if !seen.insert((path, within.map_or(reference.line, |w| w.start))) {
            continue;
        }
        let test = is_test(path, symbols, reference.line);
        if (test && switches.tests) || (!test && switches.callers) {
            found.push(Use {
                path,
                line: reference.line,
                within,
                test,
            });
        }
    }
    found
}

/// the signatures of what the added lines call, from where it's defined
fn callees<'a>(
    sources: &Sources<'a>,
    shown: &[Shown<'a>],
    changed: &[Changed<'a>],
    lines: &ShownLines<'_>,
) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for s in shown {
        let Some(family) = grammar_of(s.path).map(|g| g.family) else {
            continue;
        };
        let Some(symbols) = sources.index.file(s.path) else {
            continue;
        };
        let (added, _) = changed_lines(s.hunks);
        for reference in &symbols.references {
            let name = reference.name.as_str();
            if added.binary_search(&reference.line).is_err()
                || changed.iter().any(|c| c.name == name)
                || !seen.insert(name)
            {
                continue;
            }
            let defined: Vec<_> = sources
                .index
                .definitions(name)
                .filter(|(path, d)| {
                    grammar_of(path).map(|g| g.family) == Some(family)
                        && !lines.overlaps(path, d.start, d.end)
                })
                .collect();
            if defined.len() > MAX_SAME_NAME {
                continue;
            }
            for (path, d) in defined {
                out.push(format!("- {path}:{}: `{}`\n", d.start, d.signature));
            }
        }
    }
    out
}

fn header(changed: &Changed<'_>) -> String {
    let (what, shape) = match changed.change {
        Change::Removed => ("removed", format!("It was: `{}`", sig(changed.before))),
        Change::Signature => (
            "signature changed",
            format!(
                "Before: `{}`\nNow: `{}`",
                sig(changed.before),
                sig(changed.after)
            ),
        ),
        Change::Body => ("body changed", format!("`{}`", sig(changed.after))),
        Change::Added => ("added", format!("`{}`", sig(changed.after))),
    };
    format!(
        "### `{}` in {}, {what}\n\n{shape}\n\n",
        changed.name, changed.path
    )
}

fn sig(definition: Option<&Definition>) -> &str {
    definition.map_or("", |d| d.signature.as_str())
}

fn render_block(block: &Block<'_>) -> String {
    let mut out = header(&block.changed);
    for picked in &block.picked {
        out.push_str(picked);
        out.push('\n');
    }
    let missed = &block.uses[block.picked.len().min(block.uses.len())..];
    if block.picked.len() < block.uses.len() {
        let mut paths: Vec<_> = missed.iter().map(|u| u.path).collect();
        paths.dedup();
        let count = block.uses.len() - block.picked.len();
        let uses = if count == 1 { "use" } else { "uses" };
        let _ = writeln!(
            out,
            "{count} more {uses} not shown, in {}\n",
            paths.join(", ")
        );
    }
    out
}

// the use with a few lines around it, kept inside the function it's in
fn render_use(found: &Use<'_>, text: Option<&str>) -> String {
    let kind = if found.test { "Tested in" } else { "Used in" };
    let mut out = match found.within {
        Some(within) => format!(
            "#### {kind} {}:{}, in `{}`\n",
            found.path, found.line, within.signature
        ),
        None => format!("#### {kind} {}:{}\n", found.path, found.line),
    };
    let first = found
        .line
        .saturating_sub(AROUND)
        .max(found.within.map_or(1, |w| w.start));
    let last = (found.line + AROUND).min(found.within.map_or(u32::MAX, |w| w.end));
    for (n, line) in (1u32..).zip(text.unwrap_or_default().lines()) {
        if n > last {
            break;
        }
        if n >= first {
            let _ = writeln!(out, "{n:>5}  {line}");
        }
    }
    out
}

/// the new lines a task's hunks add and the old lines they remove
fn changed_lines(hunks: &[Hunk]) -> (Vec<u32>, Vec<u32>) {
    let (mut added, mut removed) = (Vec::new(), Vec::new());
    for hunk in hunks {
        let (mut old, mut new) = (hunk.old_start, hunk.new_start);
        for line in &hunk.lines {
            match line.kind {
                LineKind::Context => {
                    old += 1;
                    new += 1;
                }
                LineKind::Added => {
                    added.push(new);
                    new += 1;
                }
                LineKind::Removed => {
                    removed.push(old);
                    old += 1;
                }
            }
        }
    }
    (added, removed)
}

/// the innermost definitions the lines fall in, each once
fn touched<'a>(symbols: Option<&'a Symbols>, lines: &[u32]) -> Vec<&'a Definition> {
    let Some(symbols) = symbols else {
        return Vec::new();
    };
    let mut found: Vec<&Definition> = Vec::new();
    for &line in lines {
        let inner = symbols
            .definitions
            .iter()
            .filter(|d| d.start <= line && line <= d.end)
            .min_by_key(|d| d.end - d.start);
        if let Some(d) = inner
            && !found.iter().any(|f| std::ptr::eq(*f, d))
        {
            found.push(d);
        }
    }
    found
}

/// the same definition in another version of the file, by name and kind
fn same<'a>(symbols: &'a Symbols, like: &Definition) -> Option<&'a Definition> {
    symbols
        .definitions
        .iter()
        .find(|d| d.name == like.name && d.kind == like.kind)
}

/// test code, by its path or by the function or module it's in
fn is_test(path: &str, symbols: Option<&Symbols>, line: u32) -> bool {
    let file = path.rsplit('/').next().unwrap_or(path);
    let stem = file.split('.').next().unwrap_or(file);
    let by_path = path.split('/').any(|dir| {
        matches!(
            dir.to_ascii_lowercase().as_str(),
            "test" | "tests" | "spec" | "specs" | "__tests__"
        )
    }) || stem.starts_with("test_")
        || stem.ends_with("_test")
        || stem.ends_with("_spec")
        || stem.ends_with("Test")
        || stem.ends_with("Tests")
        || file.contains(".test.")
        || file.contains(".spec.");
    by_path
        || symbols.is_some_and(|s| {
            s.definitions.iter().any(|d| {
                d.start <= line
                    && line <= d.end
                    && (d.name == "tests"
                        || d.name.starts_with("test_")
                        || d.name.starts_with("Test"))
            })
        })
}

/// the new lines each file shows in the task, uses there are already in its changes
#[derive(Debug, Default)]
struct ShownLines<'a>(HashMap<&'a str, Vec<(u32, u32)>>);

impl<'a> ShownLines<'a> {
    fn new(shown: &[Shown<'a>]) -> Self {
        let mut lines: HashMap<&str, Vec<(u32, u32)>> = HashMap::new();
        for s in shown {
            let ranges = lines.entry(s.path).or_default();
            for hunk in s.hunks.iter().filter(|h| h.new_lines > 0) {
                ranges.push((hunk.new_start, hunk.new_start + hunk.new_lines - 1));
            }
        }
        Self(lines)
    }

    fn contains(&self, path: &str, line: u32) -> bool {
        self.overlaps(path, line, line)
    }

    fn overlaps(&self, path: &str, start: u32, end: u32) -> bool {
        self.0
            .get(path)
            .is_some_and(|ranges| ranges.iter().any(|&(s, e)| s <= end && start <= e))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use gargoyle_git::{TreeFile, parse_diff};
    use gargoyle_index::parse;

    use super::*;

    /// a repo on disk: the files as they are now, the old version of the changed one,
    /// and the diff between them
    struct Fixture {
        dir: tempfile::TempDir,
        index: Index,
        before: BTreeMap<String, Symbols>,
        files: Vec<FileDiff>,
    }

    impl Fixture {
        fn new(now: &[(&str, &str)], changed: &str, old: &str, diff: &str) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let mut tree = Vec::new();
            for (path, text) in now {
                let full = dir.path().join(path);
                fs::create_dir_all(full.parent().unwrap()).unwrap();
                fs::write(full, text).unwrap();
                tree.push(TreeFile {
                    path: (*path).to_owned(),
                    blob: None,
                });
            }
            let (index, _) = Index::update(dir.path(), &tree).unwrap();
            let before =
                BTreeMap::from([(changed.to_owned(), parse(grammar_of(changed).unwrap(), old))]);
            Self {
                dir,
                index,
                before,
                files: parse_diff(diff).unwrap(),
            }
        }

        fn related(&self, switches: &ContextConfig, budget: u32) -> String {
            let root = self.dir.path().to_owned();
            let read = move |path: &str| fs::read_to_string(root.join(path)).ok();
            let sources = Sources {
                index: &self.index,
                before: &self.before,
                read: &read,
            };
            let shown: Vec<_> = self
                .files
                .iter()
                .map(|file| Shown {
                    path: file.path(),
                    file,
                    hunks: &file.hunks,
                })
                .collect();
            related(&sources, &shown, switches, budget)
        }
    }

    const OLD_CART: &str = "pub fn total(items: &[u64]) -> u64 {\n    items.iter().sum()\n}\n";
    const NEW_CART: &str =
        "pub fn total(items: &[u64], tax: u64) -> u64 {\n    items.iter().sum::<u64>() + tax\n}\n";
    const CART_DIFF: &str = "diff --git a/src/cart.rs b/src/cart.rs\n\
        --- a/src/cart.rs\n\
        +++ b/src/cart.rs\n\
        @@ -1,3 +1,3 @@\n\
        -pub fn total(items: &[u64]) -> u64 {\n\
        -    items.iter().sum()\n\
        +pub fn total(items: &[u64], tax: u64) -> u64 {\n\
        +    items.iter().sum::<u64>() + tax\n \
        }\n";
    const SHOP: &str = "fn pay() {\n    let paid = total(&[1]);\n    send(paid);\n}\n";
    // a call inside a rust macro isn't parsed as a call, so the test calls it outside one
    const CART_TEST: &str =
        "fn totals() {\n    let sum = total(&[]);\n    assert_eq!(sum, 0);\n}\n";

    fn cart() -> Fixture {
        Fixture::new(
            &[
                ("src/cart.rs", NEW_CART),
                ("src/shop.rs", SHOP),
                ("tests/cart.rs", CART_TEST),
            ],
            "src/cart.rs",
            OLD_CART,
            CART_DIFF,
        )
    }

    #[test]
    fn shows_who_uses_a_changed_signature() {
        let text = cart().related(&ContextConfig::default(), 10_000);
        assert!(
            text.starts_with(
                "### `total` in src/cart.rs, signature changed\n\n\
                 Before: `pub fn total(items: &[u64]) -> u64`\n\
                 Now: `pub fn total(items: &[u64], tax: u64) -> u64`\n\n"
            ),
            "{text}"
        );
        assert!(
            text.contains(
                "#### Used in src/shop.rs:2, in `fn pay()`\n\
                 \x20   1  fn pay() {\n\
                 \x20   2      let paid = total(&[1]);\n"
            ),
            "{text}"
        );
        assert!(
            text.contains("#### Tested in tests/cart.rs:2, in `fn totals()`"),
            "{text}"
        );
    }

    #[test]
    fn shows_who_uses_a_changed_body() {
        let new = "pub fn total(items: &[u64]) -> u64 {\n    items.iter().sum::<u64>() * 2\n}\n";
        let fixture = Fixture::new(
            &[("src/cart.rs", new), ("src/shop.rs", SHOP)],
            "src/cart.rs",
            OLD_CART,
            "diff --git a/src/cart.rs b/src/cart.rs\n\
             --- a/src/cart.rs\n\
             +++ b/src/cart.rs\n\
             @@ -1,3 +1,3 @@\n \
             pub fn total(items: &[u64]) -> u64 {\n\
             -    items.iter().sum()\n\
             +    items.iter().sum::<u64>() * 2\n \
             }\n",
        );
        let text = fixture.related(&ContextConfig::default(), 10_000);
        assert!(
            text.starts_with("### `total` in src/cart.rs, body changed\n"),
            "{text}"
        );
        assert!(text.contains("#### Used in src/shop.rs:2"), "{text}");
    }

    #[test]
    fn follows_the_context_switches() {
        let switches = ContextConfig {
            tests: false,
            ..ContextConfig::default()
        };
        let text = cart().related(&switches, 10_000);
        assert!(text.contains("Used in src/shop.rs"), "{text}");
        assert!(!text.contains("tests/cart.rs"), "{text}");
    }

    #[test]
    fn a_removed_function_shows_what_still_calls_it() {
        let fixture = Fixture::new(
            &[("src/cart.rs", "\n"), ("src/shop.rs", SHOP)],
            "src/cart.rs",
            OLD_CART,
            "diff --git a/src/cart.rs b/src/cart.rs\n\
             --- a/src/cart.rs\n\
             +++ b/src/cart.rs\n\
             @@ -1,3 +1,1 @@\n\
             -pub fn total(items: &[u64]) -> u64 {\n\
             -    items.iter().sum()\n\
             -}\n\
             +\n",
        );
        let text = fixture.related(&ContextConfig::default(), 10_000);
        assert!(
            text.starts_with(
                "### `total` in src/cart.rs, removed\n\n\
                 It was: `pub fn total(items: &[u64]) -> u64`\n\n\
                 #### Used in src/shop.rs:2"
            ),
            "{text}"
        );
    }

    #[test]
    fn lists_the_signatures_of_what_the_changes_call() {
        let fixture = Fixture::new(
            &[
                (
                    "src/shop.rs",
                    "fn pay() {\n    let paid = total(&[1]);\n    send(paid, 3);\n}\n",
                ),
                ("src/mail.rs", "pub fn send(amount: u64, retries: u32) {}\n"),
            ],
            "src/shop.rs",
            SHOP,
            "diff --git a/src/shop.rs b/src/shop.rs\n\
             --- a/src/shop.rs\n\
             +++ b/src/shop.rs\n\
             @@ -1,4 +1,4 @@\n \
             fn pay() {\n     \
             let paid = total(&[1]);\n\
             -    send(paid);\n\
             +    send(paid, 3);\n \
             }\n",
        );
        let text = fixture.related(&ContextConfig::default(), 10_000);
        assert!(
            text.contains(
                "### What the changes call\n\n\
                 - src/mail.rs:1: `pub fn send(amount: u64, retries: u32)`\n"
            ),
            "{text}"
        );
    }

    #[test]
    fn stays_within_the_budget() {
        let fixture = cart();
        let full = fixture.related(&ContextConfig::default(), 10_000);
        let small = fixture.related(&ContextConfig::default(), 90);
        assert!(estimate_tokens(&small) <= 90 + 20, "{small}");
        assert!(small.len() < full.len());
        assert!(
            small.contains("1 more use not shown, in tests/cart.rs"),
            "{small}"
        );
        assert_eq!(fixture.related(&ContextConfig::default(), 0), "");
    }

    #[test]
    fn tells_test_code_by_its_path() {
        for path in [
            "tests/cart.rs",
            "src/cart_test.go",
            "test_cart.py",
            "src/cart.test.ts",
            "spec/cart_spec.rb",
            "src/CartTest.java",
            "src/__tests__/cart.js",
        ] {
            assert!(is_test(path, None, 1), "{path}");
        }
        for path in ["src/cart.rs", "src/latest.rs", "src/contest.py"] {
            assert!(!is_test(path, None, 1), "{path}");
        }
    }
}
