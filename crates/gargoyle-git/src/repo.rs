use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::GitError;

/// a git repo, every command runs from its root
#[derive(Debug, Clone)]
pub struct Repo {
    root: PathBuf,
}

// fixed output no matter what the user has in their git config
const DIFF_ARGS: [&str; 9] = [
    "diff",
    "--no-color",
    "--no-ext-diff",
    "--no-textconv",
    "--no-relative",
    "--find-renames",
    "--unified=3",
    "--src-prefix=a/",
    "--dst-prefix=b/",
];

impl Repo {
    /// finds the repo that contains `dir`
    pub fn discover(dir: &Path) -> Result<Self, GitError> {
        match run(dir, &["rev-parse", "--show-toplevel"], &[0]) {
            Ok(out) => Ok(Self {
                root: PathBuf::from(out.trim_end()),
            }),
            Err(GitError::Failed { .. }) => Err(GitError::NotARepo(dir.to_owned())),
            Err(err) => Err(err),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// the commit `rev` points to, or none if it doesn't exist
    pub fn resolve(&self, rev: &str) -> Result<Option<String>, GitError> {
        // --end-of-options so a base like `--output=x` from a config can't become a flag
        let rev = format!("{rev}^{{commit}}");

        match self.git(&["rev-parse", "--verify", "--quiet", "--end-of-options", &rev]) {
            Ok(sha) => Ok(Some(sha.trim_end().to_owned())),
            Err(GitError::Failed { .. }) => Ok(None),
            Err(err) => Err(err),
        }
    }

    /// the last commit `base` and HEAD have in common
    pub fn merge_base(&self, base: &str) -> Result<String, GitError> {
        match self.git(&["merge-base", "--end-of-options", base, "HEAD"]) {
            Ok(sha) => Ok(sha.trim_end().to_owned()),
            Err(GitError::Failed { .. }) => Err(GitError::NoCommonHistory(base.to_owned())),
            Err(err) => Err(err),
        }
    }

    /// everything that differs from `commit`, committed or not, tracked or not,
    /// empty when nothing changed
    pub fn diff(&self, commit: &str) -> Result<String, GitError> {
        // no second commit, so git compares against the files on disk
        let mut args = DIFF_ARGS.to_vec();
        args.extend(["--end-of-options", commit]);
        let mut diff = self.git(&args)?;

        // git diff never shows new files git doesn't know about yet
        for path in self.untracked()? {
            let mut args = DIFF_ARGS.to_vec();
            args.extend(["--no-index", "--", "/dev/null", &path]);
            // --no-index exits with 1 when the files differ, and here they always do
            diff.push_str(&run(&self.root, &args, &[0, 1])?);
        }
        Ok(diff)
    }

    /// new files that aren't ignored, sorted by path
    fn untracked(&self) -> Result<Vec<String>, GitError> {
        let out = self.git(&["ls-files", "--others", "--exclude-standard", "-z"])?;
        let paths = out
            .split('\0')
            // a nested repo shows up as `dir/`, its files aren't ours to review
            .filter(|path| !path.is_empty() && !path.ends_with('/'))
            .map(str::to_owned);
        Ok(paths.collect())
    }

    pub(crate) fn git(&self, args: &[&str]) -> Result<String, GitError> {
        run(&self.root, args, &[0])
    }
}

/// runs git in `dir`, any exit code outside `ok` is a failure
fn run(dir: &Path, args: &[&str], ok: &[i32]) -> Result<String, GitError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        // paths with non ascii characters come out as is, not quoted and escaped
        .args(["-c", "core.quotepath=false"])
        .args(args)
        // read only commands shouldn't take the index lock
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .map_err(|err| match err.kind() {
            ErrorKind::NotFound => GitError::NotInstalled,
            _ => GitError::Io(err),
        })?;

    if !output.status.code().is_some_and(|code| ok.contains(&code)) {
        return Err(GitError::Failed {
            command: args.join(" "),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }

    // files that aren't utf-8 still get reviewed, the odd byte is replaced
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TestRepo;

    #[test]
    fn discover_finds_the_root_from_a_subdir() {
        let repo = TestRepo::new();
        repo.write("src/lib.rs", "fn main() {}\n");
        repo.commit("init");

        let found = Repo::discover(&repo.path().join("src")).unwrap();
        assert_eq!(
            found.root().canonicalize().unwrap(),
            repo.path().canonicalize().unwrap()
        );
    }

    #[test]
    fn discover_outside_a_repo_is_not_a_repo() {
        let dir = tempfile::tempdir().unwrap();
        let err = Repo::discover(dir.path()).unwrap_err();
        assert!(matches!(err, GitError::NotARepo(_)), "{err}");
    }

    #[test]
    fn branch_diff_has_only_the_branch_commits() {
        let repo = TestRepo::new();
        repo.write("a.txt", "one\n");
        repo.commit("init");
        repo.git(&["checkout", "-q", "-b", "feature"]);
        repo.write("b.txt", "from feature\n");
        repo.commit("feature work");
        // main moves on after the branch, that work isn't ours to review
        repo.git(&["checkout", "-q", "main"]);
        repo.write("a.txt", "changed on main\n");
        repo.commit("main work");
        repo.git(&["checkout", "-q", "feature"]);

        let git = repo.open();
        let diff = git.diff(&git.merge_base("main").unwrap()).unwrap();
        assert!(diff.contains("+++ b/b.txt"), "{diff}");
        assert!(!diff.contains("a.txt"), "{diff}");
    }

    #[test]
    fn diff_has_every_change_committed_or_not() {
        let repo = TestRepo::new();
        repo.write("committed.txt", "one\n");
        repo.write("staged.txt", "one\n");
        repo.write("unstaged.txt", "one\n");
        repo.write(".gitignore", "*.log\n");
        repo.commit("init");
        let base = repo.open().resolve("HEAD").unwrap().unwrap();

        repo.write("committed.txt", "committed\n");
        repo.commit("work");
        repo.write("staged.txt", "staged\n");
        repo.git(&["add", "staged.txt"]);
        repo.write("unstaged.txt", "unstaged\n");
        repo.write("src/untracked.txt", "untracked\n");
        repo.write("debug.log", "ignored\n");

        let diff = repo.open().diff(&base).unwrap();
        for line in ["+committed", "+staged", "+unstaged", "+untracked"] {
            assert!(diff.contains(line), "missing {line}:\n{diff}");
        }
        assert!(diff.contains("+++ b/src/untracked.txt"), "{diff}");
        assert!(!diff.contains("debug.log"), "{diff}");
    }

    #[test]
    fn deleted_files_are_in_the_diff() {
        let repo = TestRepo::new();
        repo.write("gone.txt", "bye\n");
        repo.commit("init");
        let base = repo.open().resolve("HEAD").unwrap().unwrap();
        std::fs::remove_file(repo.path().join("gone.txt")).unwrap();

        let diff = repo.open().diff(&base).unwrap();
        assert!(diff.contains("deleted file mode"), "{diff}");
    }

    #[test]
    fn no_changes_is_an_empty_diff() {
        let repo = TestRepo::new();
        repo.write("a.txt", "one\n");
        repo.commit("init");
        let git = repo.open();
        assert_eq!(
            git.diff(&git.resolve("HEAD").unwrap().unwrap()).unwrap(),
            ""
        );
    }

    #[test]
    fn diff_ignores_the_users_prefix_config() {
        let repo = TestRepo::new();
        repo.write("a.txt", "one\n");
        repo.commit("init");
        repo.git(&["config", "diff.noprefix", "true"]);
        repo.write("a.txt", "two\n");

        let git = repo.open();
        let diff = git.diff(&git.resolve("HEAD").unwrap().unwrap()).unwrap();
        assert!(diff.contains("--- a/a.txt\n+++ b/a.txt"), "{diff}");
    }

    #[test]
    fn resolve_never_treats_a_base_as_a_flag() {
        let repo = TestRepo::new();
        repo.write("a.txt", "one\n");
        repo.commit("init");
        assert_eq!(repo.open().resolve("--output=pwned").unwrap(), None);
        assert!(!repo.path().join("pwned").exists());
    }
}
