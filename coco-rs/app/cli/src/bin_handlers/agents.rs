//! `coco agents` — list discovered agent definitions.
//!
//! TS: `src/cli/handlers/agents.ts` — walks the standard agent dirs and
//! prints a flat list. Rust mirrors the same discovery sources via the
//! `coco-subagent` catalog (built-ins + per-source markdown loaders).

use anyhow::Result;

use coco_cli::paths::standard_agent_search_paths;
use coco_config::global_config;

pub async fn run_agents_subcommand() -> Result<()> {
    let cwd = std::env::current_dir().unwrap_or_default();
    let config_home = global_config::config_home();
    let paths = standard_agent_search_paths(&config_home, &cwd);
    let mut store = coco_subagent::AgentDefinitionStore::new(
        coco_subagent::BuiltinAgentCatalog::interactive(),
        paths.clone(),
    );
    store.load();
    let snapshot = store.snapshot();

    if snapshot.active_count() == 0 {
        let searched: Vec<String> = paths
            .user_dir
            .iter()
            .chain(paths.project_dirs.iter())
            .map(|p| p.display().to_string())
            .collect();
        println!("No agents found.");
        println!("Searched: {}", searched.join(", "));
        return Ok(());
    }

    let mut agents: Vec<&coco_types::AgentDefinition> = snapshot.active().collect();
    agents.sort_by(|a, b| a.name.cmp(&b.name));
    println!("{} agent(s):", agents.len());
    for agent in &agents {
        let model = agent.model.as_deref().unwrap_or("inherit");
        let desc = agent.description.as_deref().unwrap_or("(no description)");
        println!("  {} · {model}  — {desc}", agent.name);
    }
    Ok(())
}
