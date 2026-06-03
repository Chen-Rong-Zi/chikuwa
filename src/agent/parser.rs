use anyhow::{Context, Result};
use serde::Deserialize;

use super::state::{AgentState, AgentStatus, ToolInfo};

/// Result of parsing a hook event.
pub struct ParseResult {
    pub state: AgentState,
    /// Whether this event should be suppressed (not sent to TUI).
    pub suppress: bool,
}

/// Trait for parsing raw hook input into AgentState.
pub trait HookParser {
    /// Parse a raw JSON string from stdin into a ParseResult.
    fn parse(&self, pane_id: String, raw_json: &str) -> Result<ParseResult>;
}

// ─── Claude Code Hook Parser ───────────────────────────────────────────

/// Input JSON from Claude Code hooks (stdin).
#[derive(Debug, Deserialize)]
struct ClaudeHookInput {
    hook_event_name: String,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(default)]
    tool_name: Option<String>,
    #[serde(default)]
    tool_input: Option<serde_json::Value>,
    #[serde(default)]
    #[allow(dead_code)]
    message: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    file_path: Option<String>,
    #[serde(default)]
    instructions_file: Option<String>,
    #[serde(default)]
    expanded_text: Option<String>,
}

/// Event mapping entry: (status, emoji)
struct EventMapping {
    status: AgentStatus,
    emoji: &'static str,
}

/// Get the event mapping for a Claude Code hook event name.
fn claude_event_mapping(event: &str) -> Option<EventMapping> {
    match event {
        "SessionStart" => Some(EventMapping {
            status: AgentStatus::Started,
            emoji: "🚀",
        }),
        "Setup" => Some(EventMapping {
            status: AgentStatus::Started,
            emoji: "⚙️",
        }),
        "InstructionsLoaded" => Some(EventMapping {
            status: AgentStatus::Running,
            emoji: "📄",
        }),
        "UserPromptSubmit" => Some(EventMapping {
            status: AgentStatus::Running,
            emoji: "💭",
        }),
        "UserPromptExpansion" => Some(EventMapping {
            status: AgentStatus::Running,
            emoji: "⚡",
        }),
        "MessageDisplay" => Some(EventMapping {
            status: AgentStatus::Running,
            emoji: "💬",
        }),
        "PreToolUse" => Some(EventMapping {
            status: AgentStatus::Running,
            emoji: "🔧",
        }),
        "PermissionRequest" => Some(EventMapping {
            status: AgentStatus::Permission,
            emoji: "🔐",
        }),
        "PostToolUse" => Some(EventMapping {
            status: AgentStatus::Running,
            emoji: "✅",
        }),
        "PostToolUseFailure" => Some(EventMapping {
            status: AgentStatus::Running,
            emoji: "❌",
        }),
        "PostToolBatch" => Some(EventMapping {
            status: AgentStatus::Running,
            emoji: "📦",
        }),
        "PermissionDenied" => Some(EventMapping {
            status: AgentStatus::Running,
            emoji: "🚫",
        }),
        "Notification" => None, // handled specially
        "Stop" => Some(EventMapping {
            status: AgentStatus::Waiting,
            emoji: "💤",
        }),
        "StopFailure" => Some(EventMapping {
            status: AgentStatus::Waiting,
            emoji: "⚠️",
        }),
        "SubagentStart" => Some(EventMapping {
            status: AgentStatus::Running,
            emoji: "🤖",
        }),
        "SubagentStop" => Some(EventMapping {
            status: AgentStatus::Ended,
            emoji: "🏁",
        }),
        "TaskCreated" => Some(EventMapping {
            status: AgentStatus::Running,
            emoji: "📋",
        }),
        "TaskCompleted" => Some(EventMapping {
            status: AgentStatus::Running,
            emoji: "✔️",
        }),
        "TeammateIdle" => Some(EventMapping {
            status: AgentStatus::Waiting,
            emoji: "👥",
        }),
        "ConfigChange" => Some(EventMapping {
            status: AgentStatus::Running,
            emoji: "⚙️",
        }),
        "CwdChanged" => Some(EventMapping {
            status: AgentStatus::Running,
            emoji: "📁",
        }),
        "FileChanged" => Some(EventMapping {
            status: AgentStatus::Running,
            emoji: "📝",
        }),
        "WorktreeCreate" => Some(EventMapping {
            status: AgentStatus::Running,
            emoji: "🌳",
        }),
        "WorktreeRemove" => Some(EventMapping {
            status: AgentStatus::Running,
            emoji: "🗑️",
        }),
        "PreCompact" => Some(EventMapping {
            status: AgentStatus::Running,
            emoji: "🗜️",
        }),
        "PostCompact" => Some(EventMapping {
            status: AgentStatus::Running,
            emoji: "📦",
        }),
        "SessionEnd" => Some(EventMapping {
            status: AgentStatus::Ended,
            emoji: "🏁",
        }),
        "Elicitation" => Some(EventMapping {
            status: AgentStatus::Permission,
            emoji: "❓",
        }),
        "ElicitationResult" => Some(EventMapping {
            status: AgentStatus::Running,
            emoji: "✅",
        }),
        _ => None,
    }
}

