//! Status command - show current configuration.

use cocode_config::ConfigManager;
use cocode_protocol::all_features;
use cocode_protocol::model::ModelRole;

/// Run the status command.
pub async fn run(config: &ConfigManager) -> anyhow::Result<()> {
    println!("Current Configuration");
    println!("─────────────────────");
    if let Some(spec) = config.current_spec_for_role(ModelRole::Main) {
        println!("Provider: {}", spec.provider);
        println!("Model:    {}", spec.slug);
    } else {
        println!("Model:    (not configured)");
    }

    // Show config path
    let config_path = config.config_path();
    println!("Config:   {}", config_path.display());

    // Show features summary
    let features = config.features();
    let enabled_count = all_features()
        .filter(|spec| features.enabled(spec.id))
        .count();
    println!("Features: {enabled_count} enabled");

    Ok(())
}
