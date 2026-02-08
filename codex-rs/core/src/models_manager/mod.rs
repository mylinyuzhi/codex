pub mod cache;
pub mod collaboration_mode_presets;
pub mod manager;
pub mod model_info;
pub mod model_info_config;
pub mod model_info_ext;
pub mod model_info_registry;
pub mod model_presets;
pub mod provider_preset;

pub use model_info_config::ModelInfoConfig;
pub use model_info_registry::init_registry;
pub use model_info_registry::resolve_model_info;

#[cfg(any(test, feature = "test-support"))]
pub use collaboration_mode_presets::test_builtin_collaboration_mode_presets;

/// Convert the client version string to a whole version string (e.g. "1.2.3-alpha.4" -> "1.2.3").
pub fn client_version_to_whole() -> String {
    format!(
        "{}.{}.{}",
        env!("CARGO_PKG_VERSION_MAJOR"),
        env!("CARGO_PKG_VERSION_MINOR"),
        env!("CARGO_PKG_VERSION_PATCH")
    )
}
