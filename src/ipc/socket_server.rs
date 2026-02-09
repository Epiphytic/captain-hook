use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::Notify;

use crate::error::{CaptainHookError, Result};
use crate::ipc::{IpcRequest, IpcResponse};

/// Unix socket server for the supervisor agent.
pub struct IpcServer {
    socket_path: PathBuf,
    shutdown_signal: Arc<Notify>,
}

impl IpcServer {
    pub fn new(socket_path: PathBuf) -> Self {
        Self {
            socket_path,
            shutdown_signal: Arc::new(Notify::new()),
        }
    }

    /// Start listening for connections. Each connection is handled in a spawned task.
    pub async fn serve<F>(&self, handler: F) -> Result<()>
    where
        F: Fn(IpcRequest) -> Pin<Box<dyn Future<Output = Result<IpcResponse>> + Send>>
            + Send
            + Sync
            + 'static,
    {
        // Remove existing socket if present
        if self.socket_path.exists() {
            std::fs::remove_file(&self.socket_path)?;
        }

        // Create parent directory if needed
        if let Some(parent) = self.socket_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let listener =
            UnixListener::bind(&self.socket_path).map_err(|e| CaptainHookError::Ipc {
                reason: format!(
                    "failed to bind socket at {}: {}",
                    self.socket_path.display(),
                    e
                ),
            })?;

        eprintln!(
            "captain-hook: supervisor listening on {}",
            self.socket_path.display()
        );

        let handler = Arc::new(handler);
        let shutdown = self.shutdown_signal.clone();

        loop {
            tokio::select! {
                accept_result = listener.accept() => {
                    match accept_result {
                        Ok((stream, _addr)) => {
                            let handler = handler.clone();
                            tokio::spawn(async move {
                                if let Err(e) = handle_connection(stream, handler).await {
                                    eprintln!("captain-hook: connection error: {}", e);
                                }
                            });
                        }
                        Err(e) => {
                            eprintln!("captain-hook: accept error: {}", e);
                        }
                    }
                }
                _ = shutdown.notified() => {
                    eprintln!("captain-hook: supervisor shutting down");
                    break;
                }
            }
        }

        // Clean up socket file
        let _ = std::fs::remove_file(&self.socket_path);
        Ok(())
    }

    /// Graceful shutdown.
    pub async fn shutdown(&self) -> Result<()> {
        self.shutdown_signal.notify_one();
        Ok(())
    }
}

/// Handle a single client connection.
async fn handle_connection<F>(stream: tokio::net::UnixStream, handler: Arc<F>) -> Result<()>
where
    F: Fn(IpcRequest) -> Pin<Box<dyn Future<Output = Result<IpcResponse>> + Send>>
        + Send
        + Sync
        + 'static,
{
    let (reader, mut writer) = stream.into_split();
    let mut buf_reader = BufReader::new(reader);
    let mut line = String::new();

    // Read request as a JSON line
    buf_reader
        .read_line(&mut line)
        .await
        .map_err(|e| CaptainHookError::Ipc {
            reason: format!("read failed: {}", e),
        })?;

    let request: IpcRequest =
        serde_json::from_str(line.trim()).map_err(|e| CaptainHookError::Ipc {
            reason: format!("invalid request JSON: {}", e),
        })?;

    // Process request
    let response = handler(request).await?;

    // Write response as JSON
    let response_json = serde_json::to_string(&response)?;
    writer
        .write_all(response_json.as_bytes())
        .await
        .map_err(|e| CaptainHookError::Ipc {
            reason: format!("write failed: {}", e),
        })?;
    writer
        .write_all(b"\n")
        .await
        .map_err(|e| CaptainHookError::Ipc {
            reason: format!("write newline failed: {}", e),
        })?;
    writer.shutdown().await.map_err(|e| CaptainHookError::Ipc {
        reason: format!("shutdown failed: {}", e),
    })?;

    Ok(())
}
