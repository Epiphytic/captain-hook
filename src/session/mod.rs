pub mod context;
pub mod registration;

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::LazyLock;

use crate::config::{CompiledPathPolicy, PolicyConfig, RoleDefinition, RolesConfig};
use crate::error::{CaptainHookError, Result};

/// In-memory session context, populated on first tool call from a session.
#[derive(Debug, Clone)]
pub struct SessionContext {
    pub user: String,
    pub org: String,
    pub project: String,
    pub team: Option<String>,
    pub role: Option<RoleDefinition>,
    pub path_policy: Option<std::sync::Arc<CompiledPathPolicy>>,
    pub agent_prompt_hash: Option<String>,
    pub agent_prompt_path: Option<PathBuf>,
    pub task_description: Option<String>,
    pub registered_at: Option<DateTime<Utc>>,
    pub disabled: bool,
}

/// Global concurrent session cache.
pub static SESSIONS: LazyLock<DashMap<String, SessionContext>> = LazyLock::new(DashMap::new);

/// Manages session registration and lookup.
pub struct SessionManager {
    registration_file: PathBuf,
    exclusion_file: PathBuf,
}

impl SessionManager {
    pub fn new(team_id: Option<&str>) -> Self {
        let suffix = team_id.unwrap_or("solo");
        let runtime_dir = runtime_dir();
        Self {
            registration_file: runtime_dir.join(format!("captain-hook-{suffix}-sessions.json")),
            exclusion_file: runtime_dir.join(format!("captain-hook-{suffix}-exclusions.json")),
        }
    }

    /// Resolve a session's role. Checks in order:
    /// 1. In-memory cache (SESSIONS DashMap)
    /// 2. Registration file on disk
    /// 3. CAPTAIN_HOOK_ROLE env var
    pub fn resolve_role(&self, session_id: &str) -> Result<Option<RoleDefinition>> {
        // 1. Check in-memory cache
        if let Some(ctx) = SESSIONS.get(session_id) {
            return Ok(ctx.role.clone());
        }

        // 2. Check registration file on disk
        let entries = registration::read_registration_file(&self.registration_file)?;
        if let Some(entry) = entries.get(session_id) {
            // We have a registration entry -- resolve the role from roles.yml
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let roles = RolesConfig::load_project(&cwd)?;
            return Ok(roles.get_role(&entry.role).cloned());
        }

        // 3. Check env var fallback
        if let Ok(role_name) = std::env::var("CAPTAIN_HOOK_ROLE") {
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let roles = RolesConfig::load_project(&cwd)?;
            return Ok(roles.get_role(&role_name).cloned());
        }

        Ok(None)
    }

    /// Get the full session context, populating if needed.
    pub fn get_or_populate(&self, session_id: &str, cwd: &str) -> Result<SessionContext> {
        // Check in-memory cache first
        if let Some(ctx) = SESSIONS.get(session_id) {
            return Ok(ctx.clone());
        }

        // Populate from registration file + git info
        let (org, project) = extract_git_org_project(cwd);
        let user = whoami();
        let team = std::env::var("CLAUDE_TEAM_ID").ok();

        let mut ctx = SessionContext {
            user,
            org,
            project,
            team,
            role: None,
            path_policy: None,
            agent_prompt_hash: None,
            agent_prompt_path: None,
            task_description: None,
            registered_at: None,
            disabled: false,
        };

        // Check if disabled
        if self.is_disabled(session_id) {
            ctx.disabled = true;
            SESSIONS.insert(session_id.to_string(), ctx.clone());
            return Ok(ctx);
        }

        // Check registration file
        let entries = registration::read_registration_file(&self.registration_file)?;
        if let Some(entry) = entries.get(session_id) {
            let cwd_path = PathBuf::from(cwd);
            let roles = RolesConfig::load_project(&cwd_path)?;
            let policy = PolicyConfig::load_project(&cwd_path)?;

            if let Some(role_def) = roles.get_role(&entry.role) {
                let compiled = CompiledPathPolicy::compile(
                    &role_def.paths,
                    &policy.sensitive_paths.ask_write,
                )?;
                ctx.path_policy = Some(std::sync::Arc::new(compiled));
                ctx.role = Some(role_def.clone());
            }

            ctx.task_description = entry.task.clone();
            ctx.agent_prompt_hash = entry.prompt_hash.clone();
            ctx.agent_prompt_path = entry.prompt_path.as_ref().map(PathBuf::from);
            ctx.registered_at = Some(entry.registered_at);
        } else if let Ok(role_name) = std::env::var("CAPTAIN_HOOK_ROLE") {
            // Env var fallback
            let cwd_path = PathBuf::from(cwd);
            let roles = RolesConfig::load_project(&cwd_path)?;
            let policy = PolicyConfig::load_project(&cwd_path)?;

            if let Some(role_def) = roles.get_role(&role_name) {
                let compiled = CompiledPathPolicy::compile(
                    &role_def.paths,
                    &policy.sensitive_paths.ask_write,
                )?;
                ctx.path_policy = Some(std::sync::Arc::new(compiled));
                ctx.role = Some(role_def.clone());
                ctx.registered_at = Some(Utc::now());
            }
        }

        SESSIONS.insert(session_id.to_string(), ctx.clone());
        Ok(ctx)
    }

