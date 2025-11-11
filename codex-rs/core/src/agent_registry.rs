use codex_protocol::agent_definition::AgentDefinition;
use codex_protocol::agent_definition::AgentLoadStatus;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;
use tracing::warn;

/// Default system prompt for the Review agent
const DEFAULT_REVIEW_PROMPT: &str = r#"You are a code review expert. Your task is to:
- Review code changes for potential issues
- Check for bugs, performance problems, and security vulnerabilities
- Suggest improvements and best practices
- Provide constructive feedback

Focus on actionable recommendations."#;

/// Default system prompt for the Compact agent
const DEFAULT_COMPACT_PROMPT: &str = r#"You are a context compression specialist. Your task is to:
- Summarize long conversation histories
- Extract key information and decisions
- Maintain important context while reducing token count
- Preserve technical details and code snippets

Be concise but comprehensive."#;

/// Registry for managing agent definitions
pub struct AgentRegistry {
    agents: HashMap<String, AgentLoadStatus>,
}

impl AgentRegistry {
    /// Load all agents from configuration directories
    ///
    /// Priority order (highest to lowest):
    /// 1. Project-level: .codex/agents/*.toml (overrides all)
    /// 2. User-level: ~/.codex/agents/*.toml
    /// 3. Built-in: Review, Compact (code-defined)
    pub fn load() -> Self {
        let mut agents = HashMap::new();

        // 1. Built-in agents (lowest priority)
        Self::register_builtin_agents(&mut agents);

        // 2. User-level configurations
        if let Some(home_dir) = dirs::home_dir() {
            let user_dir = home_dir.join(".codex/agents");
            Self::load_from_directory(&mut agents, &user_dir, "user");
        }

        // 3. Project-level configurations (highest priority, can override)
        let project_dir = PathBuf::from(".codex/agents");
        Self::load_from_directory(&mut agents, &project_dir, "project");

        info!(
            "Loaded {} agent definitions ({} available, {} invalid)",
            agents.len(),
            agents.values().filter(|s| s.is_available()).count(),
            agents.values().filter(|s| !s.is_available()).count()
        );

        Self { agents }
    }

    /// Register built-in agent definitions
    fn register_builtin_agents(agents: &mut HashMap<String, AgentLoadStatus>) {
        agents.insert(
            "review".to_string(),
            AgentLoadStatus::Available(Arc::new(AgentDefinition {
                name: "review".to_string(),
                description: "Code review agent for analyzing changes and suggesting improvements"
                    .to_string(),
                system_prompt: DEFAULT_REVIEW_PROMPT.to_string(),
                model: None,
                tools: None,
                max_turns: None,
                thinking_budget: None,
            })),
        );

        agents.insert(
            "compact".to_string(),
            AgentLoadStatus::Available(Arc::new(AgentDefinition {
                name: "compact".to_string(),
                description: "Context compression agent for summarizing conversation history"
                    .to_string(),
                system_prompt: DEFAULT_COMPACT_PROMPT.to_string(),
                model: None,
                tools: None,
                max_turns: None,
                thinking_budget: None,
            })),
        );
    }

    /// Load agent definitions from a directory
    /// Public for testing purposes
    #[doc(hidden)]
    pub fn load_from_directory(
        agents: &mut HashMap<String, AgentLoadStatus>,
        dir: &Path,
        source: &str,
    ) {
        if !dir.exists() {
            return;
        }

        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(err) => {
                warn!("Failed to read agent directory {:?}: {}", dir, err);
                return;
            }
        };

        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();

            // Only process .toml files
            if path.extension().and_then(|s| s.to_str()) != Some("toml") {
                continue;
            }

