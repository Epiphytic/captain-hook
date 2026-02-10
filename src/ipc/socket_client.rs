use std::path::PathBuf;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

use crate::error::{HookwiseError, Result};
use crate::ipc::{IpcRequest, IpcResponse};

/// Unix socket client for worker hooks to connect to the supervisor.
pub struct IpcClient {
    socket_path: PathBuf,
    timeout_secs: u64,
}

impl IpcClient {
    pub fn new(socket_path: PathBuf, timeout_secs: u64) -> Self {
        Self {
            socket_path,
            timeout_secs,
        }
    }

    /// Send a request and wait for a response.
    pub async fn request(&self, req: &IpcRequest) -> Result<IpcResponse> {
        if !self.socket_path.exists() {
            return Err(HookwiseError::SocketNotFound {
                path: self.socket_path.clone(),
            });
        }

        let timeout = std::time::Duration::from_secs(self.timeout_secs);

        let result = tokio::time::timeout(timeout, async {
            let mut stream = UnixStream::connect(&self.socket_path).await.map_err(|e| {
                HookwiseError::Ipc {
                    reason: format!("connect failed: {}", e),
                }
            })?;

            // Send request as JSON line
            let request_json = serde_json::to_string(req)?;
            stream
                .write_all(request_json.as_bytes())
                .await
                .map_err(|e| HookwiseError::Ipc {
                    reason: format!("write failed: {}", e),
                })?;
            stream
                .write_all(b"\n")
                .await
                .map_err(|e| HookwiseError::Ipc {
                    reason: format!("write newline failed: {}", e),
                })?;
            stream.shutdown().await.map_err(|e| HookwiseError::Ipc {
                reason: format!("shutdown write failed: {}", e),
            })?;

            // Read response (bounded to 1MB to prevent OOM)
            let mut response_buf = Vec::new();
            stream
                .take(1_048_576)
                .read_to_end(&mut response_buf)
                .await
                .map_err(|e| HookwiseError::Ipc {
                    reason: format!("read failed: {}", e),
                })?;

            let response: IpcResponse =
                serde_json::from_slice(&response_buf).map_err(|e| HookwiseError::Ipc {
                    reason: format!("invalid response JSON: {}", e),
                })?;

            Ok::<IpcResponse, HookwiseError>(response)
        })
        .await;

        match result {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(HookwiseError::SupervisorTimeout {
                timeout_secs: self.timeout_secs,
            }),
        }
    }
}