    /// Register a session with a role.
    pub fn register(
        &self,
        session_id: &str,
        role_name: &str,
        task: Option<&str>,
        prompt_file: Option<&str>,
    ) -> Result<()> {
        let prompt_hash = prompt_file.and_then(|p| {
            std::fs::read(p).ok().map(|bytes| {
                use sha2::{Digest, Sha256};
                let hash = Sha256::digest(&bytes);
                format!("{:x}", hash)
            })
        });

        let entry = RegistrationEntry {
            role: role_name.to_string(),
            task: task.map(String::from),
            prompt_hash,
            prompt_path: prompt_file.map(String::from),
            registered_at: Utc::now(),
            registered_by: None,
        };

        registration::write_registration_entry(&self.registration_file, session_id, &entry)?;

        // Also remove from exclusion file if present
        if self.is_disabled(session_id) {
            self.remove_exclusion(session_id)?;
        }

        // Invalidate in-memory cache so next get_or_populate re-reads
        SESSIONS.remove(session_id);

        Ok(())
    }

    /// Disable captain-hook for a session.
    pub fn disable(&self, session_id: &str) -> Result<()> {
        self.add_exclusion(session_id)?;

        // Update in-memory cache
        if let Some(mut ctx) = SESSIONS.get_mut(session_id) {
            ctx.disabled = true;
        }

        Ok(())
    }

    /// Re-enable captain-hook for a session.
    pub fn enable(&self, session_id: &str) -> Result<()> {
        self.remove_exclusion(session_id)?;

        // Invalidate in-memory cache so it re-populates
        SESSIONS.remove(session_id);

        Ok(())
    }

    /// Switch a session's role. Clears the session's cache entries.
    pub fn switch_role(&self, session_id: &str, new_role: &str) -> Result<()> {
        // Read existing entry to preserve task/prompt info
        let entries = registration::read_registration_file(&self.registration_file)?;
        let (task, prompt_file) = if let Some(existing) = entries.get(session_id) {
            (existing.task.as_deref(), existing.prompt_path.as_deref())
        } else {
            (None, None)
        };

        // Re-register with new role (owned copies to avoid borrow issues)
        let task_owned = task.map(String::from);
        let prompt_owned = prompt_file.map(String::from);
        self.register(
            session_id,
            new_role,
            task_owned.as_deref(),
            prompt_owned.as_deref(),
        )?;

        Ok(())
    }

    /// Check if a session is registered (either with a role or disabled).
    pub fn is_registered(&self, session_id: &str) -> bool {
        // Check in-memory
        if SESSIONS.contains_key(session_id) {
            return true;
        }

        // Check registration file
        if let Ok(entries) = registration::read_registration_file(&self.registration_file) {
            if entries.contains_key(session_id) {
                return true;
            }
        }

        // Check exclusion file
        if self.is_disabled(session_id) {
            return true;
        }

        // Check env var fallback
        std::env::var("CAPTAIN_HOOK_ROLE").is_ok()
    }

    /// Check if a session is disabled.
    pub fn is_disabled(&self, session_id: &str) -> bool {
        // Check in-memory
        if let Some(ctx) = SESSIONS.get(session_id) {
            if ctx.disabled {
                return true;
            }
        }

        // Check exclusion file
        if let Ok(exclusions) = read_exclusion_file(&self.exclusion_file) {
            return exclusions.contains(&session_id.to_string());
        }

        false
    }

