use super::*;
use pretty_assertions::assert_eq;

#[test]
fn test_allow_any_accepts_all_values() {
    let mut c = Constrained::allow_any(42);
    assert_eq!(*c.get(), 42);
    assert!(c.set(0).is_ok());
    assert_eq!(*c.get(), 0);
    assert!(c.set(-999).is_ok());
    assert_eq!(*c.get(), -999);
}

#[test]
fn test_allow_only_rejects_different_value() {
    let mut c = Constrained::allow_only(10);
    assert_eq!(*c.get(), 10);

    // Same value is fine.
    assert!(c.set(10).is_ok());

    // Different value is rejected.
    let err = c.set(20).unwrap_err();
    assert!(matches!(err, ConstraintError::InvalidValue { .. }));

    // Value preserved after failed set.
    assert_eq!(*c.get(), 10);
}

#[test]
fn test_new_validates_initial_value() {
    let result = Constrained::new(5, |v| {
        if *v >= 1 && *v <= 10 {
            Ok(())
        } else {
            Err(ConstraintError::InvalidValue {
                field_name: "x",
                candidate: format!("{v}"),
                allowed: "1..=10".to_string(),
            })
        }
    });
    assert!(result.is_ok());

    let result = Constrained::new(99, |v| {
        if *v >= 1 && *v <= 10 {
            Ok(())
        } else {
            Err(ConstraintError::InvalidValue {
                field_name: "x",
                candidate: format!("{v}"),
                allowed: "1..=10".to_string(),
            })
        }
    });
    assert!(result.is_err());
}

#[test]
fn test_normalizer_applied_on_init_and_set() {
    let mut c = Constrained::normalized(-5_i32, |v| v.max(0));
    // Normalizer clamps -5 → 0 on init.
    assert_eq!(*c.get(), 0);

    // Normalizer clamps -10 → 0 on set.
    assert!(c.set(-10).is_ok());
    assert_eq!(*c.get(), 0);

    // Positive values pass through.
    assert!(c.set(7).is_ok());
    assert_eq!(*c.get(), 7);
}

#[test]
fn test_can_set_does_not_mutate() {
    let c = Constrained::new(5, |v| {
        if *v >= 0 {
            Ok(())
        } else {
            Err(ConstraintError::InvalidValue {
                field_name: "x",
                candidate: format!("{v}"),
                allowed: ">=0".to_string(),
            })
        }
    })
    .unwrap();

    assert!(c.can_set(&10).is_ok());
    assert!(c.can_set(&-1).is_err());
    // Original value unchanged.
    assert_eq!(*c.get(), 5);
}

#[test]
fn test_failed_set_preserves_previous_value() {
    let mut c = Constrained::new(5, |v| {
        if *v >= 0 {
            Ok(())
        } else {
            Err(ConstraintError::InvalidValue {
                field_name: "x",
                candidate: format!("{v}"),
                allowed: ">=0".to_string(),
            })
        }
    })
    .unwrap();

    assert!(c.set(-1).is_err());
    assert_eq!(*c.get(), 5);
}

#[test]
fn test_value_copies_for_copy_types() {
    let c = Constrained::allow_any(42_i32);
    let v: i32 = c.value();
    assert_eq!(v, 42);
}

#[test]
fn test_into_inner_consumes() {
    let c = Constrained::allow_any("hello".to_string());
    let s = c.into_inner();
    assert_eq!(s, "hello");
}

#[test]
fn test_allow_any_from_default() {
    let c = Constrained::<i32>::allow_any_from_default();
    assert_eq!(*c.get(), 0);
}

#[test]
fn test_clone_preserves_constraints() {
    let c = Constrained::allow_only(42);
    let mut c2 = c.clone();
    assert_eq!(*c2.get(), 42);
    assert!(c2.set(99).is_err());
    // Verify original is unaffected by clone's mutation attempt.
    assert_eq!(*c.get(), 42);
}

// === ModelInfo validation tests ===

#[test]
fn test_validate_model_info_valid() {
    let info = cocode_protocol::ModelInfo {
        slug: "test".to_string(),
        context_window: Some(200_000),
        max_output_tokens: Some(8192),
        temperature: Some(1.0),
        top_p: Some(0.9),
        timeout_secs: Some(120),
        ..Default::default()
    };
    let errors = validate_model_info_fields(&info);
    assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
}

#[test]
fn test_validate_model_info_invalid_context_window() {
    let info = cocode_protocol::ModelInfo {
        slug: "test".to_string(),
        context_window: Some(-1),
        ..Default::default()
    };
    let errors = validate_model_info_fields(&info);
    assert_eq!(errors.len(), 1);
    assert!(matches!(
        &errors[0],
        ConstraintError::InvalidValue {
            field_name: "context_window",
            ..
        }
    ));
}

#[test]
fn test_validate_model_info_invalid_temperature() {
    let info = cocode_protocol::ModelInfo {
        slug: "test".to_string(),
        temperature: Some(5.0),
        ..Default::default()
    };
    let errors = validate_model_info_fields(&info);
    assert_eq!(errors.len(), 1);
    assert!(matches!(
        &errors[0],
        ConstraintError::InvalidValue {
            field_name: "temperature",
            ..
        }
    ));
}

#[test]
fn test_validate_model_info_none_fields_ok() {
    let info = cocode_protocol::ModelInfo {
        slug: "test".to_string(),
        ..Default::default()
    };
    let errors = validate_model_info_fields(&info);
    assert!(errors.is_empty());
}

#[test]
fn test_constraint_error_display() {
    let err = ConstraintError::InvalidValue {
        field_name: "temperature",
        candidate: "5.0".to_string(),
        allowed: "0.0..=2.0".to_string(),
    };
    let msg = err.to_string();
    assert!(msg.contains("temperature"));
    assert!(msg.contains("5.0"));
    assert!(msg.contains("0.0..=2.0"));

    let err = ConstraintError::EmptyField {
        field_name: "slug".to_string(),
    };
    assert!(err.to_string().contains("slug"));
}
