//! `gargoyle init`, sets a repo up with a config, the built-in checks and the /gargoyle skill

use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use std::process::Command;

use anyhow::Context;
use gargoyle_core::{CONFIG_FILE, ChecksConfig};

use crate::exit::Exit;
use crate::output::out;

const CONFIG_TEMPLATE: &str = include_str!("../templates/gargoyle.toml");

// the skills cli knows where every agent keeps its skills, so gargoyle doesn't have to.
// pinned, so a new release never runs on your machine before we've tried it
const SKILLS_CLI: &str = "skills@1.7.1";
const SKILL_SOURCE: &str = "otarampinelli/gargoyle";

// baked into the binary so init works from any install, not only a clone of this repo
const BUILT_IN_CHECKS: [(&str, &str); 6] = [
    ("anti-slop.md", include_str!("../../../checks/anti-slop.md")),
    (
        "code-quality.md",
        include_str!("../../../checks/code-quality.md"),
    ),
    (
        "correctness.md",
        include_str!("../../../checks/correctness.md"),
    ),
    (
        "security-review.md",
        include_str!("../../../checks/security-review.md"),
    ),
    (
        "simplicity.md",
        include_str!("../../../checks/simplicity.md"),
    ),
    (
        "test-coverage.md",
        include_str!("../../../checks/test-coverage.md"),
    ),
];

pub(crate) fn run(root: &Path, force: bool, skill: bool) -> anyhow::Result<Exit> {
    // init can run again any time, it only fills in what's missing
    let config_path = root.join(CONFIG_FILE);
    if config_path.exists() && !force {
        out!("kept {CONFIG_FILE}, use --force to overwrite it")?;
    } else {
        fs::write(&config_path, CONFIG_TEMPLATE)
            .with_context(|| format!("writing {}", config_path.display()))?;
        out!("created {CONFIG_FILE}")?;
    }

    // never overwrite a check, you may have edited it
    let checks_dir = ChecksConfig::default().dir;
    fs::create_dir_all(root.join(&checks_dir))
        .with_context(|| format!("creating {}", checks_dir.display()))?;
    for (name, text) in BUILT_IN_CHECKS {
        let path = checks_dir.join(name);
        if root.join(&path).exists() {
            out!("kept {}", path.display())?;
            continue;
        }
        fs::write(root.join(&path), text).with_context(|| format!("writing {}", path.display()))?;
        out!("created {}", path.display())?;
    }

    if skill {
        add_skill(root)?;
    }
    Ok(Exit::Clean)
}

/// runs `npx skills add` for this repo, it asks which agents get the skill.
/// the skill is a nice to have, so a problem here never fails init
fn add_skill(root: &Path) -> anyhow::Result<()> {
    // windows only finds npx through its .cmd shim
    let npx = if cfg!(windows) { "npx.cmd" } else { "npx" };
    out!("\nadding the /gargoyle skill with npx {SKILLS_CLI} add {SKILL_SOURCE}")?;
    let status = Command::new(npx)
        .args(["--yes", SKILLS_CLI, "add", SKILL_SOURCE])
        .current_dir(root)
        .status();
    let added = match status {
        Ok(status) if status.success() => true,
        Ok(_) => {
            eprintln!("warning: npx skills add failed, see its output above");
            false
        }
        Err(err) if err.kind() == ErrorKind::NotFound => {
            eprintln!("warning: no Node.js found, so no /gargoyle skill");
            false
        }
        Err(err) => return Err(err).context("running npx"),
    };
    if !added {
        eprintln!(
            "any agent can still run a review, add this to its instructions, like AGENTS.md:\n  \
             To review changes, run `gargoyle guide` and follow it."
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gargoyle_core::Config;

    // the template and the rust defaults must never drift apart
    #[test]
    fn template_matches_the_defaults() {
        let config = Config::parse(Path::new(CONFIG_FILE), CONFIG_TEMPLATE).unwrap();
        assert_eq!(config, Config::default());
    }

    #[test]
    fn built_in_checks_are_valid() {
        for (name, text) in BUILT_IN_CHECKS {
            let path = Path::new(".gargoyle/checks").join(name);
            assert!(gargoyle_core::Check::parse(&path, text).is_ok(), "{name}");
        }
    }
}
