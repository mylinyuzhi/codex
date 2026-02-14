//! Session metadata types.
//!
//! This module defines the [`Session`] struct which holds metadata about
//! an agent session including identity, timestamps, and configuration.

use chrono::DateTime;
use chrono::Utc;
use cocode_protocol::ModelSpec;
use cocode_protocol::ProviderType;
use cocode_protocol::RoleSelection;
use cocode_protocol::RoleSelections;
use cocode_protocol::model::ModelRole;
use serde::Deserialize;
use serde::Serialize;
use std::path::PathBuf;

/// Session metadata for an agent conversation.
///
/// A session represents a single conversation with one or more LLM providers.
/// It tracks identity, timestamps, role selections, and configuration but does not
/// hold the actual conversation history (see [`SessionState`]).
///
/// # Multi-Model Support
///
/// Session supports multi-model configurations through `RoleSelections`.
/// Each role (Main, Fast, Vision, etc.) can have a different model configured.
///
/// # Example
///
/// ```
/// use cocode_session::Session;
/// use cocode_protocol::{ProviderType, ModelSpec, RoleSelection};
/// use std::path::PathBuf;
///
/// // Create session with main model
/// let session = Session::new(
///     PathBuf::from("/project"),
///     RoleSelection::new(ModelSpec::new("openai", "gpt-5")),
/// );
///
/// println!("Session ID: {}", session.id);
/// if let Some(main) = session.primary_model() {
///     println!("Main model: {}", main.model);
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Unique session identifier (UUID v4).
    pub id: String,

    /// Session creation timestamp.
    pub created_at: DateTime<Utc>,

    /// Last activity timestamp.
    pub last_activity_at: DateTime<Utc>,

    /// Working directory for the session.
    pub working_dir: PathBuf,

    /// Role selections for all model roles (Main, Fast, Vision, etc.).
    ///
    /// At minimum, the `main` role should be set. Other roles are optional
    /// and will fall back to `main` when not explicitly configured.
    pub selections: RoleSelections,

    /// Maximum turns before stopping (None = unlimited).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<i32>,

    /// Session title (optional, user-provided or auto-generated).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Whether the session is ephemeral (not persisted).
    #[serde(default)]
    pub ephemeral: bool,
}

impl Session {
    /// Create a new session with the given main model selection.
    ///
    /// Generates a new UUID and sets timestamps to now.
    pub fn new(working_dir: PathBuf, main_selection: RoleSelection) -> Self {
        let now = Utc::now();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            created_at: now,
            last_activity_at: now,
            working_dir,
            selections: RoleSelections::with_main(main_selection),
            max_turns: Some(200),
            title: None,
            ephemeral: false,
        }
    }

    /// Create a new session with a specific ID.
    ///
    /// Useful for resuming sessions or testing.
    pub fn with_id(
        id: impl Into<String>,
        working_dir: PathBuf,
        main_selection: RoleSelection,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: id.into(),
            created_at: now,
            last_activity_at: now,
            working_dir,
            selections: RoleSelections::with_main(main_selection),
            max_turns: Some(200),
            title: None,
            ephemeral: false,
        }
    }

    /// Create a new session from full role selections.
    ///
    /// Use this when you have pre-configured role selections
    /// (e.g., loaded from configuration).
    pub fn with_selections(working_dir: PathBuf, selections: RoleSelections) -> Self {
        let now = Utc::now();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            created_at: now,
            last_activity_at: now,
            working_dir,
            selections,
            max_turns: Some(200),
            title: None,
            ephemeral: false,
        }
    }

    /// Create a builder for customizing session options.
    pub fn builder() -> SessionBuilder {
        SessionBuilder::new()
    }

    /// Update the last activity timestamp to now.
    pub fn touch(&mut self) {
        self.last_activity_at = Utc::now();
    }

    /// Set the session title.
    pub fn set_title(&mut self, title: impl Into<String>) {
        self.title = Some(title.into());
    }

    /// Set the max turns limit.
    pub fn set_max_turns(&mut self, max: Option<i32>) {
        self.max_turns = max;
    }

    /// Mark the session as ephemeral (not persisted).
    pub fn set_ephemeral(&mut self, ephemeral: bool) {
        self.ephemeral = ephemeral;
    }

    /// Get the session age in seconds.
    pub fn age_secs(&self) -> i64 {
        (Utc::now() - self.created_at).num_seconds()
    }

    /// Get seconds since last activity.
    pub fn idle_secs(&self) -> i64 {
        (Utc::now() - self.last_activity_at).num_seconds()
    }

    // ==========================================================
    // Role Selection Accessors
    // ==========================================================

    /// Get the primary (main) model selection.
    ///
    /// Returns the main role selection, which is the default model
    /// used for most operations.
    pub fn primary_model(&self) -> Option<&RoleSelection> {
        self.selections.main.as_ref()
    }

    /// Get the model selection for a specific role.
    ///
    /// Returns `None` if the role is not explicitly configured.
    pub fn model_for_role(&self, role: ModelRole) -> Option<&RoleSelection> {
        self.selections.get(role)
    }

    /// Get the model selection for a role, falling back to main.
    ///
    /// If the requested role is not configured, returns the main role selection.
    pub fn model_for_role_or_main(&self, role: ModelRole) -> Option<&RoleSelection> {
        self.selections.get_or_main(role)
    }

    /// Set the model selection for a specific role.
    pub fn set_model_for_role(&mut self, role: ModelRole, selection: RoleSelection) {
        self.selections.set(role, selection);
    }

    /// Get the primary provider name.
    ///
    /// Returns the provider from the main role selection.
    pub fn provider(&self) -> Option<&str> {
        self.selections
            .main
            .as_ref()
            .map(cocode_protocol::RoleSelection::provider)
    }

    /// Get the primary provider type.
    ///
    /// Returns the provider type from the main role selection.
    pub fn provider_type(&self) -> Option<ProviderType> {
        self.selections.main.as_ref().map(|s| s.model.provider_type)
    }

    /// Get the primary model name.
    ///
    /// Returns the model name from the main role selection.
    pub fn model(&self) -> Option<&str> {
        self.selections
            .main
            .as_ref()
            .map(cocode_protocol::RoleSelection::model_name)
    }

    /// Get the primary model spec.
    ///
    /// Returns the full ModelSpec from the main role selection.
    pub fn model_spec(&self) -> Option<&ModelSpec> {
        self.selections.main.as_ref().map(|s| &s.model)
    }
}

