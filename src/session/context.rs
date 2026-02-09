use crate::session::SessionContext;

impl SessionContext {
    /// Create a minimal session context for testing/defaults.
    pub fn new_minimal(user: String, org: String, project: String) -> Self {
        Self {
            user,
            org,
            project,
            team: None,
            role: None,
            path_policy: None,
            agent_prompt_hash: None,
            agent_prompt_path: None,
            task_description: None,
            registered_at: None,
            disabled: false,
        }
    }
}
