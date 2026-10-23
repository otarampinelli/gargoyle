use crate::{GitError, Repo};

/// what a review compares against
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Base {
    /// the ref as written, like `origin/main`
    pub name: String,
    pub source: BaseSource,
    /// where the branch left the base, the review covers everything after it
    pub merge_base: String,
}

/// where the base came from, so you can tell why gargoyle picked it
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaseSource {
    Flag,
    Config,
    /// a CI provider env var, like `GITHUB_BASE_REF`
    CiEnv(&'static str),
    /// `origin/HEAD`, the remote's default branch
    RemoteHead,
    /// one of the common names, `origin/main` and friends
    Fallback,
}

/// the base branch CI providers set on pull requests
#[derive(Debug, Clone, Default)]
pub struct CiEnv {
    pub github_base_ref: Option<String>,
    pub gitlab_target_branch: Option<String>,
}

impl CiEnv {
    pub fn from_env() -> Self {
        let var = |name| {
            std::env::var(name)
                .ok()
                .filter(|value: &String| !value.is_empty())
        };
        Self {
            github_base_ref: var("GITHUB_BASE_REF"),
            gitlab_target_branch: var("CI_MERGE_REQUEST_TARGET_BRANCH_NAME"),
        }
    }
}

const FALLBACKS: [&str; 4] = ["origin/main", "origin/master", "main", "master"];

impl Repo {
    /// picks the base, first match wins: --base, review.base, CI env,
    /// origin/HEAD, then the common branch names
    pub fn find_base(
        &self,
        flag: Option<&str>,
        config: Option<&str>,
        ci: &CiEnv,
    ) -> Result<Base, GitError> {
        // a base you asked for must exist, falling back would review the wrong diff
        for (name, source) in [(flag, BaseSource::Flag), (config, BaseSource::Config)] {
            if let Some(name) = name {
                return self
                    .base(name, source)?
                    .ok_or_else(|| GitError::UnknownBase(name.to_owned()));
            }
        }

        let ci_vars = [
            ("GITHUB_BASE_REF", &ci.github_base_ref),
            (
                "CI_MERGE_REQUEST_TARGET_BRANCH_NAME",
                &ci.gitlab_target_branch,
            ),
        ];
        for (var, branch) in ci_vars {
            let Some(branch) = branch else { continue };
            // CI checkouts usually only have the remote branch
            for name in [format!("origin/{branch}"), branch.clone()] {
                if let Some(base) = self.base(&name, BaseSource::CiEnv(var))? {
                    return Ok(base);
                }
            }
        }

        if let Some(name) = self.remote_head()?
            && let Some(base) = self.base(&name, BaseSource::RemoteHead)?
        {
            return Ok(base);
        }

        for name in FALLBACKS {
            if let Some(base) = self.base(name, BaseSource::Fallback)? {
                return Ok(base);
            }
        }
        Err(GitError::NoBase)
    }

    fn base(&self, name: &str, source: BaseSource) -> Result<Option<Base>, GitError> {
        let Some(commit) = self.resolve(name)? else {
            return Ok(None);
        };
        Ok(Some(Base {
            name: name.to_owned(),
            source,
            merge_base: self.merge_base(&commit)?,
        }))
    }

    /// the remote's default branch, like `origin/main`
    fn remote_head(&self) -> Result<Option<String>, GitError> {
        match self.git(&[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ]) {
            Ok(name) => Ok(Some(name.trim_end().to_owned())),
            Err(GitError::Failed { .. }) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TestRepo;

    /// a repo on `feature`, one commit ahead of `main`
    fn feature_repo() -> TestRepo {
        let repo = TestRepo::new();
        repo.write("a.txt", "one\n");
        repo.commit("init");
        repo.git(&["checkout", "-q", "-b", "feature"]);
        repo.write("a.txt", "two\n");
        repo.commit("feature work");
        repo
    }

    fn find(repo: &TestRepo, flag: Option<&str>, config: Option<&str>, ci: &CiEnv) -> Base {
        repo.open().find_base(flag, config, ci).unwrap()
    }

    #[test]
    fn falls_back_to_main() {
        let repo = feature_repo();
        let base = find(&repo, None, None, &CiEnv::default());
        assert_eq!(
            (base.name.as_str(), base.source),
            ("main", BaseSource::Fallback)
        );
        assert_eq!(
            base.merge_base,
            repo.open().resolve("main").unwrap().unwrap()
        );
    }

    #[test]
    fn flag_beats_config() {
        let repo = feature_repo();
        repo.git(&["branch", "develop", "main"]);
        let base = find(&repo, Some("main"), Some("develop"), &CiEnv::default());
        assert_eq!(base.source, BaseSource::Flag);
        let base = find(&repo, None, Some("develop"), &CiEnv::default());
        assert_eq!(
            (base.name.as_str(), base.source),
            ("develop", BaseSource::Config)
        );
    }

    #[test]
    fn a_base_you_asked_for_must_exist() {
        let repo = feature_repo();
        let err = repo
            .open()
            .find_base(None, Some("nope"), &CiEnv::default())
            .unwrap_err();
        assert!(
            matches!(err, GitError::UnknownBase(ref name) if name == "nope"),
            "{err}"
        );
    }

    #[test]
    fn ci_env_prefers_the_remote_branch() {
        let repo = feature_repo();
        repo.git(&["update-ref", "refs/remotes/origin/main", "main"]);
        let ci = CiEnv {
            github_base_ref: Some("main".into()),
            ..CiEnv::default()
        };
        let base = find(&repo, None, None, &ci);
        assert_eq!(base.name, "origin/main");
        assert_eq!(base.source, BaseSource::CiEnv("GITHUB_BASE_REF"));
    }

    #[test]
    fn uses_the_remote_default_branch() {
        let repo = feature_repo();
        repo.git(&["update-ref", "refs/remotes/origin/develop", "main"]);
        repo.git(&[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/develop",
        ]);
        let base = find(&repo, None, None, &CiEnv::default());
        assert_eq!(
            (base.name.as_str(), base.source),
            ("origin/develop", BaseSource::RemoteHead)
        );
    }

    #[test]
    fn no_base_found() {
        let repo = TestRepo::new();
        repo.git(&["checkout", "-q", "-b", "trunk"]);
        repo.write("a.txt", "one\n");
        repo.commit("init");
        let err = repo
            .open()
            .find_base(None, None, &CiEnv::default())
            .unwrap_err();
        assert!(matches!(err, GitError::NoBase), "{err}");
    }
}