/// Extract a short detail string from tool_input based on the tool name.
/// For tools with file paths, formats as `file_path:line_number` (nvim-compatible) when a line number is available.
pub fn extract_tool_detail(tool_name: &str, input: &serde_json::Value) -> Option<String> {
    let s = match tool_name {
        "Bash" => input.get("command")?.as_str()?,
        "Read" => {
            let path = input.get("file_path")?.as_str()?;
            if let Some(offset) = input.get("offset").and_then(|v| v.as_u64()) {
                return Some(format!("{path}:{offset}"));
            }
            path
        }
        "Write" | "Edit" => input.get("file_path")?.as_str()?,
        "NotebookEdit" => input.get("notebook_path")?.as_str()?,
        "Grep" => input.get("pattern")?.as_str()?,
        "Glob" => input.get("pattern")?.as_str()?,
        "WebFetch" => input.get("url")?.as_str()?,
        "WebSearch" => input.get("query")?.as_str()?,
        "Task" => {
            if let Some(desc) = input.get("description").and_then(|v| v.as_str()) {
                return Some(desc.to_string());
            }
            return None;
        }
        _ => return None,
    };
    Some(s.to_string())
}

/// Extract event-specific detail from ClaudeHookInput for non-tool events.
fn extract_event_detail(event: &str, input: &ClaudeHookInput) -> Option<String> {
    match event {
        "InstructionsLoaded" => input.instructions_file.clone(),
        "UserPromptExpansion" => input.expanded_text.clone(),
        "CwdChanged" => input.cwd.clone(),
        "FileChanged" => input.file_path.clone(),
        "PermissionRequest" | "PermissionDenied" => input.tool_name.clone(),
        "TaskCreated" | "TaskCompleted" => input
            .tool_input
            .as_ref()
            .and_then(|v| v.get("description"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        "WorktreeCreate" | "WorktreeRemove" => input
            .tool_input
            .as_ref()
            .and_then(|v| v.get("path"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        _ => None,
    }
}

pub struct ClaudeHookParser;

impl HookParser for ClaudeHookParser {
    fn parse(&self, pane_id: String, raw_json: &str) -> Result<ParseResult> {
        let input: ClaudeHookInput = serde_json::from_str(raw_json.trim())
            .context("Failed to parse Claude Code hook input JSON from stdin")?;

        let event_name = input.hook_event_name.clone();

        // Special handling for Notification: check for permission_prompt
        let mapping = if event_name == "Notification" {
            if raw_json.contains("permission_prompt") {
                Some(EventMapping {
                    status: AgentStatus::Permission,
                    emoji: "🔐",
                })
            } else {
                // Non-permission notifications: suppress (don't update TUI state)
                return Ok(ParseResult {
                    state: AgentState::new(pane_id, AgentStatus::Running),
                    suppress: true,
                });
            }
        } else {
            claude_event_mapping(&event_name)
        };

        let mapping = match mapping {
            Some(m) => m,
            None => {
                // Unknown event: log and suppress
                eprintln!("[chikuwa hook] unknown event '{}', ignoring", event_name);
                return Ok(ParseResult {
                    state: AgentState::new(pane_id, AgentStatus::Running),
                    suppress: true,
                });
            }
        };

        let mut state = AgentState::new(pane_id, mapping.status);
        state.session_id = input.session_id.clone();
        state.agent_id = input.agent_id.clone();
        state.hook_event_name = Some(event_name.clone());
        state.event_emoji = Some(mapping.emoji.to_string());

        // Extract tool info for PreToolUse events
        if let Some(ref name) = input.tool_name {
            let detail = input
                .tool_input
                .as_ref()
                .and_then(|inp| extract_tool_detail(name, inp));
            state.tools = vec![ToolInfo {
                name: name.clone(),
                detail,
            }];
        }

        state.tool_name = input.tool_name.clone();
        // For non-tool events, use event-specific detail
        state.tool_detail = extract_event_detail(&event_name, &input).or_else(|| {
            input.tool_name.as_ref().and_then(|name| {
                input
                    .tool_input
                    .as_ref()
                    .and_then(|inp| extract_tool_detail(name, inp))
            })
        });

        Ok(ParseResult {
            state,
            suppress: false,
        })
    }
}

// ─── OpenCode Hook Parser ──────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct OpenCodeHookInput {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    file_path: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    cwd: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    data: Option<serde_json::Value>,
}

pub struct OpenCodeHookParser;

impl HookParser for OpenCodeHookParser {
    fn parse(&self, pane_id: String, raw_json: &str) -> Result<ParseResult> {
        let input: OpenCodeHookInput = serde_json::from_str(raw_json.trim())
            .context("Failed to parse OpenCode hook input JSON from stdin")?;

        let (status, emoji) = match input.event_type.as_str() {
            "file_edited" => (AgentStatus::Running, "📝"),
            "session_completed" => (AgentStatus::Ended, "🏁"),
            _ => {
                eprintln!(
                    "[chikuwa opencode-hook] unknown event type: {}",
                    input.event_type
                );
                return Ok(ParseResult {
                    state: AgentState::new(pane_id, AgentStatus::Running),
                    suppress: true,
                });
            }
        };

        let mut state = AgentState::new(pane_id, status);
        state.session_id = input.session_id;
        state.hook_event_name = Some(input.event_type);
        state.event_emoji = Some(emoji.to_string());

        if let Some(path) = input.file_path {
            state.tool_name = Some("edit".to_string());
            state.tool_detail = Some(path);
        }

        Ok(ParseResult {
            state,
            suppress: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_claude_hook_input_deserialize() {
        let json = r#"{"hook_event_name":"SessionStart","session_id":"abc123"}"#;
        let parser = ClaudeHookParser;
        let result = parser.parse("%0".to_string(), json).unwrap();
        assert!(!result.suppress);
        assert_eq!(result.state.state, AgentStatus::Started);
        assert_eq!(result.state.event_emoji.as_deref(), Some("🚀"));
        assert_eq!(result.state.session_id.as_deref(), Some("abc123"));
    }

    #[test]
    fn test_claude_hook_session_end() {
        let json = r#"{"hook_event_name":"SessionEnd"}"#;
        let parser = ClaudeHookParser;
        let result = parser.parse("%0".to_string(), json).unwrap();
        assert!(!result.suppress);
        assert_eq!(result.state.state, AgentStatus::Ended);
        assert_eq!(result.state.event_emoji.as_deref(), Some("🏁"));
    }

    #[test]
    fn test_claude_hook_pre_tool_use() {
        let json = r#"{"hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{"command":"ls -la"}}"#;
        let parser = ClaudeHookParser;
        let result = parser.parse("%0".to_string(), json).unwrap();
        assert!(!result.suppress);
        assert_eq!(result.state.state, AgentStatus::Running);
        assert_eq!(result.state.event_emoji.as_deref(), Some("🔧"));
        assert_eq!(result.state.tools.len(), 1);
        assert_eq!(result.state.tools[0].name, "Bash");
        assert_eq!(result.state.tools[0].detail.as_deref(), Some("ls -la"));
    }

    #[test]
    fn test_claude_hook_notification_permission() {
        let json = r#"{"hook_event_name":"Notification","message":"permission_prompt foo"}"#;
        let parser = ClaudeHookParser;
        let result = parser.parse("%0".to_string(), json).unwrap();
        assert!(!result.suppress);
        assert_eq!(result.state.state, AgentStatus::Permission);
        assert_eq!(result.state.event_emoji.as_deref(), Some("🔐"));
    }

    #[test]
    fn test_claude_hook_notification_non_permission_suppressed() {
        let json = r#"{"hook_event_name":"Notification","message":"some info"}"#;
        let parser = ClaudeHookParser;
        let result = parser.parse("%0".to_string(), json).unwrap();
        assert!(result.suppress);
    }

    #[test]
    fn test_claude_hook_unknown_event_suppressed() {
        let json = r#"{"hook_event_name":"FutureEvent"}"#;
        let parser = ClaudeHookParser;
        let result = parser.parse("%0".to_string(), json).unwrap();
        assert!(result.suppress);
    }

    #[test]
    fn test_claude_hook_stop() {
        let json = r#"{"hook_event_name":"Stop"}"#;
        let parser = ClaudeHookParser;
        let result = parser.parse("%0".to_string(), json).unwrap();
        assert_eq!(result.state.state, AgentStatus::Waiting);
        assert_eq!(result.state.event_emoji.as_deref(), Some("💤"));
    }

    #[test]
    fn test_claude_hook_stop_failure() {
        let json = r#"{"hook_event_name":"StopFailure"}"#;
        let parser = ClaudeHookParser;
        let result = parser.parse("%0".to_string(), json).unwrap();
        assert_eq!(result.state.state, AgentStatus::Waiting);
        assert_eq!(result.state.event_emoji.as_deref(), Some("⚠️"));
    }

    #[test]
    fn test_claude_hook_all_events_have_mapping() {
        let events = [
            "SessionStart",
            "Setup",
            "InstructionsLoaded",
            "UserPromptSubmit",
            "UserPromptExpansion",
            "MessageDisplay",
            "PreToolUse",
            "PermissionRequest",
            "PostToolUse",
            "PostToolUseFailure",
            "PostToolBatch",
            "PermissionDenied",
            "Stop",
            "StopFailure",
            "SubagentStart",
            "SubagentStop",
            "TaskCreated",
            "TaskCompleted",
            "TeammateIdle",
            "ConfigChange",
            "CwdChanged",
            "FileChanged",
            "WorktreeCreate",
            "WorktreeRemove",
            "PreCompact",
            "PostCompact",
            "SessionEnd",
            "Elicitation",
            "ElicitationResult",
        ];
        for event in &events {
            assert!(
                claude_event_mapping(event).is_some(),
                "Missing mapping for event: {}",
                event
            );
        }
    }

    #[test]
    fn test_claude_hook_subagent_start() {
        let json = r#"{"hook_event_name":"SubagentStart","agent_id":"abc123","tool_name":"Task","tool_input":{"description":"Search codebase"}}"#;
        let parser = ClaudeHookParser;
        let result = parser.parse("%0".to_string(), json).unwrap();
        assert_eq!(result.state.state, AgentStatus::Running);
        assert_eq!(result.state.event_emoji.as_deref(), Some("🤖"));
        assert_eq!(result.state.agent_id.as_deref(), Some("abc123"));
    }

    #[test]
    fn test_claude_hook_subagent_stop() {
        let json = r#"{"hook_event_name":"SubagentStop","agent_id":"abc123"}"#;
        let parser = ClaudeHookParser;
        let result = parser.parse("%0".to_string(), json).unwrap();
        assert_eq!(result.state.state, AgentStatus::Ended);
        assert_eq!(result.state.event_emoji.as_deref(), Some("🏁"));
    }

    #[test]
    fn test_claude_hook_cwd_changed() {
        let json = r#"{"hook_event_name":"CwdChanged","cwd":"/home/user/project"}"#;
        let parser = ClaudeHookParser;
        let result = parser.parse("%0".to_string(), json).unwrap();
        assert_eq!(result.state.state, AgentStatus::Running);
        assert_eq!(
            result.state.tool_detail.as_deref(),
            Some("/home/user/project")
        );
    }

    #[test]
    fn test_opencode_hook_file_edited() {
        let json = r#"{"type":"file_edited","file_path":"/src/main.rs","cwd":"/project"}"#;
        let parser = OpenCodeHookParser;
        let result = parser.parse("%0".to_string(), json).unwrap();
        assert!(!result.suppress);
        assert_eq!(result.state.state, AgentStatus::Running);
        assert_eq!(result.state.event_emoji.as_deref(), Some("📝"));
    }

    #[test]
    fn test_opencode_hook_session_completed() {
        let json = r#"{"type":"session_completed","session_id":"sess-123"}"#;
        let parser = OpenCodeHookParser;
        let result = parser.parse("%0".to_string(), json).unwrap();
        assert_eq!(result.state.state, AgentStatus::Ended);
        assert_eq!(result.state.event_emoji.as_deref(), Some("🏁"));
    }

    #[test]
    fn test_extract_tool_detail_read_with_offset() {
        let input = serde_json::json!({"file_path": "/src/main.rs", "offset": 42});
        assert_eq!(
            extract_tool_detail("Read", &input),
            Some("/src/main.rs:42".to_string())
        );
    }

    #[test]
    fn test_extract_tool_detail_bash() {
        let input = serde_json::json!({"command": "ls -la"});
        assert_eq!(
            extract_tool_detail("Bash", &input),
            Some("ls -la".to_string())
        );
    }

    #[test]
    fn test_extract_tool_detail_unknown() {
        let input = serde_json::json!({"foo": "bar"});
        assert_eq!(extract_tool_detail("UnknownTool", &input), None);
    }
}
