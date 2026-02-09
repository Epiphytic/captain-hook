use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum CaptainHookError {
    #[error("session not registered: {session_id}")]
    SessionNotRegistered { session_id: String },

    #[error("session disabled: {session_id}")]
    SessionDisabled { session_id: String },

    #[error("role not found: {role_name}")]
    RoleNotFound { role_name: String },

    #[error("policy file not found: {path}")]
    PolicyNotFound { path: PathBuf },

    #[error("invalid policy: {reason}")]
    InvalidPolicy { reason: String },

    #[error("config parse error in {path}: {reason}")]
    ConfigParse { path: PathBuf, reason: String },

    #[error("storage error: {reason}")]
    Storage { reason: String },

    #[error("index build error: {reason}")]
    IndexBuild { reason: String },

    #[error("embedding error: {reason}")]
    Embedding { reason: String },

    #[error("supervisor error: {reason}")]
    Supervisor { reason: String },

    #[error("supervisor timeout after {timeout_secs}s")]
    SupervisorTimeout { timeout_secs: u64 },

    #[error("human decision timeout after {timeout_secs}s")]
    HumanTimeout { timeout_secs: u64 },

    #[error("ipc error: {reason}")]
    Ipc { reason: String },

    #[error("socket not found at {path}")]
    SocketNotFound { path: PathBuf },

    #[error("registration timeout: waited {waited_secs}s for session {session_id}")]
    RegistrationTimeout {
        session_id: String,
        waited_secs: u64,
    },

    #[error("glob pattern error: {pattern}: {reason}")]
    GlobPattern { pattern: String, reason: String },

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("api error: status={status}, body={body}")]
    Api { status: u16, body: String },
}

pub type Result<T> = std::result::Result<T, CaptainHookError>;
