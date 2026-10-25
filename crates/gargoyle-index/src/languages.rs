//! the languages gargoyle can index, each one a tree-sitter grammar and its tags query

use std::sync::OnceLock;

use tree_sitter::{Language, Query};
use tree_sitter_language::LanguageFn;

/// a language gargoyle can index
pub struct Grammar {
    /// the languages name, like "TypeScript"
    pub name: &'static str,
    /// languages whose code can call each other share a family, like TypeScript and JavaScript
    pub family: &'static str,
    language: LanguageFn,
    /// the grammars tags query, in parts when it builds on another languages
    tags: &'static [&'static str],
    query: OnceLock<Option<Query>>,
}

/// the grammar itself has no debug output, its name says which one its
impl std::fmt::Debug for Grammar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Grammar")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl Grammar {
    const fn new(
        name: &'static str,
        family: &'static str,
        language: LanguageFn,
        tags: &'static [&'static str],
    ) -> Self {
        Self {
            name,
            family,
            language,
            tags,
            query: OnceLock::new(),
        }
    }

    pub fn language(&self) -> Language {
        self.language.into()
    }

    /// the tags query, compiled the first time its needed,
    /// none when it doesn't compile, the the lang just isn't indexed
    pub fn query(&self) -> Option<&Query> {
        self.query
            .get_or_init(|| Query::new(&self.language(), &self.tags.concat()).ok())
            .as_ref()
    }
}

static C: Grammar = Grammar::new(
    "C",
    "c",
    tree_sitter_c::LANGUAGE,
    &[tree_sitter_c::TAGS_QUERY],
);
static CPP: Grammar = Grammar::new(
    "C++",
    "c",
    tree_sitter_cpp::LANGUAGE,
    &[tree_sitter_cpp::TAGS_QUERY],
);
static CSHARP: Grammar = Grammar::new(
    "C#",
    "csharp",
    tree_sitter_c_sharp::LANGUAGE,
    &[tree_sitter_c_sharp::TAGS_QUERY],
);
static ELIXIR: Grammar = Grammar::new(
    "Elixir",
    "elixir",
    tree_sitter_elixir::LANGUAGE,
    &[tree_sitter_elixir::TAGS_QUERY],
);
static GO: Grammar = Grammar::new(
    "Go",
    "go",
    tree_sitter_go::LANGUAGE,
    &[tree_sitter_go::TAGS_QUERY],
);
static JAVA: Grammar = Grammar::new(
    "Java",
    "java",
    tree_sitter_java::LANGUAGE,
    &[tree_sitter_java::TAGS_QUERY],
);
static JAVASCRIPT: Grammar = Grammar::new(
    "JavaScript",
    "js",
    tree_sitter_javascript::LANGUAGE,
    &[tree_sitter_javascript::TAGS_QUERY],
);
static LUA: Grammar = Grammar::new(
    "Lua",
    "lua",
    tree_sitter_lua::LANGUAGE,
    &[tree_sitter_lua::TAGS_QUERY],
);
static PHP: Grammar = Grammar::new(
    "PHP",
    "php",
    tree_sitter_php::LANGUAGE_PHP,
    &[tree_sitter_php::TAGS_QUERY],
);
static PYTHON: Grammar = Grammar::new(
    "Python",
    "python",
    tree_sitter_python::LANGUAGE,
    &[tree_sitter_python::TAGS_QUERY],
);
static RUBY: Grammar = Grammar::new(
    "Ruby",
    "ruby",
    tree_sitter_ruby::LANGUAGE,
    &[tree_sitter_ruby::TAGS_QUERY],
);
static RUST: Grammar = Grammar::new(
    "Rust",
    "rust",
    tree_sitter_rust::LANGUAGE,
    &[tree_sitter_rust::TAGS_QUERY],
);
static SWIFT: Grammar = Grammar::new(
    "Swift",
    "swift",
    tree_sitter_swift::LANGUAGE,
    &[tree_sitter_swift::TAGS_QUERY],
);
// typescrips query only has what javascript doesn't, so it goes on top of javascript
static TYPESCRIPT: Grammar = Grammar::new(
    "TypeScript",
    "js",
    tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
    &[
        tree_sitter_javascript::TAGS_QUERY,
        tree_sitter_typescript::TAGS_QUERY,
    ],
);
static TSX: Grammar = Grammar::new(
    "TSX",
    "js",
    tree_sitter_typescript::LANGUAGE_TSX,
    &[
        tree_sitter_javascript::TAGS_QUERY,
        tree_sitter_typescript::TAGS_QUERY,
    ],
);

/// the grammar for a file, by its extension, none when gargoyle cant index it
pub fn grammar_of(path: &str) -> Option<&'static Grammar> {
    let file = path.rsplit('/').next().unwrap_or(path);
    let (_, extension) = file.rsplit_once('.')?;
    let grammar = match extension.to_ascii_lowercase().as_str() {
        "c" | "h" => &C,
        "cc" | "cpp" | "cxx" | "c++" | "hh" | "hpp" | "hxx" | "h++" => &CPP,
        "cs" => &CSHARP,
        "ex" | "exs" => &ELIXIR,
        "go" => &GO,
        "java" => &JAVA,
        "js" | "jsx" | "mjs" | "cjs" => &JAVASCRIPT,
        "lua" => &LUA,
        "php" => &PHP,
        "py" | "pyi" => &PYTHON,
        "rb" | "rake" => &RUBY,
        "rs" => &RUST,
        "swift" => &SWIFT,
        "ts" | "mts" | "cts" => &TYPESCRIPT,
        "tsx" => &TSX,
        _ => return None,
    };
    Some(grammar)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tags_query_compiles() {
        let grammars = [
            &C,
            &CPP,
            &CSHARP,
            &ELIXIR,
            &GO,
            &JAVA,
            &JAVASCRIPT,
            &LUA,
            &PHP,
            &PYTHON,
            &RUBY,
            &RUST,
            &SWIFT,
            &TYPESCRIPT,
            &TSX,
        ];
        for grammar in grammars {
            let query = Query::new(&grammar.language(), &grammar.tags.concat());
            assert!(query.is_ok(), "{}: {:?}", grammar.name, query.err());
        }
    }

    #[test]
    fn picks_the_grammar_by_extension() {
        let name = |path| grammar_of(path).map(|g| g.name);
        assert_eq!(name("src/App.tsx"), Some("TSX"));
        assert_eq!(name("lib/cart.MJS"), Some("JavaScript"));
        assert_eq!(name("include/a.hpp"), Some("C++"));
        assert_eq!(name("Makefile"), None);
        assert_eq!(name("run.sh"), None);
        assert_eq!(name(".gitignore"), None);
    }

    #[test]
    fn typescript_and_javascript_share_a_family() {
        let family = |path| grammar_of(path).map(|g| g.family);
        assert_eq!(family("a.ts"), family("b.js"));
        assert_ne!(family("a.ts"), family("b.py"));
    }
}
