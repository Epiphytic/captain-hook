pub mod pending_queue;
pub mod socket_client;
pub mod socket_server;

use serde::{Deserialize, Serialize};

use crate::decision::{Decision, DecisionMetadata};

/// IPC request sent from worker hook to supervisor via Unix socket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcRequest {
    pub session_id: String,
    pub tool_name: String,
    pub tool_input: String,
    pub role: String,
    pub file_path: Option<String>,
    pub task_description: Option<String>,
    pub prompt_path: Option<String>,
    pub cwd: String,
}

/// IPC response from supervisor to worker hook.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcResponse {
    pub decision: Decision,
    pub metadata: DecisionMetadata,
}
