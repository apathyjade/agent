//! Static registry of agents supported by the `skills` CLI.
//!
//! The table mirrors the CLI's supported-agents documentation
//! (<https://github.com/vercel-labs/skills/blob/main/docs/agents.md>) so the
//! install dialog and the `--agent` flag agree on ids, display names and
//! install locations. Keeping it static means no network access at runtime;
//! long-tail agents are handled by the caller's free-form id input.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

/// An agent the `skills` CLI can install into.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentOption {
    /// Value accepted by the CLI's `--agent` flag.
    pub id: String,
    /// Human-readable name shown in the UI.
    pub name: String,
    /// Project-scoped install path, relative to the workspace root.
    pub project_path: String,
    /// User-scoped install path, relative to the home directory.
    pub global_path: Option<String>,
}

/// Every supported agent, in the order the CLI documents them.
pub fn all() -> &'static [AgentOption] {
    static AGENTS: OnceLock<Vec<AgentOption>> = OnceLock::new();

    AGENTS.get_or_init(build_table).as_slice()
}

/// Build the registry. Every row is `(id, display name, project path, global path)`.
fn build_table() -> Vec<AgentOption> {
    const ROWS: &[(&str, &str, &str, Option<&str>)] = &[
        (
            "opencode",
            "OpenCode",
            ".agents/skills/",
            Some("~/.config/opencode/skills/"),
        ),
        (
            "claude-code",
            "Claude Code",
            ".claude/skills/",
            Some("~/.claude/skills/"),
        ),
        (
            "codex",
            "Codex",
            ".agents/skills/",
            Some("~/.agents/skills/"),
        ),
        (
            "cursor",
            "Cursor",
            ".agents/skills/",
            Some("~/.cursor/skills/"),
        ),
        (
            "gemini-cli",
            "Gemini CLI",
            ".agents/skills/",
            Some("~/.gemini/skills/"),
        ),
        (
            "github-copilot",
            "GitHub Copilot",
            ".agents/skills/",
            Some("~/.copilot/skills/"),
        ),
        (
            "windsurf",
            "Windsurf",
            ".windsurf/skills/",
            Some("~/.codeium/windsurf/skills/"),
        ),
        (
            "amp",
            "Amp",
            ".agents/skills/",
            Some("~/.config/agents/skills/"),
        ),
        (
            "cline",
            "Cline",
            ".agents/skills/",
            Some("~/.agents/skills/"),
        ),
        (
            "continue",
            "Continue",
            ".continue/skills/",
            Some("~/.continue/skills/"),
        ),
        (
            "goose",
            "Goose",
            ".goose/skills/",
            Some("~/.config/goose/skills/"),
        ),
        (
            "kilo",
            "Kilo Code",
            ".agents/skills/",
            Some("~/.kilo/skills/"),
        ),
        ("roo", "Roo Code", ".roo/skills/", Some("~/.roo/skills/")),
        ("zed", "Zed", ".agents/skills/", Some("~/.agents/skills/")),
        (
            "qwen-code",
            "Qwen Code",
            ".qwen/skills/",
            Some("~/.qwen/skills/"),
        ),
        (
            "kimi-code-cli",
            "Kimi Code CLI",
            ".agents/skills/",
            Some("~/.agents/skills/"),
        ),
        ("trae", "Trae", ".trae/skills/", Some("~/.trae/skills/")),
        (
            "augment",
            "Augment",
            ".augment/skills/",
            Some("~/.augment/skills/"),
        ),
        (
            "openhands",
            "OpenHands",
            ".openhands/skills/",
            Some("~/.openhands/skills/"),
        ),
        (
            "universal",
            "Universal",
            ".agents/skills/",
            Some("~/.config/agents/skills/"),
        ),
    ];

    ROWS.iter()
        .map(|(id, name, project_path, global_path)| AgentOption {
            id: (*id).to_string(),
            name: (*name).to_string(),
            project_path: (*project_path).to_string(),
            global_path: global_path.map(str::to_string),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find(id: &str) -> &'static AgentOption {
        all()
            .iter()
            .find(|agent| agent.id == id)
            .unwrap_or_else(|| panic!("missing agent '{id}'"))
    }

    #[test]
    fn contains_at_least_twenty_agents() {
        assert!(
            all().len() >= 20,
            "expected at least 20 agents, got {}",
            all().len()
        );
    }

    #[test]
    fn ids_and_display_names_are_unique() {
        let mut ids: Vec<&str> = all().iter().map(|agent| agent.id.as_str()).collect();
        let total = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), total, "agent ids must be unique");

        let mut names: Vec<&str> = all().iter().map(|agent| agent.name.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), total, "agent display names must be unique");
    }

    #[test]
    fn paths_are_relative_and_home_scoped() {
        for agent in all() {
            assert!(!agent.id.trim().is_empty(), "agent id must not be empty");
            assert!(
                !agent.name.trim().is_empty(),
                "{} name must not be empty",
                agent.id
            );
            assert!(
                agent.project_path.ends_with('/'),
                "{} project path must end with '/': {}",
                agent.id,
                agent.project_path
            );
            assert!(
                !agent.project_path.starts_with('/')
                    && !agent.project_path.starts_with('~')
                    && !agent.project_path.contains(':'),
                "{} project path must be relative: {}",
                agent.id,
                agent.project_path
            );

            if let Some(global) = &agent.global_path {
                assert!(
                    global.starts_with("~/"),
                    "{} global path must start with '~/': {}",
                    agent.id,
                    global
                );
                assert!(
                    global.ends_with('/'),
                    "{} global path must end with '/': {}",
                    agent.id,
                    global
                );
            }
        }
    }

    #[test]
    fn matches_cli_agent_table() {
        let opencode = find("opencode");
        assert_eq!(opencode.name, "OpenCode");
        assert_eq!(opencode.project_path, ".agents/skills/");
        assert_eq!(
            opencode.global_path.as_deref(),
            Some("~/.config/opencode/skills/")
        );

        let claude = find("claude-code");
        assert_eq!(claude.project_path, ".claude/skills/");
        assert_eq!(claude.global_path.as_deref(), Some("~/.claude/skills/"));

        let windsurf = find("windsurf");
        assert_eq!(windsurf.project_path, ".windsurf/skills/");
        assert_eq!(
            windsurf.global_path.as_deref(),
            Some("~/.codeium/windsurf/skills/")
        );

        let universal = find("universal");
        assert_eq!(universal.project_path, ".agents/skills/");
        assert_eq!(
            universal.global_path.as_deref(),
            Some("~/.config/agents/skills/")
        );
    }
}
