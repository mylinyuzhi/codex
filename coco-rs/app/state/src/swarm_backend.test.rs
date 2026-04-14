use super::*;

#[test]
fn test_system_prompt_mode_default() {
    assert_eq!(SystemPromptMode::default(), SystemPromptMode::Default);
}

#[test]
fn test_backend_detection_result_clone() {
    let result = BackendDetectionResult {
        backend_type: BackendType::Tmux,
        is_native: true,
        needs_it2_setup: false,
    };
    let cloned = result;
    assert_eq!(cloned.backend_type, BackendType::Tmux);
    assert!(cloned.is_native);
}

#[test]
fn test_backend_registry_new() {
    let _registry = BackendRegistry::new();
    let _registry2 = BackendRegistry::default();
}

#[tokio::test]
async fn test_backend_registry_reset() {
    let registry = BackendRegistry::new();
    registry.mark_in_process_fallback().await;
    assert!(registry.is_in_process_enabled().await);

    registry.reset().await;
    assert!(!registry.is_in_process_enabled().await);
}

#[tokio::test]
async fn test_backend_registry_in_process_mode() {
    let registry = BackendRegistry::new();

    // Default: not in-process
    assert!(!registry.is_in_process_enabled().await);
    assert_eq!(registry.get_resolved_teammate_mode().await, "tmux");

    // Mark fallback
    registry.mark_in_process_fallback().await;
    assert!(registry.is_in_process_enabled().await);
    assert_eq!(registry.get_resolved_teammate_mode().await, "in-process");
}

#[tokio::test]
async fn test_get_teammate_executor_none_when_empty() {
    let registry = BackendRegistry::new();
    assert!(
        registry
            .get_teammate_executor(/*prefer_in_process*/ false)
            .await
            .is_none()
    );
    assert!(
        registry
            .get_teammate_executor(/*prefer_in_process*/ true)
            .await
            .is_none()
    );
}

#[test]
fn test_is_inside_tmux() {
    // This test's result depends on the environment, just ensure no panic
    let _result = is_inside_tmux();
}

#[test]
fn test_get_leader_pane_id() {
    let _result = get_leader_pane_id();
}

#[test]
fn test_is_in_iterm2() {
    let _result = is_in_iterm2();
}

#[tokio::test]
async fn test_detect_backend_no_panic() {
    // Detection should complete without panicking regardless of env
    let _result = detect_backend_impl().await;
}
