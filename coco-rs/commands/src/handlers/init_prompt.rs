//! `/init` command — return the codebase-init prompt body.
//!
//! TS source: `commands/init.ts:226-254`. Two prompt bodies, gated by
//! `feature('NEW_INIT') && (USER_TYPE === 'ant' || CLAUDE_CODE_NEW_INIT)`.
//!
//! - Old: single instruction to generate CLAUDE.md.
//! - New: 8-phase guided setup (Q&A, codebase survey, CLAUDE.md, skills, hooks).

use async_trait::async_trait;
use coco_types::Feature;
use coco_types::Features;
use coco_types::UserType;

use crate::CommandHandler;
use crate::CommandResult;
use crate::PromptPart;

const NEW_INIT_PROMPT: &str = include_str!("../prompts/init_new.txt");
const OLD_INIT_PROMPT: &str = include_str!("../prompts/init_old.txt");

pub struct InitPromptHandler {
    pub user_type: UserType,
    pub features: Features,
}

impl InitPromptHandler {
    pub fn select_prompt(&self) -> &'static str {
        let new_init_env = std::env::var("COCO_NEW_INIT")
            .map(|v| matches!(v.as_str(), "1" | "true" | "yes"))
            .unwrap_or(false);
        if self.features.enabled(Feature::NewInit) && (self.user_type.is_ant() || new_init_env) {
            NEW_INIT_PROMPT
        } else {
            OLD_INIT_PROMPT
        }
    }
}

#[async_trait]
impl CommandHandler for InitPromptHandler {
    async fn execute_command(&self, _args: &str) -> anyhow::Result<CommandResult> {
        let body = self.select_prompt();
        Ok(CommandResult::Prompt {
            progress_message: "analyzing your codebase".into(),
            parts: vec![PromptPart::Text {
                text: body.to_string(),
            }],
        })
    }

    fn handler_name(&self) -> &str {
        "init"
    }
}

#[cfg(test)]
#[path = "init_prompt.test.rs"]
mod tests;
