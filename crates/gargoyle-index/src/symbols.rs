//! one file read with its grammar
//!  what it defines and the names it uses

use std::collections::{HashMap, HashSet};

use tree_sitter::{Node, Parser, Point, QueryCursor, StreamingIterator};

use serde::{Deserialize, Serialize};

use crate::Grammar;

// TODO: need to have a review about the max bytes value
/// files bigger than this are skipped, they're almost always generated or minified
const MAX_BYTES: usize = 1 << 20;

/// longest signature kept, in characters
const MAX_SIGNATURE: usize = 200;

/// what a definition is, from the tags query
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Function,
    Method,
    Class,
    Interface,
    Module,
    Type,
    Constant,
    Variable,
    Macro,
    Other,
}

impl Kind {
    fn from_tag(tag: &str) -> Self {
        match tag {
            "function" => Self::Function,
            "method" => Self::Method,
            "class" | "object" | "enum" => Self::Class,
            "interface" => Self::Interface,
            "module" => Self::Module,
            "type" => Self::Type,
            "constant" => Self::Constant,
            "variable" | "field" | "property" => Self::Variable,
            "macro" => Self::Macro,
            _ => Self::Other,
        }
    }

    /// functions and method, wat a line of code runs inside
    pub fn is_callable(self) -> bool {
        matches!(self, Self::Function | Self::Method | Self::Macro)
    }
}

/// something a file defines
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Definition {
    pub name: String,
    pub kind: Kind,
    /// first line from 1
    pub start: u32,
    /// last line, included
    pub end: u32,
    /// the definition up to its body on one line, like 'fn total(&self) -> u64'
    pub signature: String,
}

/// name a file usses, like a call or a type
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reference {
    pub name: String,
    pub line: u32,
}

/// what one file defines and uses, definitions sorted by where they start
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Symbols {
    pub definitions: Vec<Definition>,
    pub references: Vec<Reference>,
}

impl Symbols {
    /// the intermost func or method a line is in
    pub fn enclosing(&self, line: u32) -> Option<&Definition> {
        self.definitions
            .iter()
            .filter(|d| d.kind.is_callable() && d.start <= line && line <= d.end)
            .min_by_key(|d| d.end - d.start)
    }
}

/// reads a file with its grammar, empty when the file is too big or doesnt parse
pub fn parse(grammar: &Grammar, source: &str) -> Symbols {
    let mut symbols = Symbols::default();
    let Some(query) = grammar.query() else {
        return symbols;
    };

    if source.len() > MAX_BYTES {
        return symbols;
    }

    let mut parser = Parser::new();

    if parser.set_language(&grammar.language()).is_err() {
        return symbols;
    }

    let Some(tree) = parser.parse(source, None) else {
        return symbols;
    };

    let tags = query.capture_names();
    let bytes = source.as_bytes();

    // where each name starts, so a node two patterns match is kept once
    let mut defined: HashMap<usize, usize> = HashMap::new();
    let mut referenced: HashSet<usize> = HashSet::new();

    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, tree.root_node(), bytes);

    while let Some(found) = matches.next() {
        let mut name = None;
        let mut tagged = None;
        for capture in found.captures() {
            let Some(tag) = usize::try_from(capture.index)
                .ok()
                .and_then(|i| tags.get(i))
            else {
                continue;
            };
            if *tag == "name" {
                name = Some(capture.node);
            } else if let Some(split) = tag.split_once('.') {
                tagged = Some((split, capture.node));
            }
        }
        let (Some(name), Some(((role, tag), node))) = (name, tagged) else {
            continue;
        };
        let Ok(text) = name.utf8_text(bytes) else {
            continue;
        };

        match role {
            "definition" => {
                let kind = Kind::from_tag(tag);
                if let Some(&i) = defined.get(&name.start_byte()) {
                    // a rust method also matches the function pattern, method says more
                    if kind == Kind::Method {
                        symbols.definitions[i].kind = kind;
                    }
                    continue;
                }
                let node = whole_definition(node);
                defined.insert(name.start_byte(), symbols.definitions.len());
                symbols.definitions.push(Definition {
                    name: text.to_owned(),
                    kind,
                    start: line(node.start_position()),
                    end: line(node.end_position()),
                    signature: signature(node, source),
                });
            }
            "reference" if referenced.insert(name.start_byte()) => {
                symbols.references.push(Reference {
                    name: text.to_owned(),
                    line: line(name.start_position()),
                });
            }
            _ => {}
        }
    }

    symbols.definitions.sort_by_key(|d| (d.start, d.end));
    symbols
}

