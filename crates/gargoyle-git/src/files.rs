//! the files of the working tree and what they held before, for the code index

use std::collections::BTreeSet;

use crate::{GitError, Repo};

/// a file in the working tree
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeFile {
    pub path: String,
    /// git's id for the file's content, none when the copy on disk differs from git
    /// or git doesn't track it yet
    pub blob: Option<String>,
}

impl Repo {
    /// every file git tracks or would track, sorted by path,
    /// nested repos, submodules and symlinks left out
    pub fn tree_files(&self) -> Result<Vec<TreeFile>, GitError> {
        // changed or new files on disk, their content isn't the one git has
        let changed = self.git(&[
            "ls-files",
            "-z",
            "--modified",
            "--others",
            "--exclude-standard",
        ])?;
        let changed: BTreeSet<&str> = changed
            .split('\0')
            .filter(|path| !path.is_empty() && !path.ends_with('/'))
            .collect();

        let staged = self.git(&["ls-files", "-z", "--stage"])?;
        let mut files: Vec<TreeFile> = Vec::new();

        for entry in staged.split('\0').filter(|entry| !entry.is_empty()) {
            // `<mode> <blob> <stage>\t<path>`
            let Some((meta, path)) = entry.split_once('\t') else {
                continue;
            };

            let mut meta = meta.split(' ');

            let (Some(mode), Some(blob)) = (meta.next(), meta.next()) else {
                continue;
            };

            // 100644 and 100755 are files, a conflict lists the same path more than once
            if !mode.starts_with("100") || files.last().is_some_and(|f| f.path == path) {
                continue;
            }

            files.push(TreeFile {
                path: path.to_owned(),
                blob: (!changed.contains(path)).then(|| blob.to_owned()),
            });
        }

        let tracked: BTreeSet<String> = files.iter().map(|f| f.path.clone()).collect();
        files.extend(
            changed
                .into_iter()
                .filter(|path| !tracked.contains(*path))
                .map(|path| TreeFile {
                    path: path.to_owned(),
                    blob: None,
                }),
        );

        files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(files)
    }

    /// a file as it was at `commit`, none when it didn't exist there
    pub fn show(&self, commit: &str, path: &str) -> Result<Option<String>, GitError> {
        let listed = self.git(&["ls-tree", "-z", "--end-of-options", commit, "--", path])?;
        if listed.is_empty() {
            return Ok(None);
        }

        let spec = format!("{commit}:{path}");
        self.git(&["cat-file", "blob", &spec]).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::TestRepo;

    #[test]
    fn lists_files_with_an_id_only_when_git_has_their_content() {
        let repo = TestRepo::new();
        repo.write("a.rs", "fn a() {}\n");
        repo.write("b.rs", "fn b() {}\n");
        repo.commit("init");
        repo.write("b.rs", "fn b2() {}\n");
        repo.write("c.rs", "fn c() {}\n");
        repo.write(".gitignore", "target/\n");
        repo.write("target/x.rs", "fn x() {}\n");

        let files = repo.open().tree_files().unwrap();
        let found: Vec<_> = files
            .iter()
            .map(|f| (f.path.as_str(), f.blob.is_some()))
            .collect();
        assert_eq!(
            found,
            [
                (".gitignore", false),
                ("a.rs", true),
                ("b.rs", false),
                ("c.rs", false)
            ]
        );
    }

    #[test]
    fn shows_a_file_as_it_was() {
        let repo = TestRepo::new();
        repo.write("a.rs", "fn a() {}\n");
        repo.commit("init");
        repo.write("a.rs", "fn changed() {}\n");

        let git = repo.open();
        let head = git.resolve("HEAD").unwrap().unwrap();
        assert_eq!(
            git.show(&head, "a.rs").unwrap().as_deref(),
            Some("fn a() {}\n")
        );
        assert_eq!(git.show(&head, "new.rs").unwrap(), None);
    }
}