    /// Wait for a session to be registered, polling every 200ms.
    pub async fn wait_for_registration(&self, session_id: &str, timeout_secs: u64) -> Result<()> {
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(timeout_secs);

        loop {
            if self.is_registered(session_id) {
                return Ok(());
            }

            if start.elapsed() >= timeout {
                return Err(CaptainHookError::RegistrationTimeout {
                    session_id: session_id.to_string(),
                    waited_secs: timeout_secs,
                });
            }

            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
    }

    fn add_exclusion(&self, session_id: &str) -> Result<()> {
        let mut exclusions = read_exclusion_file(&self.exclusion_file)?;
        if !exclusions.contains(&session_id.to_string()) {
            exclusions.push(session_id.to_string());
        }
        write_exclusion_file(&self.exclusion_file, &exclusions)
    }

    fn remove_exclusion(&self, session_id: &str) -> Result<()> {
        let mut exclusions = read_exclusion_file(&self.exclusion_file)?;
        exclusions.retain(|s| s != session_id);
        write_exclusion_file(&self.exclusion_file, &exclusions)
    }
}

/// A registration entry from the on-disk sessions file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrationEntry {
    pub role: String,
    pub task: Option<String>,
    pub prompt_hash: Option<String>,
    pub prompt_path: Option<String>,
    pub registered_at: DateTime<Utc>,
    pub registered_by: Option<String>,
}

/// Extract org and project name from git remote origin URL.
fn extract_git_org_project(cwd: &str) -> (String, String) {
    let output = std::process::Command::new("git")
        .arg("remote")
        .arg("get-url")
        .arg("origin")
        .current_dir(cwd)
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let url = String::from_utf8_lossy(&out.stdout).trim().to_string();
            parse_git_remote_url(&url)
        }
        _ => ("unknown".into(), "unknown".into()),
    }
}

/// Parse a git remote URL into (org, project).
fn parse_git_remote_url(url: &str) -> (String, String) {
    // Handle SSH: git@github.com:org/repo.git
    if let Some(path) = url.strip_prefix("git@") {
        if let Some(colon_pos) = path.find(':') {
            let path_part = &path[colon_pos + 1..];
            let path_part = path_part.strip_suffix(".git").unwrap_or(path_part);
            let parts: Vec<&str> = path_part.splitn(2, '/').collect();
            if parts.len() == 2 {
                return (parts[0].to_string(), parts[1].to_string());
            }
        }
    }

    // Handle HTTPS: https://github.com/org/repo.git
    if url.starts_with("https://") || url.starts_with("http://") {
        let path = url
            .split("//")
            .nth(1)
            .and_then(|s| s.split('/').skip(1).collect::<Vec<_>>().join("/").into());

        if let Some(path_str) = path {
            let path_str: &str = &path_str;
            let path_str = path_str.strip_suffix(".git").unwrap_or(path_str);
            let parts: Vec<&str> = path_str.splitn(2, '/').collect();
            if parts.len() == 2 {
                return (parts[0].to_string(), parts[1].to_string());
            }
        }
    }

    ("unknown".into(), "unknown".into())
}

/// Get the current OS username.
fn whoami() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown".into())
}

/// Read exclusion file (JSON array of session IDs).
fn read_exclusion_file(path: &PathBuf) -> Result<Vec<String>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let contents = std::fs::read_to_string(path)?;
    if contents.trim().is_empty() {
        return Ok(Vec::new());
    }
    let exclusions: Vec<String> = serde_json::from_str(&contents)?;
    Ok(exclusions)
}

/// Write exclusion file (JSON array of session IDs) with restrictive permissions.
fn write_exclusion_file(path: &PathBuf, exclusions: &[String]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(exclusions)?;
    std::fs::write(path, &json)?;
    set_file_permissions_0600(path);
    Ok(())
}

/// Get the runtime directory for session state files.
/// Prefers XDG_RUNTIME_DIR (typically /run/user/<uid>/, mode 0700).
/// Falls back to /tmp if not set.
fn runtime_dir() -> PathBuf {
    std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
}

/// Set file permissions to 0600 (owner read/write only).
#[cfg(unix)]
fn set_file_permissions_0600(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let perms = std::fs::Permissions::from_mode(0o600);
    let _ = std::fs::set_permissions(path, perms);
}

#[cfg(not(unix))]
fn set_file_permissions_0600(_path: &std::path::Path) {
    // No-op on non-Unix platforms
}
