use std::io::Read;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::agent::state::{AgentState, AgentStatus};
use crate::ipc;

/// OpenCode hook event types passed via stdin.
#[derive(Debug, Deserialize)]
struct OpenCodeHookInput {
    /// Event type: "file_edited" or "session_completed"
    #[serde(rename = "type")]
    event_type: String,
    /// Session ID
    #[serde(default)]
    session_id: Option<String>,
    /// File path for file_edited events
    #[serde(default)]
    file_path: Option<String>,
    /// Working directory
    #[serde(default)]
    #[allow(dead_code)]
    cwd: Option<String>,
    /// Additional metadata
    #[serde(default)]
    #[allow(dead_code)]
    data: Option<serde_json::Value>,
}

/// Run the OpenCode hook subcommand.
/// Reads stdin JSON, determines event type, sends state via IPC.
pub async fn run() -> Result<()> {
    let pane_id = std::env::var("TMUX_PANE")
        .context("TMUX_PANE environment variable not set (not running inside tmux?)")?;

    let mut stdin_buf = String::new();
    std::io::stdin()
        .read_to_string(&mut stdin_buf)
        .context("Failed to read stdin")?;

    // Log the raw input for debugging (can be removed later)
    eprintln!("[chikuwa opencode-hook] received: {}", stdin_buf.trim());

    let input: OpenCodeHookInput = serde_json::from_str(stdin_buf.trim())
        .context("Failed to parse OpenCode hook input JSON from stdin")?;

    let status = match input.event_type.as_str() {
        "file_edited" => AgentStatus::Running,
        "session_completed" => AgentStatus::Ended,
        _ => {
            eprintln!(
                "[chikuwa opencode-hook] unknown event type: {}",
                input.event_type
            );
            return Ok(());
        }
    };

    let mut state = AgentState::new(pane_id, status);
    state.session_id = input.session_id;
    state.hook_event_name = Some(input.event_type);

    // For file_edited, show the file path as tool detail
    if let Some(path) = input.file_path {
        state.tool_name = Some("edit".to_string());
        state.tool_detail = Some(path);
    }

    ipc::broadcast_state(&state).await?;

    // Persist to JSONL so TUI can restore state on restart
    if let Err(e) = crate::persist::append_agent_state(&state) {
        eprintln!("Warning: failed to persist agent state: {}", e);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opencode_hook_input_file_edited() {
        let json = r#"{"type":"file_edited","file_path":"/src/main.rs","cwd":"/project"}"#;
        let input: OpenCodeHookInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.event_type, "file_edited");
        assert_eq!(input.file_path, Some("/src/main.rs".to_string()));
        assert_eq!(input.cwd, Some("/project".to_string()));
    }

    #[test]
    fn test_opencode_hook_input_session_completed() {
        let json = r#"{"type":"session_completed","session_id":"sess-123"}"#;
        let input: OpenCodeHookInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.event_type, "session_completed");
        assert_eq!(input.session_id, Some("sess-123".to_string()));
    }

    #[test]
    fn test_opencode_hook_input_minimal() {
        let json = r#"{"type":"file_edited"}"#;
        let input: OpenCodeHookInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.event_type, "file_edited");
        assert!(input.file_path.is_none());
    }
}
