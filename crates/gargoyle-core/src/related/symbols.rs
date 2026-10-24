//! the functions and types a diff adds, changes or removes,
//! found with one rule for every language instead of a parser per language

use std::collections::BTreeMap;

use gargoyle_git::{FileDiff, LineKind};

/// what the diff did to a definition, most important first
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Change {
    /// the definition is gone, anything still using it breaks
    Removed,
    /// the definition line changed, like new parameters
    Signature,
    /// a new definition
    Added,
    /// only the body changed
    Body,
}

/// a function or type the diff touches
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChangedSymbol {
    pub(crate) name: String,
    pub(crate) path: String,
    pub(crate) change: Change,
    /// the definition line, the old one for a removed definition
    pub(crate) signature: Option<String>,
}

/// most symbols a review looks up, the most important ones win
const MAX_SYMBOLS: usize = 40;

/// keywords that start a type definition
const TYPE_KEYWORDS: &[&str] = &[
    "class",
    "struct",
    "enum",
    "trait",
    "interface",
    "type",
    "record",
    "protocol",
    "union",
];

/// keywords that start a function definition
const FN_KEYWORDS: &[&str] = &["fn", "def", "func", "function", "fun", "sub", "proc"];

/// words that are never a definition's name, or come before a call instead of a definition
const NOT_NAMES: &[&str] = &[
    "if", "for", "while", "switch", "return", "catch", "else", "match", "new", "await", "throw",
    "typeof", "sizeof", "elif", "with", "in", "of", "and", "or", "not", "do", "case", "yield",
    "delete", "void", "assert", "print", "loop", "unless", "until", "when", "fn", "function",
];

/// names so common that their uses say nothing about one definition
const TOO_COMMON: &[&str] = &[
    "main",
    "new",
    "init",
    "__init__",
    "constructor",
    "default",
    "toString",
    "equals",
    "hashCode",
    "render",
    "setup",
    "run",
    "call",
    "get",
    "set",
    "test",
    "it",
    "describe",
    "from",
    "into",
    "fmt",
    "clone",
];

/// every function and type the diff touches, most important first, at most `MAX_SYMBOLS`
pub(crate) fn changed_symbols(files: &[&FileDiff]) -> Vec<ChangedSymbol> {
    let mut found: BTreeMap<String, ChangedSymbol> = BTreeMap::new();
    let mut removed: Vec<ChangedSymbol> = Vec::new();

    for file in files {
        let path = file.path();
        let mut added_defs: BTreeMap<String, String> = BTreeMap::new();
        let mut removed_defs: BTreeMap<String, String> = BTreeMap::new();
        let mut bodies: Vec<String> = Vec::new();

        for hunk in &file.hunks {
            // the function a changed line is in: the last definition above it in the hunk,
            // or the one git put in the hunk header
            let mut current = definition_name(&hunk.section);
            for line in &hunk.lines {
                let name = definition_name(&line.text);
                match line.kind {
                    LineKind::Context => {
                        if name.is_some() {
                            current = name;
                        }
                    }
                    LineKind::Added => {
                        if let Some(name) = name {
                            added_defs
                                .entry(name.clone())
                                .or_insert_with(|| line.text.trim().to_owned());
                            current = Some(name);
                        } else if let Some(name) = &current {
                            bodies.push(name.clone());
                        }
                    }
                    LineKind::Removed => {
                        if let Some(name) = name {
                            removed_defs
                                .entry(name)
                                .or_insert_with(|| line.text.trim().to_owned());
                        } else if let Some(name) = &current {
                            bodies.push(name.clone());
                        }
                    }
                }
            }
        }

        let symbol = |name: &str, change: Change, signature: Option<&String>| ChangedSymbol {
            name: name.to_owned(),
            path: path.to_owned(),
            change,
            signature: signature.cloned(),
        };
        for (name, signature) in &added_defs {
            let change = match removed_defs.get(name) {
                Some(old) if old == signature => Change::Body,
                Some(_) => Change::Signature,
                None => Change::Added,
            };
            keep(&mut found, symbol(name.as_str(), change, Some(signature)));
        }
        for (name, signature) in &removed_defs {
            if !added_defs.contains_key(name) {
                removed.push(symbol(name.as_str(), Change::Removed, Some(signature)));
            }
        }
        for name in &bodies {
            keep(&mut found, symbol(name.as_str(), Change::Body, None));
        }
    }

    // a definition removed from one file and added to another moved, its signature may still differ
    for symbol in removed {
        match found.get_mut(&symbol.name) {
            Some(moved) if moved.change == Change::Added => moved.change = Change::Signature,
            Some(_) => {}
            None => keep(&mut found, symbol),
        }
    }

    let mut symbols: Vec<_> = found.into_values().collect();
    symbols.sort_by(|a, b| (a.change, &a.path, &a.name).cmp(&(b.change, &b.path, &b.name)));
    symbols.truncate(MAX_SYMBOLS);
    symbols
}

