use serde::{Deserialize, Serialize};

use super::state::{ActiveTool, AgentStatus};

/// Full state from OpenCode hooks/plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenCodeState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub status: AgentStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_emoji: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_detail: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub active_tools: Vec<ActiveTool>,
    #[serde(default)]
    pub is_busy: bool,
}

impl OpenCodeState {
    /// Merge an incoming event into existing state, returning the new state.
    pub fn merge(incoming: OpenCodeState, existing: &OpenCodeState) -> OpenCodeState {
        let event = incoming.event_type.as_deref().unwrap_or("");

        // Preserve session_id if incoming is None
        let session_id = incoming
            .session_id
            .clone()
            .or_else(|| existing.session_id.clone());

        // Merge active tools
        let active_tools = match event {
            "tool.execute" | "tool.running" => {
                let mut tools = existing.active_tools.clone();
                for tool in &incoming.active_tools {
                    if !tools.iter().any(|t| t.key == tool.key) {
                        tools.push(tool.clone());
                    }
                }
                tools
            }
            "tool.completed" | "tool.error" => {
                let mut tools = existing.active_tools.clone();
                if let Some(removing) = incoming.active_tools.first() {
                    let pos = tools.iter().position(|t| t.key == removing.key);
                    if let Some(pos) = pos {
                        tools.remove(pos);
                    }
                }
                tools
            }
            "session.idle" | "session.deleted" => Vec::new(),
            _ => existing.active_tools.clone(),
        };

        let mut merged = incoming;
        merged.session_id = session_id;
        merged.active_tools = active_tools;
        merged
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::state::ToolKey;

    fn make_state(event: &str, status: AgentStatus) -> OpenCodeState {
        OpenCodeState {
            session_id: None,
            status,
            event_type: Some(event.to_string()),
            event_emoji: None,
            tool_name: None,
            tool_detail: None,
            active_tools: Vec::new(),
            is_busy: false,
        }
    }

    fn opencode_tool(name: &str, detail: Option<&str>) -> ActiveTool {
        ActiveTool {
            key: ToolKey::OpenCode {
                name: name.to_string(),
                detail: detail.map(String::from),
            },
            name: name.to_string(),
            detail: detail.map(String::from),
            failure_detail: None,
        }
    }

    #[test]
    fn test_tool_execute_adds_tool() {
        let existing = make_state("tool.execute", AgentStatus::Running);
        let mut incoming = make_state("tool.execute", AgentStatus::Running);
        incoming
            .active_tools
            .push(opencode_tool("bash", Some("cargo test")));

        let merged = OpenCodeState::merge(incoming, &existing);
        assert_eq!(merged.active_tools.len(), 1);
        assert_eq!(merged.active_tools[0].name, "bash");
    }

    #[test]
    fn test_tool_execute_no_duplicate() {
        let mut existing = make_state("tool.execute", AgentStatus::Running);
        existing
            .active_tools
            .push(opencode_tool("bash", Some("cargo test")));

        let mut incoming = make_state("tool.execute", AgentStatus::Running);
        incoming
            .active_tools
            .push(opencode_tool("bash", Some("cargo test")));

        let merged = OpenCodeState::merge(incoming, &existing);
        assert_eq!(merged.active_tools.len(), 1);
    }

    #[test]
    fn test_tool_completed_removes_by_key() {
        let mut existing = make_state("tool.execute", AgentStatus::Running);
        existing
            .active_tools
            .push(opencode_tool("bash", Some("cargo test")));
        existing
            .active_tools
            .push(opencode_tool("read", Some("/tmp/file")));

        let mut incoming = make_state("tool.completed", AgentStatus::Running);
        incoming
            .active_tools
            .push(opencode_tool("bash", Some("cargo test")));

        let merged = OpenCodeState::merge(incoming, &existing);
        assert_eq!(merged.active_tools.len(), 1);
        assert_eq!(merged.active_tools[0].name, "read");
    }

    #[test]
    fn test_tool_error_removes_by_key() {
        let mut existing = make_state("tool.execute", AgentStatus::Running);
        existing
            .active_tools
            .push(opencode_tool("bash", Some("cargo test")));

        let mut incoming = make_state("tool.error", AgentStatus::Running);
        incoming
            .active_tools
            .push(opencode_tool("bash", Some("cargo test")));

        let merged = OpenCodeState::merge(incoming, &existing);
        assert!(merged.active_tools.is_empty());
    }

    #[test]
    fn test_session_idle_clears_tools() {
        let mut existing = make_state("tool.execute", AgentStatus::Running);
        existing
            .active_tools
            .push(opencode_tool("bash", Some("cargo test")));

        let incoming = make_state("session.idle", AgentStatus::Waiting);

        let merged = OpenCodeState::merge(incoming, &existing);
        assert!(merged.active_tools.is_empty());
    }

    #[test]
    fn test_session_deleted_clears_tools() {
        let mut existing = make_state("tool.execute", AgentStatus::Running);
        existing.active_tools.push(opencode_tool("bash", None));

        let incoming = make_state("session.deleted", AgentStatus::Ended);

        let merged = OpenCodeState::merge(incoming, &existing);
        assert!(merged.active_tools.is_empty());
    }

    #[test]
    fn test_session_id_preserved_from_existing() {
        let mut existing = make_state("tool.execute", AgentStatus::Running);
        existing.session_id = Some("session-abc".to_string());

        let incoming = make_state("tool.execute", AgentStatus::Running);

        let merged = OpenCodeState::merge(incoming, &existing);
        assert_eq!(merged.session_id, Some("session-abc".to_string()));
    }

    #[test]
    fn test_session_id_incoming_overrides() {
        let mut existing = make_state("tool.execute", AgentStatus::Running);
        existing.session_id = Some("old".to_string());

        let mut incoming = make_state("tool.execute", AgentStatus::Running);
        incoming.session_id = Some("new".to_string());

        let merged = OpenCodeState::merge(incoming, &existing);
        assert_eq!(merged.session_id, Some("new".to_string()));
    }

    #[test]
    fn test_unknown_event_preserves_tools() {
        let mut existing = make_state("tool.execute", AgentStatus::Running);
        existing.active_tools.push(opencode_tool("bash", None));

        let incoming = make_state("permission.asked", AgentStatus::Permission);

        let merged = OpenCodeState::merge(incoming, &existing);
        assert_eq!(merged.active_tools.len(), 1);
    }

    #[test]
    fn test_same_name_different_detail_not_deduped() {
        let mut existing = make_state("tool.execute", AgentStatus::Running);
        existing
            .active_tools
            .push(opencode_tool("bash", Some("cmd1")));

        let mut incoming = make_state("tool.execute", AgentStatus::Running);
        incoming
            .active_tools
            .push(opencode_tool("bash", Some("cmd2")));

        let merged = OpenCodeState::merge(incoming, &existing);
        assert_eq!(merged.active_tools.len(), 2);
    }
}
