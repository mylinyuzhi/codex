use super::*;

#[test]
fn test_agent_color_to_tmux() {
    assert_eq!(agent_color_to_tmux(AgentColorName::Red), "red");
    assert_eq!(agent_color_to_tmux(AgentColorName::Purple), "magenta");
    assert_eq!(agent_color_to_tmux(AgentColorName::Orange), "colour208");
    assert_eq!(agent_color_to_tmux(AgentColorName::Pink), "colour213");
    assert_eq!(agent_color_to_tmux(AgentColorName::Cyan), "cyan");
}

#[test]
fn test_tmux_backend_properties() {
    let backend = TmuxBackend::new(/*is_native*/ true);
    assert_eq!(backend.backend_type(), BackendType::Tmux);
    assert_eq!(backend.display_name(), "tmux");
    assert!(backend.supports_hide_show());
}

#[test]
fn test_tmux_backend_external_properties() {
    let backend = TmuxBackend::new(/*is_native*/ false);
    assert_eq!(backend.backend_type(), BackendType::Tmux);
    assert!(!backend.is_native);
}

#[test]
fn test_pane_shell_init_delay() {
    assert_eq!(PANE_SHELL_INIT_DELAY_MS, 200);
}
