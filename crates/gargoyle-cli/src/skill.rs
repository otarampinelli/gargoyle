//! the /gargoyle skill: which folder each agent reads skills from, and writing it there
//!
//! the folders come from the skills cli's agent table (skills@1.7.4), project scope only

use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use anyhow::Context;

// baked into the binary so init works from any install, not only a clone of this repo
const SKILL: &str = include_str!("../../../skills/SKILL.md");

/// the folder most agents share, from the open skills standard
pub(crate) const SHARED_DIR: &str = ".agents/skills";

/// where `init` puts the skill when nobody picks: the shared folder and Claude Code's
pub(crate) const DEFAULT_DIRS: [&str; 2] = [SHARED_DIR, ".claude/skills"];

#[derive(Debug)]
pub(crate) struct Agent {
    /// what `--agent` takes, the same ids as the skills cli
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    pub(crate) dir: &'static str,
}

const fn agent(id: &'static str, name: &'static str, dir: &'static str) -> Agent {
    Agent { id, name, dir }
}

// the shared folder first, in rough order of how many people use each agent
pub(crate) const AGENTS: &[Agent] = &[
    agent("codex", "Codex", SHARED_DIR),
    agent("cursor", "Cursor", SHARED_DIR),
    agent("gemini-cli", "Gemini CLI", SHARED_DIR),
    agent("github-copilot", "GitHub Copilot", SHARED_DIR),
    agent("opencode", "OpenCode", SHARED_DIR),
    agent("amp", "Amp", SHARED_DIR),
    agent("cline", "Cline", SHARED_DIR),
    agent("kilo", "Kilo Code", SHARED_DIR),
    agent("droid", "Droid", SHARED_DIR),
    agent("warp", "Warp", SHARED_DIR),
    agent("zed", "Zed", SHARED_DIR),
    agent("antigravity", "Antigravity", SHARED_DIR),
    agent("replit", "Replit", SHARED_DIR),
    agent("kimi-code-cli", "Kimi Code CLI", SHARED_DIR),
    agent("pi", "Pi", SHARED_DIR),
    agent("deepagents", "Deep Agents", SHARED_DIR),
    agent(
        "universal",
        "any agent that reads .agents/skills",
        SHARED_DIR,
    ),
    agent("claude-code", "Claude Code", ".claude/skills"),
    agent("windsurf", "Windsurf", ".windsurf/skills"),
    agent("roo", "Roo Code", ".roo/skills"),
    agent("continue", "Continue", ".continue/skills"),
    agent("goose", "Goose", ".goose/skills"),
    agent("kiro-cli", "Kiro CLI", ".kiro/skills"),
    agent("junie", "Junie", ".junie/skills"),
    agent("augment", "Augment", ".augment/skills"),
    agent("qwen-code", "Qwen Code", ".qwen/skills"),
    agent("trae", "Trae", ".trae/skills"),
    agent("openhands", "OpenHands", ".openhands/skills"),
    agent("crush", "Crush", ".crush/skills"),
    agent("devin", "Devin for Terminal", ".devin/skills"),
    agent("mistral-vibe", "Mistral Vibe", ".vibe/skills"),
];

/// a skills folder and the agents that read it
#[derive(Debug)]
pub(crate) struct Target {
    pub(crate) dir: &'static str,
    pub(crate) agents: Vec<&'static str>,
}

impl Target {
    /// `Codex, Cursor, Gemini CLI and 14 more`, short enough for one line
    pub(crate) fn label(&self) -> String {
        const SHOWN: usize = 4;
        let named: Vec<_> = self
            .agents
            .iter()
            .copied()
            .filter(|name| !name.starts_with("any agent"))
            .collect();
        match named.len() {
            n if n <= SHOWN + 1 => named.join(", "),
            n => format!("{} and {} more", named[..SHOWN].join(", "), n - SHOWN),
        }
    }
}

