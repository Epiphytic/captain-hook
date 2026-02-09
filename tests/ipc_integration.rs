//! Integration tests for IPC: socket server/client round-trip.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use tempfile::TempDir;

use captain_hook::decision::{Decision, DecisionMetadata, DecisionTier};
use captain_hook::error::Result as CHResult;
use captain_hook::ipc::socket_client::IpcClient;
use captain_hook::ipc::socket_server::IpcServer;
use captain_hook::ipc::{IpcRequest, IpcResponse};

// ---------------------------------------------------------------------------
// IPC message serialization
// ---------------------------------------------------------------------------

#[test]
fn ipc_request_serialization_roundtrip() {
    let request = IpcRequest {
        session_id: "session-123".into(),
        tool_name: "Bash".into(),
        tool_input: r#"{"command": "echo hello"}"#.into(),
        role: "coder".into(),
        file_path: None,
        task_description: Some("build feature X".into()),
        prompt_path: None,
        cwd: "/tmp".into(),
    };

    let json = serde_json::to_string(&request).unwrap();
    let deserialized: IpcRequest = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.session_id, "session-123");
    assert_eq!(deserialized.tool_name, "Bash");
    assert_eq!(deserialized.role, "coder");
    assert_eq!(deserialized.cwd, "/tmp");
    assert_eq!(
        deserialized.task_description.as_deref(),
        Some("build feature X")
    );
}

#[test]
fn ipc_response_serialization_roundtrip() {
    let response = IpcResponse {
        decision: Decision::Allow,
        metadata: DecisionMetadata {
            tier: DecisionTier::Supervisor,
            confidence: 0.95,
            reason: "looks safe".into(),
            matched_key: None,
            similarity_score: None,
        },
    };

    let json = serde_json::to_string(&response).unwrap();
    let deserialized: IpcResponse = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.decision, Decision::Allow);
    assert_eq!(deserialized.metadata.tier, DecisionTier::Supervisor);
    assert!((deserialized.metadata.confidence - 0.95).abs() < f64::EPSILON);
}

#[test]
fn ipc_request_with_all_fields() {
    let request = IpcRequest {
        session_id: "s1".into(),
        tool_name: "Write".into(),
        tool_input: r#"{"file_path": "src/main.rs"}"#.into(),
        role: "coder".into(),
        file_path: Some("src/main.rs".into()),
        task_description: Some("refactor main".into()),
        prompt_path: Some("/tmp/prompt.md".into()),
        cwd: "/home/user/project".into(),
    };

    let json = serde_json::to_string(&request).unwrap();
    assert!(json.contains("src/main.rs"));
    assert!(json.contains("refactor main"));
    assert!(json.contains("/tmp/prompt.md"));
}

// ---------------------------------------------------------------------------
// Socket server/client round-trip
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ipc_server_client_roundtrip() {
    let tmp = TempDir::new().unwrap();
    let socket_path = tmp.path().join("test.sock");

    // Handler that always returns Allow
    let handler =
        |_req: IpcRequest| -> Pin<Box<dyn Future<Output = CHResult<IpcResponse>> + Send>> {
            Box::pin(async move {
                Ok(IpcResponse {
                    decision: Decision::Allow,
                    metadata: DecisionMetadata {
                        tier: DecisionTier::Supervisor,
                        confidence: 0.9,
                        reason: "test approved".into(),
                        matched_key: None,
                        similarity_score: None,
                    },
                })
            })
        };

    // Start server in background
    let server_socket = socket_path.clone();
    let server_handle = tokio::spawn(async move {
        let srv = IpcServer::new(server_socket);
        let _ = srv.serve(handler).await;
    });

    // Wait for server to start
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // Send a request from the client
    let client = IpcClient::new(socket_path.clone(), 5);
    let request = IpcRequest {
        session_id: "test-session".into(),
        tool_name: "Bash".into(),
        tool_input: r#"{"command": "echo hello"}"#.into(),
        role: "coder".into(),
        file_path: None,
        task_description: None,
        prompt_path: None,
        cwd: "/tmp".into(),
    };

    let response = client.request(&request).await.unwrap();
    assert_eq!(response.decision, Decision::Allow);
    assert_eq!(response.metadata.reason, "test approved");

    // Clean up
    server_handle.abort();
    let _ = std::fs::remove_file(&socket_path);
}

#[tokio::test]
async fn ipc_server_handles_deny_response() {
    let tmp = TempDir::new().unwrap();
    let socket_path = tmp.path().join("deny.sock");

    // Handler that always denies
    let handler =
        |_req: IpcRequest| -> Pin<Box<dyn Future<Output = CHResult<IpcResponse>> + Send>> {
            Box::pin(async move {
                Ok(IpcResponse {
                    decision: Decision::Deny,
                    metadata: DecisionMetadata {
                        tier: DecisionTier::Supervisor,
                        confidence: 0.99,
                        reason: "dangerous operation".into(),
                        matched_key: None,
                        similarity_score: None,
                    },
                })
            })
        };

    let server_socket = socket_path.clone();
    let server_handle = tokio::spawn(async move {
        let srv = IpcServer::new(server_socket);
        let _ = srv.serve(handler).await;
    });

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let client = IpcClient::new(socket_path.clone(), 5);
    let request = IpcRequest {
        session_id: "test".into(),
        tool_name: "Bash".into(),
        tool_input: r#"{"command": "rm -rf /"}"#.into(),
        role: "coder".into(),
        file_path: None,
        task_description: None,
        prompt_path: None,
        cwd: "/tmp".into(),
    };

    let response = client.request(&request).await.unwrap();
    assert_eq!(response.decision, Decision::Deny);
    assert_eq!(response.metadata.reason, "dangerous operation");

    server_handle.abort();
    let _ = std::fs::remove_file(&socket_path);
}

