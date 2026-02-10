use crate::error::Result;
use crate::session::SessionManager;

/// Register a session with a role.
pub async fn run_register(
    session_id: &str,
    role: &str,
    task: Option<&str>,
    prompt_file: Option<&str>,
) -> Result<()> {
    let team_id = std::env::var("CLAUDE_TEAM_ID").ok();
    let session_mgr = SessionManager::new(team_id.as_deref());

    // Validate the role exists
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let roles = crate::config::RolesConfig::load_project(&cwd)?;
    if roles.get_role(role).is_none() {
        eprintln!("hookwise: unknown role '{}'. Available roles:", role);
        for name in roles.roles.keys() {
            eprintln!("  - {}", name);
        }
        std::process::exit(1);
    }

    session_mgr.register(session_id, role, task, prompt_file)?;
    eprintln!(
        "hookwise: session {} registered as '{}'",
        session_id, role
    );
    Ok(())
}

/// Disable hookwise for a session.
pub async fn run_disable(session_id: &str) -> Result<()> {
    let team_id = std::env::var("CLAUDE_TEAM_ID").ok();
    let session_mgr = SessionManager::new(team_id.as_deref());

    session_mgr.disable(session_id)?;
    eprintln!("hookwise: session {} disabled", session_id);
    Ok(())
}

/// Re-enable hookwise for a session.
pub async fn run_enable(session_id: &str) -> Result<()> {
    let team_id = std::env::var("CLAUDE_TEAM_ID").ok();
    let session_mgr = SessionManager::new(team_id.as_deref());

    session_mgr.enable(session_id)?;
    eprintln!("hookwise: session {} re-enabled", session_id);
    Ok(())
}
