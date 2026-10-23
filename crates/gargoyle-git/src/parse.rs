//! turns the unified diff git prints into files and hunks

/// one file in a diff
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiff {
    /// path before the change, none for a new file
    pub old_path: Option<String>,
    /// path after the change, none for a deleted file
    pub new_path: Option<String>,
    /// git only says binary files differ, so they have no hunks
    pub binary: bool,
    pub hunks: Vec<Hunk>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStatus {
    Added,
    Deleted,
    Modified,
    Renamed,
}

/// a block of changed lines with some context around them
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    /// text after the second `@@`, usually the enclosing function
    pub section: String,
    pub lines: Vec<Line>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub kind: LineKind,
    /// the line without its `+`, `-` or space prefix
    pub text: String,
    /// git printed `\ No newline at end of file` after it
    pub missing_newline: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Context,
    Added,
    Removed,
}

impl FileDiff {
    /// the path to show, the new one unless the file was deleted
    pub fn path(&self) -> &str {
        self.new_path
            .as_deref()
            .or(self.old_path.as_deref())
            .unwrap_or_default()
    }

    pub fn status(&self) -> FileStatus {
        match (&self.old_path, &self.new_path) {
            (None, _) => FileStatus::Added,
            (_, None) => FileStatus::Deleted,
            (Some(old), Some(new)) if old != new => FileStatus::Renamed,
            _ => FileStatus::Modified,
        }
    }

    /// how many lines were added and removed
    pub fn line_counts(&self) -> (usize, usize) {
        let lines = self.hunks.iter().flat_map(|hunk| &hunk.lines);
        lines.fold((0, 0), |(added, removed), line| match line.kind {
            LineKind::Added => (added + 1, removed),
            LineKind::Removed => (added, removed + 1),
            LineKind::Context => (added, removed),
        })
    }
}

/// the diff text doesn't look like git output, it's always a bug, never the user's fault
#[derive(Debug, thiserror::Error)]
#[error("invalid diff at line {line}: {message}")]
pub struct DiffParseError {
    pub line: usize,
    pub message: &'static str,
}

/// parses the output of `git diff`, an empty text gives no files
pub fn parse_diff(text: &str) -> Result<Vec<FileDiff>, DiffParseError> {
    let mut parser = Parser::new(text);
    let mut files = Vec::new();
    while let Some(line) = parser.next() {
        let Some(header) = line.strip_prefix("diff --git ") else {
            return Err(parser.error("expected `diff --git`"));
        };
        files.push(parser.file(header)?);
    }
    Ok(files)
}

struct Parser<'a> {
    lines: Vec<&'a str>,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(text: &'a str) -> Self {
        let mut lines: Vec<_> = text.split('\n').collect();
        // the text ends with a newline, so the last piece is empty
        if lines.last() == Some(&"") {
            lines.pop();
        }
        Self { lines, pos: 0 }
    }

    fn peek(&self) -> Option<&'a str> {
        self.lines.get(self.pos).copied()
    }

    fn next(&mut self) -> Option<&'a str> {
        let line = self.peek()?;
        self.pos += 1;
        Some(line)
    }

    /// an error pointing at the last line read, counting from 1
    fn error(&self, message: &'static str) -> DiffParseError {
        DiffParseError {
            line: self.pos,
            message,
        }
    }

    fn file(&mut self, header: &str) -> Result<FileDiff, DiffParseError> {
        // the header is enough for binary and mode only changes, the lines below win otherwise
        let (mut old_path, mut new_path) = match split_header(header) {
            Some((old, new)) => (Some(old), Some(new)),
            None => (None, None),
        };
        let mut binary = false;

        // extended headers, until the first hunk or the next file
        while let Some(line) = self.peek() {
            if line.starts_with("diff --git ") || line.starts_with("@@ ") {
                break;
            }
            self.pos += 1;
            if line.starts_with("new file mode ") {
                old_path = None;
            } else if line.starts_with("deleted file mode ") {
                new_path = None;
            } else if let Some(path) = line.strip_prefix("rename from ") {
                old_path = Some(unquote(path));
            } else if let Some(path) = line.strip_prefix("rename to ") {
                new_path = Some(unquote(path));
            } else if let Some(path) = line.strip_prefix("--- ") {
                old_path = side_path(path, "a/");
            } else if let Some(path) = line.strip_prefix("+++ ") {
                new_path = side_path(path, "b/");
            } else if line.starts_with("Binary files ") {
                binary = true;
            }
            // index, similarity and mode lines tell a review nothing
        }
        if old_path.is_none() && new_path.is_none() {
            return Err(self.error("could not find the file path"));
        }

        let mut hunks = Vec::new();
        while self.peek().is_some_and(|line| line.starts_with("@@ ")) {
            hunks.push(self.hunk()?);
        }
        Ok(FileDiff {
            old_path,
            new_path,
            binary,
            hunks,
        })
    }

    fn hunk(&mut self) -> Result<Hunk, DiffParseError> {
        let header = self.next().unwrap_or_default();
        let Some((old_start, old_lines, new_start, new_lines, section)) = parse_hunk_header(header)
        else {
            return Err(self.error("invalid hunk header"));
        };

        // the header says how many lines follow, that's safer than guessing from prefixes
        let (mut old_left, mut new_left) = (old_lines, new_lines);
        let mut lines: Vec<Line> = Vec::new();
        while old_left > 0 || new_left > 0 {
            let Some(line) = self.next() else {
                return Err(self.error("the diff ends inside a hunk"));
            };
            if line.starts_with('\\') {
                mark_missing_newline(&mut lines);
                continue;
            }
            // some tools strip the space off empty context lines
            let (prefix, text) = line.split_at_checked(1).unwrap_or((" ", ""));
            let (kind, old_step, new_step) = match prefix {
                " " => (LineKind::Context, 1, 1),
                "+" => (LineKind::Added, 0, 1),
                "-" => (LineKind::Removed, 1, 0),
                _ => return Err(self.error("unexpected line inside a hunk")),
            };
            let (Some(old), Some(new)) = (
                old_left.checked_sub(old_step),
                new_left.checked_sub(new_step),
            ) else {
                return Err(self.error("the hunk has more lines than its header says"));
            };
            (old_left, new_left) = (old, new);
            lines.push(Line {
                kind,
                text: text.to_owned(),
                missing_newline: false,
            });
        }
        // the marker for the last line comes after the line count runs out
        if self.peek().is_some_and(|line| line.starts_with('\\')) {
            self.pos += 1;
            mark_missing_newline(&mut lines);
        }

        Ok(Hunk {
            old_start,
            old_lines,
            new_start,
            new_lines,
            section: section.to_owned(),
            lines,
        })
    }
}