#[tokio::test]
async fn ipc_server_handles_ask_response() {
    let tmp = TempDir::new().unwrap();
    let socket_path = tmp.path().join("ask.sock");

    // Handler that returns Ask
    let handler =
        |_req: IpcRequest| -> Pin<Box<dyn Future<Output = CHResult<IpcResponse>> + Send>> {
            Box::pin(async move {
                Ok(IpcResponse {
                    decision: Decision::Ask,
                    metadata: DecisionMetadata {
                        tier: DecisionTier::Supervisor,
                        confidence: 0.5,
                        reason: "needs human review".into(),
                        matched_key: None,
                        similarity_score: None,
                    },
                })
            })
        };

    let server_socket = socket_path.clone();
    let server_handle = tokio::spawn(async move {
        let srv = IpcServer::new(server_socket);
        let _ = srv.serve(handler).await;
    });

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let client = IpcClient::new(socket_path.clone(), 5);
    let request = IpcRequest {
        session_id: "test".into(),
        tool_name: "Write".into(),
        tool_input: r#"{"file_path": ".env"}"#.into(),
        role: "coder".into(),
        file_path: Some(".env".into()),
        task_description: None,
        prompt_path: None,
        cwd: "/tmp".into(),
    };

    let response = client.request(&request).await.unwrap();
    assert_eq!(response.decision, Decision::Ask);

    server_handle.abort();
    let _ = std::fs::remove_file(&socket_path);
}

// ---------------------------------------------------------------------------
// Client error cases
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ipc_client_nonexistent_socket_errors() {
    let client = IpcClient::new(PathBuf::from("/tmp/nonexistent-captain-hook-test.sock"), 1);

    let request = IpcRequest {
        session_id: "test".into(),
        tool_name: "Bash".into(),
        tool_input: "{}".into(),
        role: "coder".into(),
        file_path: None,
        task_description: None,
        prompt_path: None,
        cwd: "/tmp".into(),
    };

    let result = client.request(&request).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn ipc_multiple_sequential_requests() {
    let tmp = TempDir::new().unwrap();
    let socket_path = tmp.path().join("multi.sock");

    // Handler that echoes back the tool name in the reason
    let handler = |req: IpcRequest| -> Pin<Box<dyn Future<Output = CHResult<IpcResponse>> + Send>> {
        Box::pin(async move {
            Ok(IpcResponse {
                decision: Decision::Allow,
                metadata: DecisionMetadata {
                    tier: DecisionTier::Supervisor,
                    confidence: 0.9,
                    reason: format!("approved {}", req.tool_name),
                    matched_key: None,
                    similarity_score: None,
                },
            })
        })
    };

    let server_socket = socket_path.clone();
    let server_handle = tokio::spawn(async move {
        let srv = IpcServer::new(server_socket);
        let _ = srv.serve(handler).await;
    });

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let client = IpcClient::new(socket_path.clone(), 5);

    // Send multiple requests
    for tool in &["Bash", "Write", "Read", "Edit"] {
        let request = IpcRequest {
            session_id: "test".into(),
            tool_name: tool.to_string(),
            tool_input: "{}".into(),
            role: "coder".into(),
            file_path: None,
            task_description: None,
            prompt_path: None,
            cwd: "/tmp".into(),
        };
        let response = client.request(&request).await.unwrap();
        assert_eq!(response.decision, Decision::Allow);
        assert!(response.metadata.reason.contains(tool));
    }

    server_handle.abort();
    let _ = std::fs::remove_file(&socket_path);
}

// ---------------------------------------------------------------------------
// Pending queue serialization
// ---------------------------------------------------------------------------

#[test]
fn pending_queue_serialization_roundtrip() {
    use captain_hook::cascade::human::PendingDecision;
    use captain_hook::ipc::pending_queue;
    use chrono::Utc;

    let decisions = vec![
        PendingDecision {
            id: "id-1".into(),
            session_id: "session-1".into(),
            role: "coder".into(),
            tool_name: "Bash".into(),
            sanitized_input: "echo hello".into(),
            file_path: None,
            recommendation: None,
            is_ask_reprompt: false,
            ask_reason: None,
            queued_at: Utc::now(),
        },
        PendingDecision {
            id: "id-2".into(),
            session_id: "session-2".into(),
            role: "tester".into(),
            tool_name: "Write".into(),
            sanitized_input: "write to file".into(),
            file_path: Some("src/main.rs".into()),
            recommendation: None,
            is_ask_reprompt: true,
            ask_reason: Some("sensitive path".into()),
            queued_at: Utc::now(),
        },
    ];

    let serialized = pending_queue::serialize_pending(&decisions).unwrap();
    let deserialized = pending_queue::deserialize_pending(&serialized).unwrap();

    assert_eq!(deserialized.len(), 2);
    assert_eq!(deserialized[0].id, "id-1");
    assert_eq!(deserialized[1].id, "id-2");
    assert_eq!(deserialized[1].file_path.as_deref(), Some("src/main.rs"));
    assert!(deserialized[1].is_ask_reprompt);
}
