use std::path::{Path, PathBuf};

use anyhow::Result;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc;

use crate::agent::state::AgentState;
use crate::event::AppEvent;

pub fn socket_dir() -> PathBuf {
    if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        PathBuf::from(runtime_dir).join("chikuwa")
    } else {
        PathBuf::from("/tmp/chikuwa")
    }
}

pub fn instance_socket_path(pid: u32) -> PathBuf {
    socket_dir().join(format!("{}.sock", pid))
}

pub async fn broadcast_state(state: &AgentState) -> Result<()> {
    let json = serde_json::to_string(state)?;
    let mut buf = json;
    buf.push('\n');
    let dir = socket_dir();
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("sock") {
            continue;
        }
        if let Ok(mut stream) = UnixStream::connect(&path).await {
            let _ = stream.write_all(buf.as_bytes()).await;
            let _ = stream.shutdown().await;
        }
    }
    Ok(())
}

pub async fn broadcast_notify() -> Result<()> {
    let dir = socket_dir();
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("sock") {
            continue;
        }
        if let Ok(mut stream) = UnixStream::connect(&path).await {
            let _ = stream.write_all(b"notify\n").await;
            let _ = stream.shutdown().await;
        }
    }
    Ok(())
}

pub async fn start_listener(path: &Path, tx: mpsc::Sender<AppEvent>) -> Result<()> {
    let listener = UnixListener::bind(path)?;
    // Semaphore to limit concurrent IPC handlers (prevent resource exhaustion)
    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(16));
    loop {
        let (stream, _) = match listener.accept().await {
            Ok(conn) => conn,
            Err(_) => continue,
        };
        let tx = tx.clone();
        let permit = match semaphore.clone().try_acquire_owned() {
            Ok(p) => p,
            Err(_) => {
                // Too many concurrent handlers, skip this connection
                // This prevents task spawn explosion under heavy load
                continue;
            }
        };
        tokio::spawn(async move {
            let reader = BufReader::new(stream);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let line = line.trim().to_string();
                if line.is_empty() {
                    continue;
                }
                if line == "notify" {
                    // Use try_send to avoid blocking if channel is full
                    let _ = tx.try_send(AppEvent::TmuxChanged);
                } else if let Ok(state) = serde_json::from_str::<AgentState>(&line) {
                    let _ = tx.try_send(AppEvent::AgentStateUpdate(state));
                }
            }
            drop(permit);
        });
    }
}

pub fn cleanup_instance_socket(pid: u32) {
    let path = instance_socket_path(pid);
    if path.exists() {
        std::fs::remove_file(&path).ok();
    }
    let dir = socket_dir();
    if let Ok(mut entries) = std::fs::read_dir(&dir) {
        if entries.next().is_none() {
            std::fs::remove_dir(&dir).ok();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_socket_dir_with_xdg() {
        std::env::set_var("XDG_RUNTIME_DIR", "/run/user/1000");
        let dir = socket_dir();
        assert_eq!(dir, PathBuf::from("/run/user/1000/chikuwa"));
        std::env::remove_var("XDG_RUNTIME_DIR");
    }

    #[test]
    fn test_socket_dir_fallback() {
        std::env::remove_var("XDG_RUNTIME_DIR");
        let dir = socket_dir();
        assert_eq!(dir, PathBuf::from("/tmp/chikuwa"));
    }

    #[test]
    fn test_instance_socket_path() {
        std::env::set_var("XDG_RUNTIME_DIR", "/run/user/1000");
        let path = instance_socket_path(12345);
        assert_eq!(path, PathBuf::from("/run/user/1000/chikuwa/12345.sock"));
        std::env::remove_var("XDG_RUNTIME_DIR");
    }
}