fn mark_missing_newline(lines: &mut [Line]) {
    if let Some(last) = lines.last_mut() {
        last.missing_newline = true;
    }
}

/// `@@ -1,3 +1,4 @@ fn main` into its numbers and section
fn parse_hunk_header(header: &str) -> Option<(u32, u32, u32, u32, &str)> {
    let rest = header.strip_prefix("@@ -")?;
    let (old, rest) = rest.split_once(" +")?;
    let (new, section) = rest.split_once(" @@")?;
    let (old_start, old_lines) = parse_range(old)?;
    let (new_start, new_lines) = parse_range(new)?;
    let section = section.strip_prefix(' ').unwrap_or(section);
    Some((old_start, old_lines, new_start, new_lines, section))
}

/// `3,4` or just `3`, git leaves the count out when it's 1
fn parse_range(range: &str) -> Option<(u32, u32)> {
    match range.split_once(',') {
        Some((start, count)) => Some((start.parse().ok()?, count.parse().ok()?)),
        None => Some((range.parse().ok()?, 1)),
    }
}

/// the path on a `---` or `+++` line, none for /dev/null
fn side_path(path: &str, prefix: &str) -> Option<String> {
    // git adds a tab after paths that contain a space
    let path = path.strip_suffix('\t').unwrap_or(path);
    if path == "/dev/null" {
        return None;
    }
    let path = unquote(path);
    Some(
        path.strip_prefix(prefix)
            .map_or_else(|| path.clone(), str::to_owned),
    )
}

/// both paths from `diff --git a/x b/x`
fn split_header(header: &str) -> Option<(String, String)> {
    let (old, new) = if header.starts_with('"') {
        let (old, rest) = take_quoted(header)?;
        (old, unquote(rest.strip_prefix(' ')?))
    } else {
        // the paths only differ on renames, and those have their own lines,
        // so split in the middle, which also works when paths contain spaces
        let middle = header.len().checked_sub(1)? / 2;
        if header.as_bytes().get(middle) != Some(&b' ') {
            return None;
        }
        (
            header.get(..middle)?.to_owned(),
            header.get(middle + 1..)?.to_owned(),
        )
    };
    Some((
        old.strip_prefix("a/")?.to_owned(),
        new.strip_prefix("b/")?.to_owned(),
    ))
}

/// a path as is, or a C style quoted one with its escapes undone
fn unquote(path: &str) -> String {
    match take_quoted(path) {
        Some((path, "")) => path,
        _ => path.to_owned(),
    }
}