            match Self::load_agent_file(&path) {
                Ok(def) => {
                    info!(
                        "Loaded agent '{}' from {} ({})",
                        def.name,
                        source,
                        path.display()
                    );
                    agents.insert(def.name.clone(), AgentLoadStatus::Available(Arc::new(def)));
                }
                Err(err) => {
                    let name = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("unknown")
                        .to_string();
                    warn!(
                        "Failed to load agent from {:?}: {} (marked as Invalid)",
                        path, err
                    );
                    agents.insert(
                        name.clone(),
                        AgentLoadStatus::Invalid {
                            name,
                            error: err.to_string(),
                        },
                    );
                }
            }
        }
    }

    /// Parse and validate a single agent configuration file
    fn load_agent_file(path: &Path) -> Result<AgentDefinition, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let def: AgentDefinition = toml::from_str(&content)?;

        // Validate required fields
        def.validate()
            .map_err(|e| format!("Validation failed: {}", e))?;

        Ok(def)
    }

    /// Get agent definition by name
    pub fn get(&self, name: &str) -> Option<&AgentLoadStatus> {
        self.agents.get(name)
    }

    /// List all available (valid) agents
    pub fn list_available(&self) -> Vec<Arc<AgentDefinition>> {
        self.agents
            .values()
            .filter_map(|status| match status {
                AgentLoadStatus::Available(def) => Some(Arc::clone(def)),
                AgentLoadStatus::Invalid { .. } => None,
            })
            .collect()
    }

    /// List all agents including invalid ones
    pub fn list_all(&self) -> &HashMap<String, AgentLoadStatus> {
        &self.agents
    }

    /// Check if an agent exists and is available
    pub fn is_available(&self, name: &str) -> bool {
        self.agents
            .get(name)
            .map(|s| s.is_available())
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_builtin_agents_loaded() {
        let registry = AgentRegistry::load();

        assert!(registry.is_available("review"));
        assert!(registry.is_available("compact"));

        let review = registry.get("review").unwrap();
        match review {
            AgentLoadStatus::Available(def) => {
                assert_eq!(def.name, "review");
                assert!(!def.system_prompt.is_empty());
            }
            _ => panic!("Expected review agent to be available"),
        }
    }

    #[test]
    fn test_load_custom_agent_from_file() {
        let temp_dir = TempDir::new().unwrap();
        let agent_file = temp_dir.path().join("test-agent.toml");

        std::fs::write(
            &agent_file,
            r#"
name = "test-agent"
description = "Test agent for unit testing"
system_prompt = "You are a test agent"
model = "claude-sonnet-4"
tools = ["read_file", "grep"]
max_turns = 10
thinking_budget = 5000
"#,
        )
        .unwrap();

        let mut agents = HashMap::new();
        AgentRegistry::load_from_directory(&mut agents, temp_dir.path(), "test");

        assert_eq!(agents.len(), 1);
        let status = agents.get("test-agent").unwrap();

        match status {
            AgentLoadStatus::Available(def) => {
                assert_eq!(def.name, "test-agent");
                assert_eq!(def.model.as_ref().unwrap(), "claude-sonnet-4");
                assert_eq!(def.tools.as_ref().unwrap().len(), 2);
                assert_eq!(def.max_turns, Some(10));
                assert_eq!(def.thinking_budget, Some(5000));
            }
            AgentLoadStatus::Invalid { error, .. } => {
                panic!("Expected valid agent, got Invalid: {}", error)
            }
        }
    }

    #[test]
    fn test_invalid_agent_marked_as_invalid() {
        let temp_dir = TempDir::new().unwrap();
        let agent_file = temp_dir.path().join("bad-agent.toml");

        // Missing required field: system_prompt
        std::fs::write(
            &agent_file,
            r#"
name = "bad-agent"
description = "Bad agent with missing system_prompt"
"#,
        )
        .unwrap();

        let mut agents = HashMap::new();
        AgentRegistry::load_from_directory(&mut agents, temp_dir.path(), "test");

        assert_eq!(agents.len(), 1);
        match agents.get("bad-agent").unwrap() {
            AgentLoadStatus::Invalid { error, .. } => {
                assert!(error.contains("system_prompt"));
            }
            _ => panic!("Expected Invalid status"),
        }
    }

    #[test]
    fn test_priority_override() {
        // Simulate priority: built-in < user < project
        let mut agents = HashMap::new();

        // Built-in
        AgentRegistry::register_builtin_agents(&mut agents);
        let builtin_review = match agents.get("review").unwrap() {
            AgentLoadStatus::Available(def) => Arc::clone(def),
            _ => panic!("Expected available"),
        };

        // Override with custom
        let temp_dir = TempDir::new().unwrap();
        let agent_file = temp_dir.path().join("review.toml");
        std::fs::write(
            &agent_file,
            r#"
name = "review"
description = "Custom review agent"
system_prompt = "Custom prompt for review"
model = "claude-opus-4"
"#,
        )
        .unwrap();

        AgentRegistry::load_from_directory(&mut agents, temp_dir.path(), "custom");

        let custom_review = match agents.get("review").unwrap() {
            AgentLoadStatus::Available(def) => Arc::clone(def),
            _ => panic!("Expected available"),
        };

        // Verify override
        assert_ne!(builtin_review.system_prompt, custom_review.system_prompt);
        assert_eq!(custom_review.model.as_ref().unwrap(), "claude-opus-4");
    }

    #[test]
    fn test_list_available_excludes_invalid() {
        let mut agents = HashMap::new();

        agents.insert(
            "valid1".to_string(),
            AgentLoadStatus::Available(Arc::new(AgentDefinition {
                name: "valid1".to_string(),
                description: "Valid agent 1".to_string(),
                system_prompt: "Prompt 1".to_string(),
                model: None,
                tools: None,
                max_turns: None,
                thinking_budget: None,
            })),
        );

        agents.insert(
            "invalid1".to_string(),
            AgentLoadStatus::Invalid {
                name: "invalid1".to_string(),
                error: "Test error".to_string(),
            },
        );

        agents.insert(
            "valid2".to_string(),
            AgentLoadStatus::Available(Arc::new(AgentDefinition {
                name: "valid2".to_string(),
                description: "Valid agent 2".to_string(),
                system_prompt: "Prompt 2".to_string(),
                model: None,
                tools: None,
                max_turns: None,
                thinking_budget: None,
            })),
        );

        let registry = AgentRegistry { agents };
        let available = registry.list_available();

        assert_eq!(available.len(), 2);
        assert!(available.iter().any(|def| def.name == "valid1"));
        assert!(available.iter().any(|def| def.name == "valid2"));
        assert!(!available.iter().any(|def| def.name == "invalid1"));
    }
}
