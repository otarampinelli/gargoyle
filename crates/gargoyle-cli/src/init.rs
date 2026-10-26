//! `gargoyle init`, sets a repo up with a config, the built-in checks and the /gargoyle skill

use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use anyhow::Context;
use gargoyle_core::{CONFIG_FILE, ChecksConfig};

use crate::exit::{Exit, config_error};
use crate::skill::{self, DEFAULT_DIRS, Installed};
use crate::ui::Ui;

const CONFIG_TEMPLATE: &str = include_str!("../templates/gargoyle.toml");

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

pub(crate) fn run(
    root: &Path,
    force: bool,
    add_skill: bool,
    agents: &[String],
) -> anyhow::Result<Exit> {
    // an unknown agent fails before anything is written
    let named = if agents.is_empty() {
        None
    } else {
        Some(skill::dirs_for(agents).map_err(|id| {
            config_error(format!(
                "unknown agent `{id}`, the agents are: {}",
                skill::known_ids()
            ))
        })?)
    };
    let ui = Ui::detect();
    ui.intro("gargoyle init")?;

    // init can run again any time, it only fills in what's missing
    let config_path = root.join(CONFIG_FILE);
    if config_path.exists() && !force {
        ui.note(format!("Kept {CONFIG_FILE}, --force overwrites it"))?;
    } else {
        fs::write(&config_path, CONFIG_TEMPLATE)
            .with_context(|| format!("writing {}", config_path.display()))?;
        ui.done(format!("Created {CONFIG_FILE}"))?;
    }

    // never overwrite a check, you may have edited it
    let checks_dir = ChecksConfig::default().dir;
    fs::create_dir_all(root.join(&checks_dir))
        .with_context(|| format!("creating {}", checks_dir.display()))?;
    let (mut created, mut kept) = (0, 0);
    for (name, text) in BUILT_IN_CHECKS {
        let path = root.join(&checks_dir).join(name);
        if path.exists() {
            kept += 1;
            continue;
        }
        fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
        created += 1;
    }
    let dir = checks_dir.display();
    if created > 0 {
        ui.done(format!("Added {created} built-in checks to {dir}"))?;
    }
    if kept > 0 {
        ui.note(format!(
            "Kept {kept} checks already in {dir}, init never overwrites them"
        ))?;
    }

    if add_skill {
        let dirs = match named {
            Some(dirs) => dirs,
            None if ui == Ui::Fancy => pick_agents()?,
            None => DEFAULT_DIRS.to_vec(),
        };
        install_skill(ui, root, &dirs, force)?;
    }
    ui.outro("Ready. Run /gargoyle in your agent to review your changes.")?;
    Ok(Exit::Clean)
}

/// asks which agents get the skill, the shared folder and Claude Code are picked already
fn pick_agents() -> anyhow::Result<Vec<&'static str>> {
    let mut prompt = cliclack::multiselect("Which agents should get the /gargoyle skill?")
        .required(false)
        .max_rows(12);
    for target in skill::targets() {
        prompt = prompt.item(target.dir, target.label(), target.dir);
    }
    match prompt.initial_values(DEFAULT_DIRS.to_vec()).interact() {
        Ok(dirs) => Ok(dirs),
        // esc or ctrl-c on the picker skips the skill, the rest is already set up
        Err(err) if err.kind() == ErrorKind::Interrupted => Ok(Vec::new()),
        Err(err) => Err(err.into()),
    }
}

fn install_skill(ui: Ui, root: &Path, dirs: &[&'static str], force: bool) -> anyhow::Result<()> {
    if dirs.is_empty() {
        ui.note(
            "No skill added. Any agent can still run a review, add this to its instructions, \
             like AGENTS.md: To review changes, run `gargoyle guide` and follow it.",
        )?;
        return Ok(());
    }
    for &dir in dirs {
        let who = skill::target(dir).map(|t| t.label()).unwrap_or_default();
        let (message, created) = match skill::install(root, dir, force)? {
            Installed::Created => (
                format!("Added the /gargoyle skill to {dir} for {who}"),
                true,
            ),
            Installed::UpToDate => (format!("The /gargoyle skill in {dir} is up to date"), false),
            Installed::Kept => (
                format!("Kept {dir}/gargoyle/SKILL.md, it has other text, --force overwrites it"),
                false,
            ),
        };
        if created {
            ui.done(message)?;
        } else {
            ui.note(message)?;
        }
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