/// reads one `"..."` path off the front of `text`, returns it and what follows
fn take_quoted(text: &str) -> Option<(String, &str)> {
    let mut bytes = Vec::new();
    let mut chars = text.strip_prefix('"')?.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '"' => {
                let rest = &text[i + 2..];
                return Some((String::from_utf8_lossy(&bytes).into_owned(), rest));
            }
            '\\' => {
                let (_, escaped) = chars.next()?;
                let byte = match escaped {
                    'n' => b'\n',
                    't' => b'\t',
                    '"' => b'"',
                    '\\' => b'\\',
                    // non ascii bytes come as three octal digits, like \303
                    '0'..='7' => {
                        let mut value = escaped.to_digit(8)?;
                        for _ in 0..2 {
                            value = value * 8 + chars.next()?.1.to_digit(8)?;
                        }
                        u8::try_from(value).ok()?
                    }
                    _ => return None,
                };
                bytes.push(byte);
            }
            c => bytes.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes()),
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TestRepo;

    fn parse_one(text: &str) -> FileDiff {
        let mut files = parse_diff(text).unwrap();
        assert_eq!(files.len(), 1, "{files:?}");
        files.remove(0)
    }

    #[test]
    fn empty_diff_has_no_files() {
        assert_eq!(parse_diff("").unwrap(), []);
    }

    #[test]
    fn parses_a_modified_file() {
        use LineKind::{Added, Context, Removed};

        let file = parse_one(
            "diff --git a/src/lib.rs b/src/lib.rs\n\
             index 1111111..2222222 100644\n\
             --- a/src/lib.rs\n\
             +++ b/src/lib.rs\n\
             @@ -1,3 +1,3 @@ fn main() {\n \
             one\n\
             -two\n\
             +TWO\n \
             three\n",
        );
        assert_eq!(file.path(), "src/lib.rs");
        assert_eq!(file.status(), FileStatus::Modified);
        assert_eq!(file.line_counts(), (1, 1));

        let hunk = &file.hunks[0];
        assert_eq!(
            (
                hunk.old_start,
                hunk.old_lines,
                hunk.new_start,
                hunk.new_lines
            ),
            (1, 3, 1, 3)
        );
        assert_eq!(hunk.section, "fn main() {");
        let kinds: Vec<_> = hunk.lines.iter().map(|line| line.kind).collect();
        assert_eq!(kinds, [Context, Removed, Added, Context]);
        assert_eq!(hunk.lines[2].text, "TWO");
    }

    #[test]
    fn counts_default_to_one() {
        let file = parse_one(
            "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1 +1 @@\n-one\n+two\n",
        );
        let hunk = &file.hunks[0];
        assert_eq!((hunk.old_lines, hunk.new_lines), (1, 1));
    }

    #[test]
    fn marks_a_missing_newline_at_the_end() {
        let file = parse_one(
            "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1 +1 @@\n\
             -one\n\\ No newline at end of file\n+two\n\\ No newline at end of file\n",
        );
        let lines = &file.hunks[0].lines;
        assert!(lines.iter().all(|line| line.missing_newline), "{lines:?}");
    }

    #[test]
    fn reads_quoted_paths() {
        let file = parse_one(
            "diff --git \"a/tab\\there\" \"b/tab\\there\"\n\
             new file mode 100644\n\
             --- /dev/null\n\
             +++ \"b/tab\\there\"\n\
             @@ -0,0 +1 @@\n+hi\n",
        );
        assert_eq!(file.path(), "tab\there");
        assert_eq!(file.status(), FileStatus::Added);
    }

    #[test]
    fn rejects_a_hunk_that_ends_early() {
        let err = parse_diff("diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -1,2 +1,2 @@\n one\n")
            .unwrap_err();
        assert_eq!(err.message, "the diff ends inside a hunk");
    }

    #[test]
    fn rejects_text_that_isnt_a_diff() {
        assert!(parse_diff("hello\n").is_err());
    }

    /// the real test, every kind of change straight from git
    #[test]
    fn parses_real_git_output() {
        use FileStatus::{Added, Deleted, Modified, Renamed};

        let repo = TestRepo::new();
        repo.write("modified.txt", "one\ntwo\nthree\n");
        repo.write("deleted.txt", "bye\n");
        repo.write(
            "old name.txt",
            "same content\nfor the rename\nto be found\n",
        );
        repo.write("no-newline.txt", "end");
        repo.commit("init");
        let git = repo.open();
        let base = git.resolve("HEAD").unwrap().unwrap();

        repo.write("modified.txt", "one\n2\nthree\n");
        std::fs::remove_file(repo.path().join("deleted.txt")).unwrap();
        repo.git(&["mv", "old name.txt", "new name.txt"]);
        repo.write("no-newline.txt", "the end");
        repo.write("src/added.rs", "fn main() {}\n");
        repo.write("café.txt", "non ascii path\n");
        std::fs::write(repo.path().join("image.bin"), [0u8, 1, 2]).unwrap();

        let files = parse_diff(&git.diff(&base).unwrap()).unwrap();
        let summary: Vec<_> = files
            .iter()
            .map(|f| (f.path(), f.status(), f.binary))
            .collect();
        assert_eq!(
            summary,
            [
                ("deleted.txt", Deleted, false),
                ("modified.txt", Modified, false),
                ("new name.txt", Renamed, false),
                ("no-newline.txt", Modified, false),
                ("café.txt", Added, false),
                ("image.bin", Added, true),
                ("src/added.rs", Added, false),
            ]
        );
        let renamed = &files[2];
        assert_eq!(renamed.old_path.as_deref(), Some("old name.txt"));
        assert_eq!(renamed.hunks.len(), 0);
        assert!(
            files[3].hunks[0]
                .lines
                .iter()
                .all(|line| line.missing_newline)
        );
    }
}
