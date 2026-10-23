//! `gargoyle init`, sets a repo up with a config and the built-in checks

use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use std::process::Command;

use anyhow::Context;
use gargoyle_core::{CONFIG_FILE, ChecksConfig};

use crate::exit::Exit;

const CONFIG_TEMPLATE: &str = include_str!("../templates/gargoyle.toml");

// the skills cli knows where every agent keeps its skills, so gargoyle doesn't have to.
// pinned, so a new release never runs on your machine before we've tried it
const SKILLS_CLI: &str = "skills@1.7.1";
const SKILL_SOURCE: &str = "otarampinelli/gargoyle";

// baked into the binary so init works from any install, not only a clone of this repo
const BUILT_IN_CHECKS: [(&str, &str); 5] = [
    ("anti-slop.md", include_str!("../../../checks/anti-slop.md")),
    (
        "code-quality.md",
        include_str!("../../../checks/code-quality.md"),
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
    let config_path = root.join(CONFIG_FILE);
    if config_path.exists() && !force {
        // adding the skill to a repo that's already set up is fine
        if !skill {
            eprintln!("error: {CONFIG_FILE} already exists, use --force to overwrite it");
            return Ok(Exit::Config);
        }
        println!("kept {CONFIG_FILE}");
    } else {
        fs::write(&config_path, CONFIG_TEMPLATE)
            .with_context(|| format!("writing {}", config_path.display()))?;
        println!("created {CONFIG_FILE}");
    }

    // never overwrite a check, you may have edited it
    let checks_dir = ChecksConfig::default().dir;
    fs::create_dir_all(root.join(&checks_dir))
        .with_context(|| format!("creating {}", checks_dir.display()))?;
    for (name, text) in BUILT_IN_CHECKS {
        let path = checks_dir.join(name);
        if root.join(&path).exists() {
            println!("kept {}", path.display());
            continue;
        }
        fs::write(root.join(&path), text).with_context(|| format!("writing {}", path.display()))?;
        println!("created {}", path.display());
    }

    if skill {
        return add_skill(root);
    }
    println!("\nto add the /gargoyle skill to your agent, run:\n  npx skills add {SKILL_SOURCE}");
    Ok(Exit::Clean)
}

/// runs `npx skills add`, it asks which agents to add the skill to
fn add_skill(root: &Path) -> anyhow::Result<Exit> {
    // windows only finds npx through its .cmd shim
    let npx = if cfg!(windows) { "npx.cmd" } else { "npx" };
    println!("\nrunning npx {SKILLS_CLI} add {SKILL_SOURCE}");
    let status = Command::new(npx)
        .args(["--yes", SKILLS_CLI, "add", SKILL_SOURCE])
        .current_dir(root)
        .status();
    match status {
        Ok(status) if status.success() => Ok(Exit::Clean),
        Ok(_) => {
            eprintln!("error: npx skills add failed, see its output above");
            Ok(Exit::Runtime)
        }
        Err(err) if err.kind() == ErrorKind::NotFound => {
            eprintln!(
                "error: npx not found, install Node.js, or tell your agent to run `gargoyle guide`"
            );
            Ok(Exit::Runtime)
        }
        Err(err) => Err(err).context("running npx"),
    }
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
