//! what of each changed file a reviewer sees: every definition the changes touch,
//! shown whole, and a one-line outline of the rest of the file
//!
//! a file the index doesn't cover keeps a few lines around each change, like a plain diff

use std::fmt::Write;
use std::ptr;

use gargoyle_git::{FileDiff, Hunk, Line, LineKind};
use gargoyle_index::{Definition, Symbols};

use crate::Sources;

/// lines kept around a change when the file isn't indexed, the same as `git diff`
const AROUND: usize = 3;

/// lines kept around a change outside every definition, like imports or top-level code
const LOOSE: usize = 10;

/// a changed file with each change widened to the definition it's in,
/// and the outline of what the file has besides
pub(crate) fn focus_file(file: &FileDiff, sources: Option<&Sources<'_>>) -> (FileDiff, String) {
    let after = sources.and_then(|s| s.index.file(file.path()));
    let before = sources.and_then(|s| s.before.get(file.old_path.as_deref()?));
    let hunks: Vec<Hunk> = file
        .hunks
        .iter()
        .flat_map(|hunk| focus(hunk, before, after))
        .collect();
    let outline = after.map(|a| outline(a, &hunks)).unwrap_or_default();
    let focused = FileDiff {
        old_path: file.old_path.clone(),
        new_path: file.new_path.clone(),
        binary: file.binary,
        hunks,
    };
    (focused, outline)
}

// a line of a hunk with its numbers in the old and the new file, and where it sits
// in each when it isn't in one of them
#[derive(Debug)]
struct Numbered<'a> {
    old: Option<u32>,
    new: Option<u32>,
    old_at: u32,
    new_at: u32,
    line: &'a Line,
}

fn number(hunk: &Hunk) -> Vec<Numbered<'_>> {
    // an empty side starts at the line before it
    let next = |start: u32, lines: u32| if lines == 0 { start + 1 } else { start };
    let mut old = next(hunk.old_start, hunk.old_lines);
    let mut new = next(hunk.new_start, hunk.new_lines);
    hunk.lines
        .iter()
        .map(|line| {
            let (has_old, has_new) = match line.kind {
                LineKind::Context => (true, true),
                LineKind::Added => (false, true),
                LineKind::Removed => (true, false),
            };
            let numbered = Numbered {
                old: has_old.then_some(old),
                new: has_new.then_some(new),
                old_at: old,
                new_at: new,
                line,
            };
            old += u32::from(has_old);
            new += u32::from(has_new);
            numbered
        })
        .collect()
}

///hunk cut down to the definitions its changes are in, with a few lines
/// around a change outside then
fn focus(hunk: &Hunk, before: Option<&Symbols>, after: Option<&Symbols>) -> Vec<Hunk> {
    let lines = number(hunk);
    let last = lines.len().saturating_sub(1);

    let mut ranges: Vec<(usize, usize, Option<&Definition>)> = Vec::new();
    for (i, numbered) in lines.iter().enumerate() {
        // a removed line was in the old file, an added one is in the new one
        let (symbols, line, new_side) = match numbered.line.kind {
            LineKind::Context => continue,
            LineKind::Added => (after, numbered.new, true),
            LineKind::Removed => (before, numbered.old, false),
        };
        let found = symbols.zip(line).and_then(|(s, line)| enclosing(s, line));
        let range = if let Some(d) = found {
            let inside = |n: &Numbered<'_>| {
                let line = if new_side { n.new } else { n.old };
                line.is_some_and(|line| d.start <= line && line <= d.end)
            };
            let start = lines.iter().position(inside).unwrap_or(i);
            let end = lines.iter().rposition(inside).unwrap_or(i);
            (start.min(i), end.max(i), Some(d))
        } else {
            let around = if symbols.is_some() { LOOSE } else { AROUND };
            (i.saturating_sub(around), (i + around).min(last), None)
        };
        ranges.push(range);
    }

    ranges.sort_by_key(|&(start, _, _)| start);
    let mut merged: Vec<(usize, usize, Option<&Definition>)> = Vec::new();
    for (start, end, shows) in ranges {
        match merged.last_mut() {
            // touching ranges become one, like git joins hunks whose context meets
            Some(last) if start <= last.1 + 1 => {
                last.1 = last.1.max(end);
                last.2 = last.2.or(shows);
            }
            _ => merged.push((start, end, shows)),
        }
    }
    merged
        .into_iter()
        .map(|(start, end, shows)| to_hunk(&lines[start..=end], hunk, shows))
        .collect()
}

