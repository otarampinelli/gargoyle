//! the imports a diff adds or removes, read from import lines with one small rule per syntax

use std::collections::BTreeSet;

use gargoyle_git::{FileDiff, FileStatus, LineKind};

/// an import the diff adds or removes in one file
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ImportChange {
    pub(crate) path: String,
    /// what it imports, relative paths resolved from the repo root
    pub(crate) target: String,
    pub(crate) added: bool,
    /// the folder or package the target lives in, to compare with other files
    pub(crate) module: String,
}

/// the imports each changed file adds or removes, an import that only moved isn't a change
pub(crate) fn import_changes(files: &[&FileDiff]) -> Vec<ImportChange> {
    let mut changes = Vec::new();
    for file in files {
        if file.status() == FileStatus::Deleted {
            continue;
        }
        let path = file.path();
        let mut added = BTreeSet::new();
        let mut removed = BTreeSet::new();
        for line in file.hunks.iter().flat_map(|hunk| &hunk.lines) {
            let Some(target) = import_target(&line.text) else {
                continue;
            };
            match line.kind {
                LineKind::Added => added.insert(resolve(path, &target)),
                LineKind::Removed => removed.insert(resolve(path, &target)),
                LineKind::Context => false,
            };
        }
        for (targets, is_added) in [
            (added.difference(&removed), true),
            (removed.difference(&added), false),
        ] {
            for target in targets {
                changes.push(ImportChange {
                    path: path.to_owned(),
                    module: module_of(target),
                    target: target.clone(),
                    added: is_added,
                });
            }
        }
    }
    changes.sort();
    changes
}

/// the modules a whole file imports, to compare a new import with the rest of its folder
pub(crate) fn modules_imported(path: &str, text: &str) -> BTreeSet<String> {
    text.lines()
        .filter_map(import_target)
        .map(|target| module_of(&resolve(path, &target)))
        .collect()
}

/// what an import line points at, as written: `./db/orders`, `lodash`, `crate::db::Orders`,
/// `app.models`, `stdio.h`, none if the line isn't an import
pub(crate) fn import_target(line: &str) -> Option<String> {
    let text = line.trim();
    // `import x from "y"`, `export * from "y"`, `import "y"`, `import "fmt"` in Go
    if let Some(rest) = text
        .strip_prefix("import ")
        .or_else(|| text.strip_prefix("export "))
    {
        if let Some((_, from)) = rest.split_once(" from ") {
            return quoted(from);
        }
        if text.starts_with("import ")
            && let Some(target) = quoted(rest)
        {
            return Some(target);
        }
    }
    // `require("y")`, `await import("y")`, `require_relative "y"` in Ruby
    for call in ["require(", "import(", "require_relative ", "require "] {
        if let Some(at) = text.find(call) {
            let before = &text[..at];
            let is_word_start = before
                .chars()
                .last()
                .is_none_or(|c| !super::symbols::is_identifier_char(c) && c != '.');
            if is_word_start {
                return quoted(&text[at + call.len()..]);
            }
        }
    }
    // `#include "x.h"` or `#include <x.h>`
    if let Some(rest) = text.strip_prefix("#include") {
        let rest = rest.trim();
        return quoted(rest).or_else(|| {
            let inner = rest.strip_prefix('<')?.split('>').next()?;
            Some(inner.to_owned())
        });
    }
    // `from app.models import Order` in Python
    if let Some(rest) = text.strip_prefix("from ") {
        let (module, _) = rest.split_once(" import ")?;
        return Some(module.trim().to_owned());
    }
    // `import app.models`, `import java.util.List;`
    if let Some(rest) = text.strip_prefix("import ") {
        let rest = rest.strip_prefix("static ").unwrap_or(rest);
        let module = rest.split([',', ' ', ';']).next()?;
        let plain = !module.is_empty() && !module.contains(['"', '\'', '{', '*']);
        return (plain || module.ends_with(".*")).then(|| module.trim_end_matches(".*").to_owned());
    }
    // `use crate::db::{Orders, Price};` in Rust, `use App\Models\Order;` in PHP
    if let Some(rest) = text
        .strip_prefix("use ")
        .or_else(|| text.strip_prefix("pub use "))
    {
        let module = rest.split([';', '{', ' ']).next()?.trim_end_matches("::");
        return (!module.is_empty()).then(|| module.to_owned());
    }
    None
}

/// the text between the first pair of quotes
fn quoted(text: &str) -> Option<String> {
    let start = text.find(['"', '\'', '`'])?;
    let quote = text[start..].chars().next()?;
    let rest = &text[start + 1..];
    let end = rest.find(quote)?;
    let target = &rest[..end];
    (!target.is_empty()).then(|| target.to_owned())
}

/// a relative import as a path from the repo root without extension, anything else as is
fn resolve(importer: &str, target: &str) -> String {
    let dir: Vec<&str> = importer.split('/').collect();
    let dir = &dir[..dir.len().saturating_sub(1)];

    // `./x` and `../x` in JavaScript
    if target.starts_with("./") || target.starts_with("../") {
        let mut parts: Vec<&str> = dir.to_vec();
        for segment in target.split('/') {
            match segment {
                "." | "" => {}
                ".." => {
                    if parts.pop().is_none() {
                        return target.to_owned();
                    }
                }
                other => parts.push(other),
            }
        }
        let joined = parts.join("/");
        return strip_extension(&joined).to_owned();
    }
    // `.models` and `..db.orders` in Python
    if target.starts_with('.') {
        let dots = target.chars().take_while(|&c| c == '.').count();
        let Some(up) = dir.len().checked_sub(dots - 1) else {
            return target.to_owned();
        };
        let mut parts: Vec<&str> = dir[..up].to_vec();
        parts.extend(target[dots..].split('.').filter(|s| !s.is_empty()));
        return parts.join("/");
    }
    target.to_owned()
}

