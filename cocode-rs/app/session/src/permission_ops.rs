//! Permission management methods for SessionState.
//!
//! Contains permission mode switching, approval store injection,
//! permission rule appending, and SDK permission requester wiring.

use std::sync::Arc;

use cocode_protocol::PermissionMode;

use super::SessionState;

impl SessionState {
    /// Set the permission mode for the session.
    ///
    /// Updates the loop config's permission mode. If switching to Plan mode,
    /// also saves the pre-plan mode for restoration on exit.
    pub fn set_permission_mode(&mut self, mode: PermissionMode) {
        let old_mode = self.loop_config.permission_mode;
        self.loop_config.permission_mode = mode;

        if mode == PermissionMode::Plan && old_mode != PermissionMode::Plan {
            // Entering plan mode: save the old mode for restoration
            self.plan_mode_state.pre_plan_mode = Some(old_mode);
            self.plan_mode_state.is_active = true;
        } else if mode != PermissionMode::Plan && old_mode == PermissionMode::Plan {
            // Leaving plan mode via mode cycle (not via ExitPlanMode tool)
            self.plan_mode_state.is_active = false;
            self.plan_mode_state.pre_plan_mode = None;
        }
    }

    /// Set the permission mode from a string (e.g., "default", "acceptEdits").
    pub fn set_permission_mode_from_str(&mut self, mode: &str) {
        self.set_permission_mode(mode.parse().unwrap_or(PermissionMode::Default));
    }

    /// Inject allowed prompts from a plan's `allowedPrompts` into the
    /// shared approval store so they persist across subsequent turns.
    pub async fn inject_allowed_prompts(&self, prompts: &[cocode_protocol::AllowedPrompt]) {
        let mut store = self.shared_approval_store.lock().await;
        for ap in prompts {
            store.approve_pattern(&ap.tool, &ap.prompt);
        }
    }

    /// Append SDK-provided permission rules from raw JSON.
    ///
    /// Each JSON value should have: `tool_pattern`, optional `file_pattern`,
    /// and `action` ("allow", "deny", or "ask").
    pub fn append_permission_rules_from_json(&mut self, rules: &[serde_json::Value]) {
        for rule_json in rules {
            let tool_pattern = rule_json
                .get("tool_pattern")
                .and_then(|v| v.as_str())
                .unwrap_or("*")
                .to_string();
            let file_pattern = rule_json
                .get("file_pattern")
                .and_then(|v| v.as_str())
                .map(String::from);
            let action = match rule_json
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("ask")
            {
                "allow" => cocode_policy::RuleAction::Allow,
                "deny" => cocode_policy::RuleAction::Deny,
                _ => cocode_policy::RuleAction::Ask,
            };
            self.permission_rules.push(cocode_policy::PermissionRule {
                source: cocode_protocol::RuleSource::Session,
                tool_pattern,
                file_pattern,
                action,
            });
        }
    }

    /// Set the permission requester for interactive approval flow (SDK mode).
    pub fn set_permission_requester(
        &mut self,
        requester: Arc<dyn cocode_tools::PermissionRequester>,
    ) {
        self.permission_requester = Some(requester);
    }
}
