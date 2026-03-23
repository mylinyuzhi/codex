//! Constrained configuration values with runtime validation.
//!
//! `Constrained<T>` wraps a value with a validator and optional normalizer,
//! ensuring the value satisfies invariants at construction and on every mutation.
//!
//! Inspired by codex-rs's constraint system for catching invalid configuration
//! at the boundary rather than at point of use.

use std::fmt;
use std::sync::Arc;

/// Validator function: returns `Ok(())` if the value is valid.
type ConstraintValidator<T> = dyn Fn(&T) -> Result<(), ConstraintError> + Send + Sync;

/// Normalizer function: transforms a value before validation.
type ConstraintNormalizer<T> = dyn Fn(T) -> T + Send + Sync;

/// A value wrapper that enforces invariants via validator and optional normalizer.
///
/// The validator runs on construction (`new`) and on every `set`. If validation
/// fails, the previous value is preserved.
pub struct Constrained<T> {
    value: T,
    validator: Arc<ConstraintValidator<T>>,
    normalizer: Option<Arc<ConstraintNormalizer<T>>>,
}

impl<T: fmt::Debug> fmt::Debug for Constrained<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Constrained")
            .field("value", &self.value)
            .finish_non_exhaustive()
    }
}

impl<T: Clone> Clone for Constrained<T> {
    fn clone(&self) -> Self {
        Self {
            value: self.value.clone(),
            validator: Arc::clone(&self.validator),
            normalizer: self.normalizer.as_ref().map(Arc::clone),
        }
    }
}

impl<T> Constrained<T> {
    /// Create a constrained value with a validator.
    ///
    /// Returns an error if the initial value fails validation.
    pub fn new(
        value: T,
        validator: impl Fn(&T) -> Result<(), ConstraintError> + Send + Sync + 'static,
    ) -> Result<Self, ConstraintError> {
        let validator = Arc::new(validator) as Arc<ConstraintValidator<T>>;
        validator(&value)?;
        Ok(Self {
            value,
            validator,
            normalizer: None,
        })
    }

    /// Create a constrained value with a normalizer (permissive validator).
    ///
    /// The normalizer transforms the value on init and every `set`.
    /// No validation failure is possible.
    pub fn normalized(value: T, normalizer: impl Fn(T) -> T + Send + Sync + 'static) -> Self {
        let normalizer = Arc::new(normalizer) as Arc<ConstraintNormalizer<T>>;
        let value = normalizer(value);
        Self {
            value,
            validator: Arc::new(|_| Ok(())),
            normalizer: Some(normalizer),
        }
    }

    /// Create an unconstrained value (accepts anything).
    pub fn allow_any(value: T) -> Self {
        Self {
            value,
            validator: Arc::new(|_| Ok(())),
            normalizer: None,
        }
    }

    /// Borrow the wrapped value.
    pub fn get(&self) -> &T {
        &self.value
    }

    /// Consume and return the inner value.
    pub fn into_inner(self) -> T {
        self.value
    }

    /// Set a new value. Normalizes first (if normalizer exists), then validates.
    ///
    /// On validation failure, the previous value is preserved.
    pub fn set(&mut self, value: T) -> Result<(), ConstraintError> {
        let value = match &self.normalizer {
            Some(normalizer) => normalizer(value),
            None => value,
        };
        (self.validator)(&value)?;
        self.value = value;
        Ok(())
    }

    /// Check if a candidate value would pass validation without mutating.
    pub fn can_set(&self, candidate: &T) -> Result<(), ConstraintError> {
        (self.validator)(candidate)
    }
}

impl<T: Copy> Constrained<T> {
    /// Copy out the wrapped value.
    pub fn value(&self) -> T {
        self.value
    }
}

impl<T: Clone + fmt::Debug + PartialEq + Send + Sync + 'static> Constrained<T> {
    /// Create a constrained value that is frozen to exactly `only_value`.
    ///
    /// Any attempt to `set` a different value will fail.
    pub fn allow_only(only_value: T) -> Self {
        let frozen = only_value.clone();
        Self {
            value: only_value,
            validator: Arc::new(move |v| {
                if v == &frozen {
                    Ok(())
                } else {
                    Err(ConstraintError::InvalidValue {
                        field_name: "frozen",
                        candidate: format!("{v:?}"),
                        allowed: format!("{frozen:?}"),
                    })
                }
            }),
            normalizer: None,
        }
    }
}

impl<T: Default> Constrained<T> {
    /// Create an unconstrained value starting from `T::default()`.
    pub fn allow_any_from_default() -> Self {
        Self::allow_any(T::default())
    }
}

/// Error when a constraint is violated.
#[derive(Debug, Clone)]
pub enum ConstraintError {
    /// Value is outside the allowed range or set.
    InvalidValue {
        /// Name of the configuration field.
        field_name: &'static str,
        /// String representation of the rejected value.
        candidate: String,
        /// Description of the allowed values.
        allowed: String,
    },
    /// Required field is empty or missing.
    EmptyField {
        /// Name of the configuration field.
        field_name: String,
    },
}

impl fmt::Display for ConstraintError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidValue {
                field_name,
                candidate,
                allowed,
            } => write!(
                f,
                "invalid value for {field_name}: {candidate} (allowed: {allowed})"
            ),
            Self::EmptyField { field_name } => {
                write!(f, "required field is empty: {field_name}")
            }
        }
    }
}

impl std::error::Error for ConstraintError {}

/// Validate `ModelInfo` fields that have natural constraints.
///
/// Returns a list of constraint violations found, or an empty vec if all valid.
pub fn validate_model_info_fields(info: &cocode_protocol::ModelInfo) -> Vec<ConstraintError> {
    let mut errors = Vec::new();

    if let Some(ctx) = info.context_window
        && !(1..=10_000_000).contains(&ctx)
    {
        errors.push(ConstraintError::InvalidValue {
            field_name: "context_window",
            candidate: format!("{ctx}"),
            allowed: "1..=10000000".to_string(),
        });
    }

    if let Some(max) = info.max_output_tokens
        && !(1..=10_000_000).contains(&max)
    {
        errors.push(ConstraintError::InvalidValue {
            field_name: "max_output_tokens",
            candidate: format!("{max}"),
            allowed: "1..=10000000".to_string(),
        });
    }

    if let Some(temp) = info.temperature
        && !(0.0..=2.0).contains(&temp)
    {
        errors.push(ConstraintError::InvalidValue {
            field_name: "temperature",
            candidate: format!("{temp}"),
            allowed: "0.0..=2.0".to_string(),
        });
    }

    if let Some(top_p) = info.top_p
        && !(0.0..=1.0).contains(&top_p)
    {
        errors.push(ConstraintError::InvalidValue {
            field_name: "top_p",
            candidate: format!("{top_p}"),
            allowed: "0.0..=1.0".to_string(),
        });
    }

    if let Some(timeout) = info.timeout_secs
        && !(1..=3600).contains(&timeout)
    {
        errors.push(ConstraintError::InvalidValue {
            field_name: "timeout_secs",
            candidate: format!("{timeout}"),
            allowed: "1..=3600".to_string(),
        });
    }

    errors
}

#[cfg(test)]
#[path = "constraint.test.rs"]
mod tests;