/// c and c++ tag a fucntion delcarator, the definition with its body is above it
fn whole_definition(mut node: Node<'_>) -> Node<'_> {
    while node.kind().ends_with("declarator") {
        match node.parent() {
            Some(parent) => node = parent,
            None => break,
        }
    }

    node
}

// everything before the body, or the first line when there is no body
fn signature(node: Node<'_>, source: &str) -> String {
    let head = match node.child_by_field_name("body") {
        Some(body) => source.get(node.start_byte()..body.start_byte()),
        None => source
            .get(node.byte_range())
            .and_then(|text| text.lines().next()),
    };

    let mut signature = head
        .unwrap_or_default()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    if let Some((cut, _)) = signature.char_indices().nth(MAX_SIGNATURE) {
        signature.truncate(cut);
        signature.push('…');
    }

    signature
}

fn line(point: Point) -> u32 {
    u32::try_from(point.row + 1).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar_of;

    fn parse_as(path: &str, source: &str) -> Symbols {
        parse(grammar_of(path).unwrap(), source)
    }

    fn defs(symbols: &Symbols) -> Vec<(&str, Kind, u32, u32)> {
        symbols
            .definitions
            .iter()
            .map(|d| (d.name.as_str(), d.kind, d.start, d.end))
            .collect()
    }

    fn uses(symbols: &Symbols, name: &str) -> Vec<u32> {
        symbols
            .references
            .iter()
            .filter(|r| r.name == name)
            .map(|r| r.line)
            .collect()
    }

    #[test]
    fn reads_rust_methods_and_their_callers() {
        let symbols = parse_as(
            "src/cart.rs",
            "struct Cart { items: Vec<u64> }\n\
             \n\
             impl Cart {\n    \
                 fn total(&self) -> u64 {\n        \
                     self.items.iter().sum()\n    \
                 }\n\
             }\n\
             \n\
             fn checkout(cart: &Cart) -> u64 {\n    \
                 cart.total()\n\
             }\n",
        );
        assert_eq!(
            defs(&symbols),
            [
                ("Cart", Kind::Class, 1, 1),
                ("total", Kind::Method, 4, 6),
                ("checkout", Kind::Function, 9, 11),
            ]
        );
        assert_eq!(symbols.definitions[1].signature, "fn total(&self) -> u64");
        assert_eq!(uses(&symbols, "total"), [10]);
        assert_eq!(
            symbols.enclosing(10).map(|d| d.name.as_str()),
            Some("checkout")
        );
        assert_eq!(symbols.enclosing(5).map(|d| d.name.as_str()), Some("total"));
        assert_eq!(symbols.enclosing(8), None);
    }

    #[test]
    fn c_functions_cover_their_body() {
        let symbols = parse_as(
            "src/math.c",
            "static int add(int a, int b) {\n    \
                 return a + b;\n\
             }\n\
             \n\
             int main(void) {\n    \
                 return add(1, 2);\n\
             }\n",
        );
        assert_eq!(
            defs(&symbols),
            [
                ("add", Kind::Function, 1, 3),
                ("main", Kind::Function, 5, 7),
            ]
        );
        assert_eq!(
            symbols.definitions[0].signature,
            "static int add(int a, int b)"
        );
        assert_eq!(uses(&symbols, "add"), [6]);
    }

    #[test]
    fn signatures_fit_on_one_line() {
        let symbols = parse_as(
            "cart.py",
            "class Cart:\n    \
                 def total(self,\n              \
                           tax):\n        \
                     return sum(self.items) * tax\n",
        );
        let signatures: Vec<_> = symbols
            .definitions
            .iter()
            .map(|d| d.signature.as_str())
            .collect();
        assert_eq!(signatures, ["class Cart:", "def total(self, tax):"]);
    }

    #[test]
    fn skips_files_too_big_to_be_source() {
        let source = "x = 1\n".repeat(MAX_BYTES / 6 + 1);
        assert_eq!(parse_as("big.py", &source), Symbols::default());
    }
}