/// the innermost definition a line is in, none when that's a class or module
/// with methods of its own, those are too big to show whole
fn enclosing(symbols: &Symbols, line: u32) -> Option<&Definition> {
    let inner = symbols
        .definitions
        .iter()
        .filter(|d| d.start <= line && line <= d.end)
        .min_by_key(|d| d.end - d.start)?;
    let holds_methods = symbols.definitions.iter().any(|d| {
        !ptr::eq(d, inner) && d.kind.is_callable() && inner.start <= d.start && d.end <= inner.end
    });
    (!holds_methods).then_some(inner)
}

fn to_hunk(lines: &[Numbered<'_>], from: &Hunk, shows: Option<&Definition>) -> Hunk {
    let count = |side: fn(&Numbered<'_>) -> Option<u32>| {
        u32::try_from(lines.iter().filter(|n| side(n).is_some()).count()).unwrap_or(u32::MAX)
    };
    let (old_lines, new_lines) = (count(|n| n.old), count(|n| n.new));
    // like git, an empty side starts at the line before it
    let start = |first: Option<u32>, at: u32| first.unwrap_or(at.saturating_sub(1));
    let first = lines.first();
    Hunk {
        old_start: start(
            lines.iter().find_map(|n| n.old),
            first.map_or(0, |n| n.old_at),
        ),
        old_lines,
        new_start: start(
            lines.iter().find_map(|n| n.new),
            first.map_or(0, |n| n.new_at),
        ),
        new_lines,
        section: shows.map_or_else(|| from.section.clone(), |d| d.signature.clone()),
        lines: lines.iter().map(|n| n.line.clone()).collect(),
    }
}

/// the definitions the hunks don't show, one line each: the top-level ones and
/// the ones next to a shown definition, so the reviewer knows what else is there
fn outline(after: &Symbols, hunks: &[Hunk]) -> String {
    let shown: Vec<(u32, u32)> = hunks
        .iter()
        .filter(|h| h.new_lines > 0)
        .map(|h| (h.new_start, h.new_start + h.new_lines - 1))
        .collect();
    let is_shown = |d: &Definition| shown.iter().any(|&(s, e)| s <= d.end && d.start <= e);

    let mut out = String::new();
    for d in &after.definitions {
        if is_shown(d) {
            continue;
        }
        let parent = after
            .definitions
            .iter()
            .filter(|p| !ptr::eq(*p, d) && p.start <= d.start && d.end <= p.end)
            .min_by_key(|p| p.end - p.start);
        if parent.is_none_or(is_shown) {
            let _ = writeln!(out, "- {}: `{}`", d.start, d.signature);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::process::Command;

    use gargoyle_git::parse_diff;
    use gargoyle_index::{grammar_of, parse};

    use super::*;

    /// the whole-file diff from `old` to `new`, made by git like a review's is
    fn full_diff(old: &str, new: &str) -> FileDiff {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a"), old).unwrap();
        fs::write(dir.path().join("b"), new).unwrap();
        let out = Command::new("git")
            .current_dir(dir.path())
            .args([
                "diff",
                "--no-index",
                "--no-color",
                "--unified=100000",
                "a",
                "b",
            ])
            .output()
            .unwrap();
        parse_diff(&String::from_utf8(out.stdout).unwrap())
            .unwrap()
            .remove(0)
    }

    fn rust(text: &str) -> Symbols {
        parse(grammar_of("x.rs").unwrap(), text)
    }

    /// the new lines each hunk shows
    fn shown(hunks: &[Hunk]) -> Vec<(u32, u32)> {
        hunks
            .iter()
            .map(|h| (h.new_start, h.new_start + h.new_lines - 1))
            .collect()
    }

    const OLD: &str = "use std::fmt;\n\
        \n\
        fn first() -> u32 {\n    \
            1\n\
        }\n\
        \n\
        fn second() -> u32 {\n    \
            let a = 1;\n    \
            let b = 2;\n    \
            a + b\n\
        }\n\
        \n\
        fn third() -> u32 {\n    \
            3\n\
        }\n";

    #[test]
    fn shows_the_whole_function_a_change_is_in() {
        let new = OLD.replace("let b = 2;", "let b = 20;");
        let file = full_diff(OLD, &new);
        let hunks: Vec<_> = file
            .hunks
            .iter()
            .flat_map(|h| focus(h, Some(&rust(OLD)), Some(&rust(&new))))
            .collect();
        assert_eq!(shown(&hunks), [(7, 11)]);
        assert_eq!(hunks[0].section, "fn second() -> u32");
        assert_eq!((hunks[0].old_start, hunks[0].old_lines), (7, 5));
    }

    #[test]
    fn a_removed_function_is_shown_from_the_old_file() {
        let new = OLD.replace("fn third() -> u32 {\n    3\n}\n", "");
        let file = full_diff(OLD, &new);
        let hunks = focus(&file.hunks[0], Some(&rust(OLD)), Some(&rust(&new)));
        assert_eq!(hunks.len(), 1);
        let removed = hunks[0]
            .lines
            .iter()
            .filter(|l| l.kind == LineKind::Removed)
            .count();
        assert_eq!(removed, 3);
        assert_eq!(hunks[0].old_start + hunks[0].old_lines - 1, 15);
    }

    #[test]
    fn a_change_outside_every_function_keeps_the_lines_around_it() {
        let new = OLD.replace("use std::fmt;", "use std::io;");
        let file = full_diff(OLD, &new);
        let hunks = focus(&file.hunks[0], Some(&rust(OLD)), Some(&rust(&new)));
        assert_eq!(shown(&hunks), [(1, 11)]);
    }

    #[test]
    fn without_the_index_it_is_a_plain_diff() {
        let new = OLD.replace("let b = 2;", "let b = 20;");
        let file = full_diff(OLD, &new);
        let hunks = focus(&file.hunks[0], None, None);
        assert_eq!(shown(&hunks), [(6, 12)]);
    }

    #[test]
    fn a_class_with_methods_isnt_shown_whole() {
        let python = |text: &str| parse(grammar_of("x.py").unwrap(), text);
        let old = "class Cart:\n    \
                   size = 1\n\
                   \n    \
                   def total(self):\n        \
                       return 1\n\
                   \n    \
                   def count(self):\n        \
                       return 2\n";
        let new = old.replace("size = 1", "size = 2");
        let file = full_diff(old, &new);
        let hunks = focus(&file.hunks[0], Some(&python(old)), Some(&python(&new)));
        // the attribute isn't in a method, so it keeps the lines around it, not the class
        assert_eq!(hunks[0].section, "");
    }

    #[test]
    fn outlines_what_the_file_has_besides_the_change() {
        let new = OLD.replace("let b = 2;", "let b = 20;");
        let file = full_diff(OLD, &new);
        let after = rust(&new);
        let hunks = focus(&file.hunks[0], Some(&rust(OLD)), Some(&after));
        assert_eq!(
            outline(&after, &hunks),
            "- 3: `fn first() -> u32`\n- 13: `fn third() -> u32`\n"
        );
    }

    #[test]
    fn a_new_file_keeps_its_numbers() {
        let new = "fn a() {}\n";
        let file = full_diff("", new);
        let hunks = focus(&file.hunks[0], None, Some(&rust(new)));
        assert_eq!(
            (hunks[0].old_start, hunks[0].old_lines, hunks[0].new_start),
            (0, 0, 1)
        );
    }
}