/// keeps the most important change seen for a name
fn keep(found: &mut BTreeMap<String, ChangedSymbol>, symbol: ChangedSymbol) {
    if TOO_COMMON.contains(&symbol.name.as_str()) {
        return;
    }
    match found.get(&symbol.name) {
        Some(seen) if seen.change <= symbol.change => {}
        _ => {
            found.insert(symbol.name.clone(), symbol);
        }
    }
}

/// the name a line defines, like `parse` in `pub fn parse(`, `def parse(`,
/// `const parse = (x) =>` or `class Parser`, none for any other line
pub(crate) fn definition_name(line: &str) -> Option<String> {
    let text = strip_visibility(line.trim());
    if text.is_empty() || is_comment(text) {
        return None;
    }
    type_name(text)
        .or_else(|| assigned_function(text))
        .or_else(|| function_name(text))
        .filter(|name| name.len() >= 2 && !NOT_NAMES.contains(&name.as_str()))
}

/// `class Parser`, `pub struct Price`, `type Id = ...`
fn type_name(text: &str) -> Option<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let at = words.iter().position(|w| TYPE_KEYWORDS.contains(w))?;
    if !words[..at].iter().all(|w| is_identifier(w)) {
        return None;
    }
    // `enum class Color` in C++
    let name = words[at + 1..]
        .iter()
        .find(|w| !TYPE_KEYWORDS.contains(*w))?;
    let name = identifier_prefix(name);
    (!name.is_empty()).then(|| name.to_owned())
}

/// `const parse = (x) =>`, `export const parse = async function (`, `parse = (x) => {`
fn assigned_function(text: &str) -> Option<String> {
    let (left, right) = text.split_once(" = ")?;
    let right = right.trim_start();
    let right = right.strip_prefix("async ").unwrap_or(right).trim_start();
    let is_function = right.starts_with("function")
        || (right.starts_with('(') && right.contains("=>"))
        || right
            .split_once("=>")
            .is_some_and(|(param, _)| is_identifier(param.trim()));
    if !is_function {
        return None;
    }
    // `const parse: Parser = ...`
    let left = left.split(':').next().unwrap_or(left);
    let words: Vec<&str> = left.split_whitespace().collect();
    if !words.iter().all(|w| is_identifier(w)) {
        return None;
    }
    words.last().map(|name| (*name).to_owned())
}

/// `pub fn parse(`, `def total(`, `func (s *Server) Start(`, `public static int parse(`,
/// or a method like `parse(input) {`
fn function_name(text: &str) -> Option<String> {
    let open = text.find('(')?;
    let prefix = text[..open].trim_end();
    // a Go method: the receiver comes before the name
    if prefix == "func" {
        let close = matching_paren(text, open)?;
        let next = text[close + 1..].find('(')? + close + 1;
        let name = text[close + 1..next].trim();
        return (is_identifier(name) && matching_paren(text, next).is_some())
            .then(|| name.to_owned());
    }

    let words: Vec<&str> = prefix.split_whitespace().collect();
    let (last, before) = words.split_last()?;
    if !words.iter().all(|w| is_signature_word(w)) {
        return None;
    }
    if before
        .iter()
        .any(|w| NOT_NAMES.contains(w) && !FN_KEYWORDS.contains(w))
    {
        return None;
    }
    let name = last.trim_start_matches(['*', '&']);
    let name = name.split('<').next().unwrap_or(name);
    // `Foo::bar` in C++
    let name = name.rsplit("::").next().unwrap_or(name);
    if !is_identifier(name) {
        return None;
    }

    let has_keyword = before.iter().any(|w| FN_KEYWORDS.contains(w));
    if has_keyword {
        return Some(name.to_owned());
    }
    // no keyword, so it has to look like a whole signature that opens a body,
    // `fetch(url, {` is a call, `parse(input) {` is a method
    let close = matching_paren(text, open)?;
    let rest = text[close + 1..].trim();
    let opens_body = rest.ends_with('{') && !rest.contains(['(', '=', ';', ',']);
    opens_body.then(|| name.to_owned())
}

/// where the `(` at `open` closes
fn matching_paren(text: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (i, c) in text[open..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(open + i);
                }
            }
            _ => {}
        }
    }
    None
}

/// `pub(crate) fn x` becomes `fn x`
fn strip_visibility(text: &str) -> &str {
    if let Some(rest) = text.strip_prefix("pub(")
        && let Some(close) = rest.find(')')
    {
        return rest[close + 1..].trim_start();
    }
    text
}

pub(crate) fn is_comment(text: &str) -> bool {
    let text = text.trim_start();
    ["//", "#", "/*", "*", "--", ";", "<!--", "\"\"\"", "'''"]
        .iter()
        .any(|marker| text.starts_with(marker))
}

pub(crate) fn is_identifier(word: &str) -> bool {
    let mut chars = word.chars();
    chars
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '_' || c == '$')
        && chars.all(is_identifier_char)
}

