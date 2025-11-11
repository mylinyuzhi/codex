use serde::Deserialize;
use serde::Serialize;
use std::sync::Arc;

/// Definition of a configurable agent loaded from TOML files
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentDefinition {
    /// Unique agent identifier
    pub name: String,

    /// Description for documentation and tool tips
    pub description: String,

    /// System prompt prepended to task prompts
    pub system_prompt: String,

    /// Optional: Override model to use
    #[serde(default)]
    pub model: Option<String>,

    /// Optional: Tool whitelist (None = all tools allowed)
    #[serde(default)]
    pub tools: Option<Vec<String>>,

    /// Optional: Maximum number of turns
    #[serde(default)]
    pub max_turns: Option<i32>,

    /// Optional: Thinking budget in tokens
    #[serde(default)]
    pub thinking_budget: Option<i32>,
}

impl AgentDefinition {
    /// Check if a tool is allowed for this agent
    pub fn is_tool_allowed(&self, tool_name: &str) -> bool {
        match &self.tools {
            None => true, // No restrictions = all tools allowed
            Some(allowed) => allowed.contains(&tool_name.to_string()),
        }
    }

    /// Validate required fields
    pub fn validate(&self) -> Result<(), String> {
        if self.name.is_empty() {
            return Err("Agent name cannot be empty".to_string());
        }
        if self.description.is_empty() {
            return Err("Agent description cannot be empty".to_string());
        }
        if self.system_prompt.is_empty() {
            return Err("Agent system_prompt cannot be empty".to_string());
        }
        Ok(())
    }
}

/// Agent loading status
#[derive(Debug, Clone)]
pub enum AgentLoadStatus {
    /// Successfully loaded and validated
    Available(Arc<AgentDefinition>),

    /// Configuration file has errors
    Invalid { name: String, error: String },
}

impl AgentLoadStatus {
    pub fn name(&self) -> &str {
        match self {
            AgentLoadStatus::Available(def) => &def.name,
            AgentLoadStatus::Invalid { name, .. } => name,
        }
    }

    pub fn is_available(&self) -> bool {
        matches!(self, AgentLoadStatus::Available(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_definition_validation() {
        let valid = AgentDefinition {
            name: "test".to_string(),
            description: "Test agent".to_string(),
            system_prompt: "You are a test agent".to_string(),
            model: None,
            tools: None,
            max_turns: None,
            thinking_budget: None,
        };
        assert!(valid.validate().is_ok());

        let invalid = AgentDefinition {
            name: "".to_string(),
            description: "Test".to_string(),
            system_prompt: "Test".to_string(),
            model: None,
            tools: None,
            max_turns: None,
            thinking_budget: None,
        };
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn test_tool_filtering() {
        let agent = AgentDefinition {
            name: "restricted".to_string(),
            description: "Test".to_string(),
            system_prompt: "Test".to_string(),
            model: None,
            tools: Some(vec!["read_file".to_string(), "grep".to_string()]),
            max_turns: None,
            thinking_budget: None,
        };

        assert!(agent.is_tool_allowed("read_file"));
        assert!(agent.is_tool_allowed("grep"));
        assert!(!agent.is_tool_allowed("shell"));
        assert!(!agent.is_tool_allowed("write_file"));
    }

    #[test]
    fn test_no_tool_restrictions() {
        let agent = AgentDefinition {
            name: "unrestricted".to_string(),
            description: "Test".to_string(),
            system_prompt: "Test".to_string(),
            model: None,
            tools: None,
            max_turns: None,
            thinking_budget: None,
        };

        assert!(agent.is_tool_allowed("read_file"));
        assert!(agent.is_tool_allowed("shell"));
        assert!(agent.is_tool_allowed("anything"));
    }
}
