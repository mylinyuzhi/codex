//! Unified configuration validation system.
//!
//! This module consolidates validation logic that was previously scattered across
//! resolver.rs, manager.rs, and types.rs. It provides a single unified validation
//! interface with clear phases and error reporting.

use crate::error::ConfigError;
use cocode_protocol::ModelInfo;
use cocode_protocol::ProviderInfo;
use std::collections::HashMap;

/// Validation phase for better error context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationPhase {
    /// JSON schema validation (file load time)
    FileLoad,
    /// Provider/model existence checks (resolution time)
    Resolution,
    /// Runtime constraints (before use)
    Runtime,
}

/// A single validation error with context.
#[derive(Debug, Clone)]
pub struct ValidationError {
    /// Which phase this validation failed in
    pub phase: ValidationPhase,
    /// The field that failed validation
    pub field: String,
    /// Human-readable error message
    pub message: String,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{:?}] {}: {}",
            self.phase, self.field, self.message
        )
    }
}

/// Unified validator for configuration.
///
/// Consolidates validation logic from multiple sources into a single interface.
pub struct Validator {
    phase: ValidationPhase,
    errors: Vec<ValidationError>,
}

impl Validator {
    /// Create a new validator for a specific phase.
    pub fn new(phase: ValidationPhase) -> Self {
        Self {
            phase,
            errors: Vec::new(),
        }
    }

    /// Add an error to the validator.
    pub fn add_error(&mut self, field: String, message: String) {
        self.errors.push(ValidationError {
            phase: self.phase,
            field,
            message,
        });
    }

    /// Check if validation passed (no errors).
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }

    /// Get all accumulated errors.
    pub fn errors(&self) -> &[ValidationError] {
        &self.errors
    }

    /// Finish validation and return errors if any.
    pub fn finish(self) -> Result<(), Vec<ValidationError>> {
        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(self.errors)
        }
    }

    /// Validate models exist and have required fields.
    pub fn check_models(
        &mut self,
        models: &HashMap<cocode_protocol::model::ModelRole, ModelInfo>,
    ) {
        for (role, info) in models {
            if info.context_window.is_none() {
                self.add_error(
                    format!("{:?}.context_window", role),
                    "context_window is required for all models".to_string(),
                );
            }
            if info.slug.is_empty() {
                self.add_error(
                    format!("{:?}.slug", role),
                    "model slug cannot be empty".to_string(),
                );
            }
        }
    }

    /// Validate providers exist and have required fields.
    pub fn check_providers(&mut self, providers: &HashMap<String, ProviderInfo>) {
        for (name, _info) in providers {
            // Basic validation that provider name is not empty
            if name.is_empty() {
                self.add_error(
                    "provider.name".to_string(),
                    "provider name cannot be empty".to_string(),
                );
            }
        }
    }

    /// Validate features configuration.
    pub fn check_features(&mut self, _features: &cocode_protocol::Features) {
        // Features validation framework ready for implementation
        // Future: validate that all configured features are known
    }

    /// Validate all configuration (models, providers, features).
    pub fn validate_all(
        models: &HashMap<cocode_protocol::model::ModelRole, ModelInfo>,
        providers: &HashMap<String, ProviderInfo>,
        features: &cocode_protocol::Features,
    ) -> Result<(), Vec<ValidationError>> {
        let mut validator = Self::new(ValidationPhase::Runtime);
        validator.check_models(models);
        validator.check_providers(providers);
        validator.check_features(features);
        validator.finish()
    }
}

/// Validate that a provider exists in the system.
///
/// This is a simple check used by manager.rs before allowing provider switches.
/// Returns Ok if provider exists, Err otherwise.
pub fn validate_provider_exists(
    provider: &str,
    has_provider_fn: impl Fn(&str) -> bool,
) -> Result<(), String> {
    if !has_provider_fn(provider) {
        return Err(format!("provider not found: {}", provider));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validator_new() {
        let validator = Validator::new(ValidationPhase::Runtime);
        assert!(validator.is_valid());
    }

    #[test]
    fn test_validator_add_error() {
        let mut validator = Validator::new(ValidationPhase::Runtime);
        validator.add_error("field".to_string(), "error message".to_string());
        assert!(!validator.is_valid());
        assert_eq!(validator.errors().len(), 1);
    }

    #[test]
    fn test_validator_finish_valid() {
        let validator = Validator::new(ValidationPhase::Runtime);
        assert!(validator.finish().is_ok());
    }

    #[test]
    fn test_validator_finish_invalid() {
        let mut validator = Validator::new(ValidationPhase::Runtime);
        validator.add_error("field".to_string(), "error".to_string());
        let result = validator.finish();
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().len(), 1);
    }

    #[test]
    fn test_validation_error_display() {
        let error = ValidationError {
            phase: ValidationPhase::Runtime,
            field: "test_field".to_string(),
            message: "test error".to_string(),
        };
        let display = format!("{}", error);
        assert!(display.contains("test_field"));
        assert!(display.contains("test error"));
    }
}