/// every folder once, with its agents, in the order of `AGENTS`
pub(crate) fn targets() -> Vec<Target> {
    let mut targets: Vec<Target> = Vec::new();
    for agent in AGENTS {
        match targets.iter_mut().find(|t| t.dir == agent.dir) {
            Some(target) => target.agents.push(agent.name),
            None => targets.push(Target {
                dir: agent.dir,
                agents: vec![agent.name],
            }),
        }
    }
    targets
}

pub(crate) fn target(dir: &str) -> Option<Target> {
    targets().into_iter().find(|t| t.dir == dir)
}

/// the folders for the agents named in `--agent`, each once, or the id nobody knows
pub(crate) fn dirs_for(ids: &[String]) -> Result<Vec<&'static str>, &str> {
    let mut dirs = Vec::new();
    for id in ids {
        let agent = AGENTS
            .iter()
            .find(|agent| agent.id == id.trim())
            .ok_or(id.as_str())?;
        if !dirs.contains(&agent.dir) {
            dirs.push(agent.dir);
        }
    }
    Ok(dirs)
}

pub(crate) fn known_ids() -> String {
    AGENTS
        .iter()
        .map(|agent| agent.id)
        .collect::<Vec<_>>()
        .join(", ")
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Installed {
    Created,
    /// already there with the same text
    UpToDate,
    /// already there with other text, maybe edited, left as it was
    Kept,
}

/// writes the skill to `<dir>/gargoyle/SKILL.md`, an edited one only with `force`
pub(crate) fn install(root: &Path, dir: &str, force: bool) -> anyhow::Result<Installed> {
    let path = root.join(dir).join("gargoyle").join("SKILL.md");
    match fs::read_to_string(&path) {
        Ok(text) if text == SKILL => return Ok(Installed::UpToDate),
        Ok(_) if !force => return Ok(Installed::Kept),
        Ok(_) => {}
        Err(err) if err.kind() == ErrorKind::NotFound => {}
        Err(err) => return Err(err).with_context(|| format!("reading {}", path.display())),
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    fs::write(&path, SKILL).with_context(|| format!("writing {}", path.display()))?;
    Ok(Installed::Created)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_id_is_unique() {
        for (i, agent) in AGENTS.iter().enumerate() {
            assert!(
                AGENTS[i + 1..].iter().all(|other| other.id != agent.id),
                "{}",
                agent.id
            );
        }
    }

    #[test]
    fn agents_that_share_a_folder_are_one_target() {
        let shared = target(SHARED_DIR).unwrap();
        assert_eq!(
            shared.label(),
            "Codex, Cursor, Gemini CLI, GitHub Copilot and 12 more"
        );
        assert_eq!(target(".claude/skills").unwrap().label(), "Claude Code");
        assert_eq!(targets()[0].dir, SHARED_DIR);
    }

    #[test]
    fn named_agents_become_their_folders_once() {
        let ids = [
            "codex".to_owned(),
            " cursor".to_owned(),
            "windsurf".to_owned(),
        ];
        assert_eq!(dirs_for(&ids).unwrap(), [SHARED_DIR, ".windsurf/skills"]);
        assert_eq!(dirs_for(&["vim".to_owned()]).unwrap_err(), "vim");
    }

    #[test]
    fn an_edited_skill_is_kept_unless_forced() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            install(dir.path(), SHARED_DIR, false).unwrap(),
            Installed::Created
        );
        assert_eq!(
            install(dir.path(), SHARED_DIR, false).unwrap(),
            Installed::UpToDate
        );

        let path = dir.path().join(SHARED_DIR).join("gargoyle/SKILL.md");
        fs::write(&path, "my edits").unwrap();
        assert_eq!(
            install(dir.path(), SHARED_DIR, false).unwrap(),
            Installed::Kept
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "my edits");
        assert_eq!(
            install(dir.path(), SHARED_DIR, true).unwrap(),
            Installed::Created
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), SKILL);
    }
}
