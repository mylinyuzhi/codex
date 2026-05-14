//! Output styles — TS-parity definitions, loaders, and resolution.
//!
//! ## TS Source
//!
//! - `constants/outputStyles.ts` — `OutputStyleConfig`, built-in catalog
//!   (`default` / `Explanatory` / `Learning`), `getAllOutputStyles`,
//!   `getOutputStyleConfig`, `hasCustomOutputStyle`.
//! - `outputStyles/loadOutputStylesDir.ts` — reads project + user
//!   `.coco/output-styles/*.md` via the markdown discovery loader,
//!   parses `name` / `description` / `keep-coding-instructions`.
//! - `utils/plugins/loadPluginOutputStyles.ts` — reads
//!   `<plugin>/output-styles/*.md` plus manifest `outputStyles` extras,
//!   parses `force-for-plugin`, namespaces names as `pluginName:baseName`.
//! - `constants/prompts.ts` — system-prompt injection
//!   (`# Output Style: <name>\n<prompt>` block + intro toggle +
//!   `keepCodingInstructions` gate on the doing-tasks section).
//! - `utils/messages.ts:3797` and `utils/attachments.ts:1597` —
//!   per-turn reminder template + main-thread, non-default gate.
//! - `commands/output-style/output-style.tsx` — deprecated CLI stub.
//!
//! ## Architecture
//!
//! Pure-logic crate at root tier (alongside `commands/`, `skills/`,
//! `plugins/`). No async runtime needed; no config dependency. The CLI
//! wires the resolved [`OutputStyleConfig`] into:
//!
//! - [`coco_context::build_system_prompt`] (system-prompt section)
//! - `coco_query::SessionBootstrap` (per-turn reminder name + SDK init)
//! - The SDK `available_output_styles` accessor.
//!
//! The crate intentionally does not depend on `coco_config` so it can
//! be reused by any caller that already has a settings-derived
//! `output_style` name; callers thread the resolved name in.

pub mod builtin;
pub mod catalog;
pub mod dir_loader;
pub mod error;
pub mod manager;
pub mod plugin_loader;
pub mod resolver;

pub use builtin::DEFAULT_OUTPUT_STYLE_NAME;
pub use builtin::EXPLANATORY_STYLE_NAME;
pub use builtin::LEARNING_STYLE_NAME;
pub use builtin::builtin_styles;
pub use catalog::OutputStyleConfig;
pub use catalog::OutputStyleSource;
pub use dir_loader::load_dir_styles;
pub use error::OutputStylesError;
pub use manager::OutputStyleManager;
pub use plugin_loader::PluginOutputStyleSource;
pub use plugin_loader::load_plugin_output_styles;
pub use resolver::Aggregated;
pub use resolver::ForceForPluginVerdict;
pub use resolver::aggregate;
pub use resolver::resolve_active_style;

/// Crate-local Result alias. The default error is [`OutputStylesError`]
/// but the open generic preserves `Result::ok` / 2-arg
/// `Result<T, E>` resolution at callsites.
pub type Result<T, E = OutputStylesError> = std::result::Result<T, E>;