pub(crate) fn is_identifier_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

fn identifier_prefix(word: &str) -> &str {
    let end = word
        .char_indices()
        .find(|&(_, c)| !is_identifier_char(c))
        .map_or(word.len(), |(i, _)| i);
    &word[..end]
}

/// a word that can be part of a signature before its `(`, like `static`, `Map<K,` or `*parse`
fn is_signature_word(word: &str) -> bool {
    word.chars().all(|c| {
        is_identifier_char(c) || matches!(c, '<' | '>' | ',' | '*' | '&' | ':' | '[' | ']')
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gargoyle_git::parse_diff;

    #[test]
    fn finds_definitions_in_many_languages() {
        let cases = [
            ("pub fn parse(input: &str) -> u32 {", "parse"),
            ("pub(crate) async fn load<T>(x: T) {", "load"),
            ("def total(items):", "total"),
            ("    def price(self, x):", "price"),
            (
                "func (s *Server) Start(ctx context.Context) error {",
                "Start",
            ),
            ("func Start() {", "Start"),
            (
                "export async function customJsResolver(id) {",
                "customJsResolver",
            ),
            ("function* walk(tree) {", "walk"),
            ("const parse = (x) => x * 2", "parse"),
            (
                "export const resolve = async (id: string): Promise<void> => {",
                "resolve",
            ),
            ("const handler: Handler = function (req) {", "handler"),
            ("  onClick = e => {", "onClick"),
            ("public static int parse(String s) {", "parse"),
            ("  parse(input) {", "parse"),
            ("  async resolveId(id, importer) {", "resolveId"),
            ("static int *make_buffer(size_t n) {", "make_buffer"),
            ("export default class OrderRepo extends Repo {", "OrderRepo"),
            ("pub struct Price {", "Price"),
            ("class Cart(Base):", "Cart"),
            ("type Id = string", "Id"),
            ("enum class Color {", "Color"),
        ];
        for (line, name) in cases {
            assert_eq!(definition_name(line).as_deref(), Some(name), "{line}");
        }
    }

    #[test]
    fn calls_and_statements_are_not_definitions() {
        for line in [
            "fetch(url, {",
            "    let p = parse(a);",
            "if (ready) {",
            "} else if (ready) {",
            "return parse(x)",
            "describe(\"parse\", () => {",
            "it(\"works\", async () => {",
            "foo.bar(x) {",
            "// fn parse(x) {",
            "# def total(x):",
            "const x = parse(1)",
            "this.handler = (x) => x",
            "await save(order) {",
            "",
        ] {
            assert_eq!(definition_name(line), None, "{line}");
        }
    }

    fn symbols(diff: &str) -> Vec<(String, Change)> {
        let files = parse_diff(diff).unwrap();
        let files: Vec<_> = files.iter().collect();
        changed_symbols(&files)
            .into_iter()
            .map(|s| (s.name, s.change))
            .collect()
    }

    /// a diff of one modified file, `lines` already have their `+`, `-` or space
    fn modified(path: &str, header: &str, lines: &[&str]) -> String {
        format!(
            "diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n{header}\n{}\n",
            lines.join("\n")
        )
    }

    #[test]
    fn tells_what_the_diff_did_to_each_definition() {
        let diff = modified(
            "price.ts",
            "@@ -1,7 +1,9 @@",
            &[
                "-export function toCents(x) {",
                "+export function toCents(x, round) {",
                "   return x * 100",
                " }",
                "-function oldHelper() {}",
                " function total(items) {",
                "-  return items.length",
                "+  return items.length + 1",
                " }",
                "+export function discount(x) {",
                "+  return x",
                "+}",
            ],
        );
        assert_eq!(
            symbols(&diff),
            [
                ("oldHelper".to_owned(), Change::Removed),
                ("toCents".to_owned(), Change::Signature),
                ("discount".to_owned(), Change::Added),
                ("total".to_owned(), Change::Body),
            ]
        );
    }

    #[test]
    fn a_body_change_uses_the_function_in_the_hunk_header() {
        let diff = modified(
            "a.py",
            "@@ -10,2 +10,2 @@ def total(items):",
            &["     x = 1", "-    return x", "+    return x + 1"],
        );
        assert_eq!(symbols(&diff), [("total".to_owned(), Change::Body)]);
    }

    #[test]
    fn a_definition_moved_to_another_file_is_not_removed() {
        let diff = [
            modified("a.js", "@@ -1 +0,0 @@", &["-function parse(x) {}"]),
            modified("b.js", "@@ -0,0 +1 @@", &["+function parse(x) {}"]),
        ]
        .concat();
        assert_eq!(symbols(&diff), [("parse".to_owned(), Change::Signature)]);
    }

    #[test]
    fn skips_names_too_common_to_trace() {
        let diff = modified("a.rs", "@@ -0,0 +1 @@", &["+fn main() {}"]);
        assert_eq!(symbols(&diff), []);
    }
}
