//! tiny real git repos for tests

use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

use crate::Repo;

pub(crate) struct TestRepo {
    dir: TempDir,
}

impl TestRepo {
    pub(crate) fn new() -> Self {
        let repo = Self {
            dir: tempfile::tempdir().unwrap(),
        };
        repo.git(&["init", "-q", "-b", "main"]);
        repo
    }

    pub(crate) fn path(&self) -> &Path {
        self.dir.path()
    }

    pub(crate) fn open(&self) -> Repo {
        Repo::discover(self.path()).unwrap()
    }

    pub(crate) fn write(&self, path: &str, text: &str) {
        let path = self.path().join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    pub(crate) fn commit(&self, message: &str) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", message]);
    }

    pub(crate) fn git(&self, args: &[&str]) {
        // the -c flags keep tests working with any global git config
        let status = Command::new("git")
            .arg("-C")
            .arg(self.path())
            .args(["-c", "user.name=test", "-c", "user.email=test@example.com"])
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    }
}
