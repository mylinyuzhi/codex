//! `coco sessions` subcommand handler.
//!
//! `coco resume` itself is wired in `main.rs` so it shares the same
//! TUI-bootstrap path as `coco --resume <id>` and `coco --continue`.

use anyhow::Result;
use coco_cli::paths::sessions_dir;
use coco_session::SessionManager;

pub fn handle_sessions() -> Result<()> {
    let mgr = SessionManager::new(sessions_dir());
    let sessions = mgr.list()?;

    if sessions.is_empty() {
        println!("No sessions found.");
        return Ok(());
    }

    println!(
        "{:<38}  {:<30}  {:<12}  Working Dir",
        "ID", "Model", "Created"
    );
    println!("{}", "-".repeat(100));
    for s in &sessions {
        println!(
            "{:<38}  {:<30}  {:<12}  {}",
            s.id,
            s.model,
            &s.created_at,
            s.working_dir.display()
        );
    }
    println!("\n{} session(s) total.", sessions.len());
    Ok(())
}
