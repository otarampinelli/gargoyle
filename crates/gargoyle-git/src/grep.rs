//! finds where words are used with `git grep`, and the function each use is in

use crate::{GitError, Repo};

/// one line `git grep` found
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrepHit {
    pub path: String,
    pub line: u32,
    pub text: String,
    /// the line that starts the function this hit is in, by git's own rule,
    /// none for a hit at the top level of the file
    pub function: Option<String>,
}

impl Repo {
    /// every line in the working tree that uses one of `words` as a whole word,
    /// tracked or new files, binary files skipped, sorted by path and line
    pub fn grep_words(&self, words: &[&str]) -> Result<Vec<GrepHit>, GitError> {
        if words.is_empty() {
            return Ok(Vec::new());
        }
        let mut args = vec![
            "grep",
            "--untracked",
            "-I",
            "-n",
            "-w",
            "-F",
            // the function line of each hit, printed with `=` instead of `:`
            "-p",
            // the path on its own line, files split by an empty line,
            // so a `:` or `=` in a path can't be mistaken for a separator
            "--heading",
            "--break",
            "--no-color",
        ];
        for word in words {
            // -e keeps a word that starts with `-` from becoming a flag
            args.extend(["-e", word]);
        }
        // no match exits with 1, that's not a failure
        let out = self.git_with(&args, &[0, 1])?;
        Ok(parse_grep(&out))
    }

    /// tracked and new files under `dir`, sorted by path
    pub fn files_in(&self, dir: &str) -> Result<Vec<String>, GitError> {
        let dir = if dir.is_empty() { "." } else { dir };
        let out = self.git(&[
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
            "--",
            dir,
        ])?;
        let mut paths: Vec<String> = out
            .split('\0')
            .filter(|path| !path.is_empty() && !path.ends_with('/'))
            .map(str::to_owned)
            .collect();
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}

/// parses `git grep --heading --break -n -p` output
fn parse_grep(out: &str) -> Vec<GrepHit> {
    let mut hits = Vec::new();
    let mut path: Option<&str> = None;
    let mut function: Option<String> = None;
    for line in out.lines() {
        if line.is_empty() {
            path = None;
            continue;
        }
        let Some(current) = path else {
            path = Some(line);
            function = None;
            continue;
        };
        let Some((number, sep, text)) = split_numbered(line) else {
            continue;
        };
        if sep == '=' {
            function = Some(text.trim_end().to_owned());
            continue;
        }
        // a hit on a line that starts a function is at the top level,
        // and the hits after it are inside that function
        let starts_function = is_function_line(text);
        hits.push(GrepHit {
            path: current.to_owned(),
            line: number,
            text: text.to_owned(),
            function: if starts_function {
                None
            } else {
                function.clone()
            },
        });
        if starts_function {
            function = Some(text.trim_end().to_owned());
        }
    }
    hits
}

/// `12:text` or `12=text`
fn split_numbered(line: &str) -> Option<(u32, char, &str)> {
    let end = line.find(|c: char| !c.is_ascii_digit())?;
    let number = line[..end].parse().ok()?;
    let sep = line[end..].chars().next()?;
    matches!(sep, ':' | '=').then(|| (number, sep, &line[end + 1..]))
}

/// git's default rule for a function line: it starts with a letter, `_` or `$`
fn is_function_line(text: &str) -> bool {
    text.starts_with(|c: char| c.is_alphabetic() || c == '_' || c == '$')
}

#[cfg(test)]
mod tests {
    use crate::testing::TestRepo;

    #[test]
    fn finds_each_use_and_the_function_it_is_in() {
        let repo = TestRepo::new();
        repo.write(
            "src/a.rs",
            "fn total(a: u32) -> u32 {\n    let p = parse(a);\n    parse(p)\n}\n\n\
             impl Foo {\n    fn bar() {\n        parse(1);\n    }\n}\n",
        );
        repo.write("b.js", "const x = parse(1)\nconst parsed = 2\n");
        repo.commit("init");
        // new files count too
        repo.write("c.py", "y = parse(2)\n");

        let hits = repo.open().grep_words(&["parse"]).unwrap();
        let found: Vec<_> = hits
            .iter()
            .map(|h| (h.path.as_str(), h.line, h.function.as_deref()))
            .collect();
        assert_eq!(
            found,
            [
                ("b.js", 1, None),
                ("c.py", 1, None),
                ("src/a.rs", 2, Some("fn total(a: u32) -> u32 {")),
                ("src/a.rs", 3, Some("fn total(a: u32) -> u32 {")),
                ("src/a.rs", 8, Some("impl Foo {")),
            ]
        );
        assert_eq!(hits[2].text, "    let p = parse(a);");
    }

    #[test]
    fn no_match_is_no_hits() {
        let repo = TestRepo::new();
        repo.write("a.txt", "one\n");
        repo.commit("init");
        assert_eq!(repo.open().grep_words(&["missing"]).unwrap(), []);
        assert_eq!(repo.open().grep_words(&[]).unwrap(), []);
    }

    #[test]
    fn a_word_is_never_a_flag() {
        let repo = TestRepo::new();
        repo.write("a.txt", "--output=x\n");
        repo.commit("init");
        let hits = repo.open().grep_words(&["--output=x"]).unwrap();
        assert!(!repo.path().join("x").exists());
        assert!(hits.len() <= 1);
    }

    #[test]
    fn lists_the_files_in_a_folder() {
        let repo = TestRepo::new();
        repo.write("src/ui/a.ts", "a\n");
        repo.write("src/db/b.ts", "b\n");
        repo.commit("init");
        repo.write("src/ui/new.ts", "c\n");
        assert_eq!(
            repo.open().files_in("src/ui").unwrap(),
            ["src/ui/a.ts", "src/ui/new.ts"]
        );
    }
}