/// Builder for creating [`Session`] instances.
#[derive(Debug, Default)]
pub struct SessionBuilder {
    working_dir: Option<PathBuf>,
    selections: Option<RoleSelections>,
    main_selection: Option<RoleSelection>,
    max_turns: Option<i32>,
    title: Option<String>,
    ephemeral: bool,
}

impl SessionBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the working directory.
    pub fn working_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.working_dir = Some(path.into());
        self
    }

    /// Set the main model selection.
    ///
    /// This is a convenience method for setting just the main role.
    pub fn main_selection(mut self, selection: RoleSelection) -> Self {
        self.main_selection = Some(selection);
        self
    }

    /// Set the main model using provider and model strings.
    ///
    /// This is a convenience method that creates a ModelSpec and RoleSelection.
    pub fn model(mut self, provider: &str, model: &str) -> Self {
        let spec = ModelSpec::new(provider, model);
        self.main_selection = Some(RoleSelection::new(spec));
        self
    }

    /// Set the main model with explicit provider type.
    ///
    /// Use this when you know the exact provider type.
    pub fn model_with_type(
        mut self,
        provider: &str,
        provider_type: ProviderType,
        model: &str,
    ) -> Self {
        let spec = ModelSpec::with_type(provider, provider_type, model);
        self.main_selection = Some(RoleSelection::new(spec));
        self
    }

    /// Set full role selections.
    ///
    /// This overrides any main_selection set previously.
    pub fn selections(mut self, selections: RoleSelections) -> Self {
        self.selections = Some(selections);
        self
    }

    /// Set the max turns.
    pub fn max_turns(mut self, max: i32) -> Self {
        self.max_turns = Some(max);
        self
    }

    /// Set the title.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Set ephemeral mode.
    pub fn ephemeral(mut self, ephemeral: bool) -> Self {
        self.ephemeral = ephemeral;
        self
    }

    /// Build the session.
    ///
    /// # Panics
    ///
    /// Panics if working_dir is not set, or if neither selections nor main_selection is set.
    #[allow(clippy::expect_used)]
    pub fn build(self) -> Session {
        let working_dir = self.working_dir.expect("working_dir is required");

        // Determine selections: explicit selections take precedence over main_selection
        let selections = if let Some(selections) = self.selections {
            selections
        } else if let Some(main_selection) = self.main_selection {
            RoleSelections::with_main(main_selection)
        } else {
            panic!("Either selections or main_selection is required")
        };

        let mut session = Session::with_selections(working_dir, selections);

        if let Some(max) = self.max_turns {
            session.max_turns = Some(max);
        }
        session.title = self.title;
        session.ephemeral = self.ephemeral;

        session
    }
}

#[cfg(test)]
#[path = "session.test.rs"]
mod tests;