/// `src/db/orders` → `src/db`, `lodash/fp` → `lodash`, `crate::db::Orders` → `crate::db`,
/// `app.models.Order` → `app.models`, a single name stays as it is
fn module_of(target: &str) -> String {
    // `@scope/pkg/sub` is the package `@scope/pkg`
    if target.starts_with('@') {
        return target.splitn(3, '/').take(2).collect::<Vec<_>>().join("/");
    }
    let separators: [&str; 4] = ["::", "/", "\\", "."];
    for sep in separators {
        // `x.h` and `x.js` are files, not packages
        if sep == "." && (target.contains('/') || has_extension(target)) {
            continue;
        }
        if let Some((module, _)) = target.rsplit_once(sep)
            && !module.is_empty()
        {
            return module.to_owned();
        }
    }
    target.to_owned()
}

fn has_extension(target: &str) -> bool {
    target.rsplit_once('.').is_some_and(|(_, ext)| {
        matches!(
            ext,
            "h" | "hpp" | "js" | "ts" | "mjs" | "cjs" | "jsx" | "tsx" | "json" | "css" | "rb"
        )
    })
}

fn strip_extension(path: &str) -> &str {
    let file = path.rsplit('/').next().unwrap_or(path);
    match file.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => &path[..path.len() - (file.len() - stem.len())],
        _ => path,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gargoyle_git::parse_diff;

    #[test]
    fn reads_imports_in_many_languages() {
        let cases = [
            (
                "import { OrderRepo } from '../db/order_repo'",
                "../db/order_repo",
            ),
            ("import type { X } from \"./types\";", "./types"),
            ("export * from './api'", "./api"),
            ("import './styles.css'", "./styles.css"),
            ("const fs = require('node:fs')", "node:fs"),
            ("const m = await import(\"./lazy\")", "./lazy"),
            ("from app.models import Order", "app.models"),
            ("from . import utils", "."),
            ("import os", "os"),
            ("import java.util.List;", "java.util.List"),
            ("use crate::db::{Orders, Price};", "crate::db"),
            ("use std::fmt;", "std::fmt"),
            ("#include \"order.h\"", "order.h"),
            ("#include <stdio.h>", "stdio.h"),
            ("import \"net/http\"", "net/http"),
            ("require_relative 'models/order'", "models/order"),
        ];
        for (line, target) in cases {
            assert_eq!(import_target(line).as_deref(), Some(target), "{line}");
        }
        for line in [
            "const x = parse(1)",
            "// import foo from 'x'",
            "fn use_it() {}",
        ] {
            assert_eq!(import_target(line), None, "{line}");
        }
    }

    #[test]
    fn resolves_relative_imports_from_the_importer() {
        assert_eq!(
            resolve("src/ui/cart.tsx", "../db/order_repo"),
            "src/db/order_repo"
        );
        assert_eq!(resolve("src/ui/cart.tsx", "./button.js"), "src/ui/button");
        assert_eq!(resolve("app/api/views.py", "..models"), "app/models");
        assert_eq!(
            resolve("app/api/views.py", ".serializers"),
            "app/api/serializers"
        );
        assert_eq!(
            resolve("a.ts", "../../x"),
            "../../x",
            "can't go above the root"
        );
        assert_eq!(resolve("src/a.ts", "lodash"), "lodash");
    }

    #[test]
    fn groups_imports_by_module() {
        assert_eq!(module_of("src/db/order_repo"), "src/db");
        assert_eq!(module_of("lodash/fp"), "lodash");
        assert_eq!(module_of("lodash"), "lodash");
        assert_eq!(module_of("crate::db::Orders"), "crate::db");
        assert_eq!(module_of("app.models"), "app");
        assert_eq!(module_of("order.h"), "order.h");
        assert_eq!(module_of("@scope/pkg/sub"), "@scope/pkg");
    }

    #[test]
    fn lists_added_and_removed_imports_but_not_moved_ones() {
        let diff = "diff --git a/src/ui/cart.ts b/src/ui/cart.ts\n--- a/src/ui/cart.ts\n\
                    +++ b/src/ui/cart.ts\n@@ -1,3 +1,3 @@\n\
                    -import { a } from './a'\n\
                    -import { svc } from '../services/orders'\n\
                    +import { a } from './a'\n\
                    +import { OrderRepo } from '../db/order_repo'\n \n";
        let files = parse_diff(diff).unwrap();
        let changes = import_changes(&files.iter().collect::<Vec<_>>());
        let found: Vec<_> = changes
            .iter()
            .map(|c| (c.target.as_str(), c.added, c.module.as_str()))
            .collect();
        assert_eq!(
            found,
            [
                ("src/db/order_repo", true, "src/db"),
                ("src/services/orders", false, "src/services"),
            ]
        );
    }
}
