//! `coco sessions` and `coco resume` subcommand handlers.

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

pub fn handle_resume(session_id: Option<&str>) -> Result<()> {
    let mgr = SessionManager::new(sessions_dir());

    let session = if let Some(id) = session_id {
        mgr.resume(id)?
    } else {
        match mgr.most_recent()? {
            Some(s) => {
                println!("Resuming most recent session: {}", s.id);
                mgr.resume(&s.id)?
            }
            None => {
                println!("No sessions to resume.");
                return Ok(());
            }
        }
    };

    println!("Session: {}", session.id);
    println!("Model: {}", session.model);
    println!("Working dir: {}", session.working_dir.display());
    println!("Messages: {}", session.message_count);
    if let Some(title) = &session.title {
        println!("Title: {title}");
    }
    println!("\nSession resumed. Run `coco` to continue the conversation.");
    Ok(())
}
