use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::agent::state::{AgentState, AgentStatus};
use crate::agent::{SubagentInfo, SubagentStatus};
use crate::tmux::types::TmuxSession;
use crate::ui::theme;

/// An agent entry for the office view (pre-computed from session data).
#[allow(dead_code)]
struct AgentEntry {
    pane_id: String,
    tmux_target: String,
    agent_state: AgentState,
    subagents: Vec<SubagentInfo>,
    completed_count: u32,
}

/// Collect all agent entries from sessions (only sessions with agents).
fn collect_agent_entries(
    sessions: &[TmuxSession],
    subagent_data: &std::collections::HashMap<String, (Vec<SubagentInfo>, u32)>,
) -> Vec<AgentEntry> {
    let mut entries = Vec::new();

    for session in sessions {
        for window in &session.windows {
            for pane in &window.panes {
                if let Some(ref state) = pane.agent_state {
                    let (subagents, completed) = subagent_data
                        .get(&pane.pane_id)
                        .cloned()
                        .unwrap_or_default();
                    entries.push(AgentEntry {
                        pane_id: pane.pane_id.clone(),
                        tmux_target: format!(
                            "{}:{}.{}",
                            session.session_name, window.window_index, pane.pane_index
                        ),
                        agent_state: state.clone(),
                        subagents,
                        completed_count: completed,
                    });
                }
            }
        }
    }

    entries
}

/// Format elapsed seconds as human-readable string.
fn format_duration(secs: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let elapsed = now.saturating_sub(secs);
    if elapsed < 60 {
        format!("{}s", elapsed)
    } else if elapsed < 3600 {
        format!("{}m {}s", elapsed / 60, elapsed % 60)
    } else {
        format!("{}h {}m", elapsed / 3600, (elapsed % 3600) / 60)
    }
}

/// Render the office view.
pub fn render(
    f: &mut Frame,
    area: Rect,
    sessions: &[TmuxSession],
    subagent_data: &std::collections::HashMap<String, (Vec<SubagentInfo>, u32)>,
    selected: usize,
    scroll_offset: usize,
    anim_frame: usize,
) {
    let entries = collect_agent_entries(sessions, subagent_data);

    let lines = build_office_lines(&entries, area.width, selected, anim_frame);

    let visible_height = area.height as usize;
    let visible_lines: Vec<Line> = lines
        .into_iter()
        .skip(scroll_offset)
        .take(visible_height)
        .collect();

    let paragraph = Paragraph::new(visible_lines);
    f.render_widget(paragraph, area);
}

/// Get the number of agent entries (selectable items).
pub fn agent_count(
    sessions: &[TmuxSession],
    subagent_data: &std::collections::HashMap<String, (Vec<SubagentInfo>, u32)>,
) -> usize {
    collect_agent_entries(sessions, subagent_data).len()
}

/// Compute the line range that the selected agent entry occupies,
/// for scroll adjustment before rendering. Returns (start_line, end_line, total_lines).
pub fn selected_line_range(
    sessions: &[TmuxSession],
    subagent_data: &std::collections::HashMap<String, (Vec<SubagentInfo>, u32)>,
    width: u16,
    selected: usize,
    anim_frame: usize,
) -> (usize, usize, usize) {
    let entries = collect_agent_entries(sessions, subagent_data);
    let lines = build_office_lines(&entries, width, selected, anim_frame);
    let total = lines.len();

    // Find the line range for the selected entry by scanning for its room borders.
    // Each room starts with ┌ and ends with ┘. We track which entry index we're on.
    let mut entry_idx = 0;
    let mut room_start = 0;
    let mut room_end = 0;
    let mut found = false;

    for (line_idx, line) in lines.iter().enumerate() {
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        if text.contains('┌') {
            room_start = line_idx;
            if entry_idx == selected {
                found = true;
            }
        }
        if text.contains('┘') {
            if found {
                room_end = line_idx;
                break;
            }
            entry_idx += 1;
        }
    }

    if found {
        (room_start, room_end, total)
    } else {
        (0, 0, total)
    }
}

/// Get the tmux target for the selected agent entry.
pub fn selected_tmux_target(
    sessions: &[TmuxSession],
    subagent_data: &std::collections::HashMap<String, (Vec<SubagentInfo>, u32)>,
    selected: usize,
) -> Option<String> {
    let entries = collect_agent_entries(sessions, subagent_data);
    entries.get(selected).map(|e| e.tmux_target.clone())
}

fn build_office_lines(
    entries: &[AgentEntry],
    width: u16,
    selected: usize,
    anim_frame: usize,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if entries.is_empty() {
        let msg = "  No active agents";
        lines.push(Line::from(Span::styled(
            msg.to_string(),
            Style::default().fg(theme::COLOR_DIM),
        )));
        return lines;
    }

    let content_width = (width as usize).saturating_sub(2); // padding

    for (idx, entry) in entries.iter().enumerate() {
        let is_selected = idx == selected;
        let is_permission = entry.agent_state.state == AgentStatus::Permission;

        // Main agent room
        let room_lines = render_agent_room(
            &entry.agent_state,
            &entry.subagents,
            entry.completed_count,
            content_width,
            is_selected,
            is_permission,
            anim_frame,
        );
        lines.extend(room_lines);

        // Separator between entries
        if idx < entries.len() - 1 {
            lines.push(Line::from(Span::styled(
                "  ···".to_string(),
                Style::default().fg(theme::COLOR_PURPLE),
            )));
        }
    }

    // Status summary at bottom
    lines.push(Line::from(""));
    let mut running = 0u32;
    let mut waiting = 0u32;
    let mut permission = 0u32;
    let mut ended = 0u32;
    for entry in entries {
        match entry.agent_state.state {
            AgentStatus::Started | AgentStatus::Running => running += 1,
            AgentStatus::Waiting => waiting += 1,
            AgentStatus::Permission => permission += 1,
            AgentStatus::Ended => ended += 1,
        }
    }
    let mut summary_parts = Vec::new();
    if running > 0 {
        summary_parts.push(format!("🔧 {} running", running));
    }
    if waiting > 0 {
        summary_parts.push(format!("💤 {} waiting", waiting));
    }
    if permission > 0 {
        summary_parts.push(format!("🔐 {} needs you!", permission));
    }
    if ended > 0 {
        summary_parts.push(format!("🏁 {} ended", ended));
    }
    if !summary_parts.is_empty() {
        let summary = format!("  {}", summary_parts.join("   "));
        lines.push(Line::from(Span::styled(
            summary,
            Style::default().fg(theme::COLOR_LIGHT_PURPLE),
        )));
    }

    lines
}

fn render_agent_room(
    agent: &AgentState,
    subagents: &[SubagentInfo],
    completed_count: u32,
    content_width: usize,
    is_selected: bool,
    is_permission: bool,
    anim_frame: usize,
) -> Vec<Line<'static>> {
    let dim_style = Style::default().fg(theme::COLOR_DIM);
    let name_style = if is_permission {
        Style::default()
            .fg(theme::COLOR_WHITE)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::COLOR_LIGHT_PURPLE)
    };

    let bg_color = if is_permission {
        Some(theme::COLOR_PERMISSION_BG)
    } else {
        None
    };

    let status_label = match agent.state {
        AgentStatus::Started => "starting",
        AgentStatus::Running => "running",
        AgentStatus::Waiting => "waiting",
        AgentStatus::Permission => "needs input!",
        AgentStatus::Ended => "ended",
    };

    let status_emoji = agent.event_emoji.as_deref().unwrap_or(match agent.state {
        AgentStatus::Started => "🚀",
        AgentStatus::Running => "🔧",
        AgentStatus::Waiting => "💤",
        AgentStatus::Permission => "🔐",
        AgentStatus::Ended => "🏁",
    });

    let border_style = if is_permission {
        Style::default().fg(theme::COLOR_WHITE)
    } else {
        Style::default().fg(theme::COLOR_PURPLE)
    };

    let mut lines = Vec::new();

    // Room top border: ┌── 🤖 Agent Name ──── Status ──┐
    let name_text = format!("🤖 {}", agent.hook_event_name.as_deref().unwrap_or("Agent"));
    let status_text = format!("{} {}", status_emoji, status_label);
    let name_width = name_text.width();
    let status_width = status_text.width();
    let inner_width = content_width.saturating_sub(2); // ┌ and ┐
    let fill_len = inner_width.saturating_sub(name_width + status_width + 4); // 2 spaces + 2 ──
    let fill = "─".repeat(fill_len);

    let mut border_spans = vec![
        Span::styled("┌─ ".to_string(), border_style),
        Span::styled(name_text, name_style),
        Span::styled(format!(" ── {} ", fill), border_style),
        Span::styled(status_text, dim_style),
        Span::styled(" ─┐".to_string(), border_style),
    ];
    apply_bg(&mut border_spans, bg_color, is_selected);
    lines.push(Line::from(border_spans));

    // Room content: activity description + tools
    let icon = theme::status_icon(&agent.state, anim_frame);

    // Activity line
    let activity = if let Some(ref tool_name) = agent.tool_name {
        if let Some(ref detail) = agent.tool_detail {
            format!("{} {}: {}", theme::ICON_TOOL, tool_name, detail)
        } else {
            format!("{} {}", theme::ICON_TOOL, tool_name)
        }
    } else if let Some(ref emoji) = agent.event_emoji {
        emoji.to_string()
    } else {
        "idle".to_string()
    };

    let mut activity_spans = vec![
        Span::styled("│ ".to_string(), border_style),
        Span::styled(
            format!(" {} ", icon),
            theme::status_style(&agent.state, true),
        ),
        Span::styled(format!(" {}", activity), dim_style),
    ];
    let used: usize = activity_spans.iter().map(|s| s.content.width()).sum();
    let pad = content_width.saturating_sub(used + 1); // +1 for │
    activity_spans.push(Span::styled(
        " ".repeat(pad),
        Style::default().fg(Color::Reset),
    ));
    activity_spans.push(Span::styled("│".to_string(), border_style));
    apply_bg(&mut activity_spans, bg_color, is_selected);
    lines.push(Line::from(activity_spans));

    // Tool lines (up to 3)
    let visible_tools: Vec<_> = if agent.tools.len() > 3 {
        agent.tools[agent.tools.len() - 3..].iter().collect()
    } else {
        agent.tools.iter().collect()
    };
    for tool in &visible_tools {
        let tool_text = match &tool.detail {
            Some(detail) => format!("  {}: {}", tool.name, detail),
            None => format!("  {}", tool.name),
        };
        let mut tool_spans = vec![
            Span::styled("│ ".to_string(), border_style),
            Span::styled(format!(" 🔧{}", tool_text), dim_style),
        ];
        let used: usize = tool_spans.iter().map(|s| s.content.width()).sum();
        let pad = content_width.saturating_sub(used + 1);
        tool_spans.push(Span::styled(
            " ".repeat(pad),
            Style::default().fg(Color::Reset),
        ));
        tool_spans.push(Span::styled("│".to_string(), border_style));
        apply_bg(&mut tool_spans, bg_color, is_selected);
        lines.push(Line::from(tool_spans));
    }

    // Duration + summary line
    let duration = format_duration(agent.updated_at);
    let tool_count = if agent.tools.is_empty() {
        String::new()
    } else {
        format!("  📋 {} tools", agent.tools.len())
    };
    let sub_count = if !subagents.is_empty() {
        format!("  🤖 {} sub", subagents.len())
    } else if completed_count > 0 {
        format!("  ✅ {} completed", completed_count)
    } else {
        String::new()
    };
    let info = format!("⏱️ {}{}{}", duration, tool_count, sub_count);

    let mut info_spans = vec![
        Span::styled("│ ".to_string(), border_style),
        Span::styled(format!(" {}", info), dim_style),
    ];
    let used: usize = info_spans.iter().map(|s| s.content.width()).sum();
    let pad = content_width.saturating_sub(used + 1);
    info_spans.push(Span::styled(
        " ".repeat(pad),
        Style::default().fg(Color::Reset),
    ));
    info_spans.push(Span::styled("│".to_string(), border_style));
    apply_bg(&mut info_spans, bg_color, is_selected);
    lines.push(Line::from(info_spans));

    // Subagent rooms (compact, inline)
    for sub in subagents {
        let sub_emoji = match sub.state {
            SubagentStatus::Running => "🔧",
            SubagentStatus::Waiting => "💤",
            SubagentStatus::Ended => "✅",
        };
        let sub_desc = sub.description.as_deref().unwrap_or(&sub.short_id);
        let sub_tool = if let Some(tool) = sub.tools.first() {
            match &tool.detail {
                Some(detail) => format!(": {} {}", tool.name, detail),
                None => format!(": {}", tool.name),
            }
        } else {
            String::new()
        };
        let sub_line = format!(
            "  ├─ 🤖 {} {} {}{}",
            sub.short_id, sub_emoji, sub_desc, sub_tool
        );
        let mut sub_spans = vec![
            Span::styled("│ ".to_string(), border_style),
            Span::styled(sub_line, dim_style),
        ];
        let used: usize = sub_spans.iter().map(|s| s.content.width()).sum();
        let pad = content_width.saturating_sub(used + 1);
        sub_spans.push(Span::styled(
            " ".repeat(pad),
            Style::default().fg(Color::Reset),
        ));
        sub_spans.push(Span::styled("│".to_string(), border_style));
        apply_bg(&mut sub_spans, None, is_selected);
        lines.push(Line::from(sub_spans));
    }

    // Completed subagents count
    if completed_count > 0 && subagents.is_empty() {
        let comp_line = format!("  └─ ✅ {} completed", completed_count);
        let mut comp_spans = vec![
            Span::styled("│ ".to_string(), border_style),
            Span::styled(comp_line, Style::default().fg(theme::COLOR_DIM_DARK)),
        ];
        let used: usize = comp_spans.iter().map(|s| s.content.width()).sum();
        let pad = content_width.saturating_sub(used + 1);
        comp_spans.push(Span::styled(
            " ".repeat(pad),
            Style::default().fg(Color::Reset),
        ));
        comp_spans.push(Span::styled("│".to_string(), border_style));
        apply_bg(&mut comp_spans, None, is_selected);
        lines.push(Line::from(comp_spans));
    }

    // Room bottom border
    let bottom_fill = "─".repeat(content_width.saturating_sub(2));
    let mut bottom_spans = vec![
        Span::styled("└".to_string(), border_style),
        Span::styled(bottom_fill, border_style),
        Span::styled("┘".to_string(), border_style),
    ];
    apply_bg(&mut bottom_spans, bg_color, is_selected);
    lines.push(Line::from(bottom_spans));

    lines
}

/// Apply background color (permission highlight or selection) to all spans in a line.
fn apply_bg(spans: &mut Vec<Span<'static>>, permission_bg: Option<Color>, is_selected: bool) {
    if is_selected {
        for span in spans {
            span.style = span.style.bg(theme::COLOR_SELECTED_BG);
        }
    } else if let Some(bg) = permission_bg {
        for span in spans {
            span.style = span.style.bg(bg);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::state::{AgentStatus, ToolInfo};
    use crate::tmux::types::{TmuxPane, TmuxSession, TmuxWindow};
    use std::collections::HashMap;

    fn make_agent_state(pane_id: &str, status: AgentStatus) -> AgentState {
        AgentState {
            tmux_pane: pane_id.to_string(),
            session_id: None,
            agent_id: None,
            state: status,
            updated_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            hook_event_name: Some("PreToolUse".to_string()),
            event_emoji: Some("🔧".to_string()),
            tool_name: Some("Read".to_string()),
            tool_detail: Some("src/main.rs".to_string()),
            failure_detail: None,
            tools: vec![ToolInfo {
                name: "Read".to_string(),
                detail: Some("src/main.rs".to_string()),
            }],
        }
    }

    fn make_session(panes: Vec<TmuxPane>) -> TmuxSession {
        TmuxSession {
            session_name: "main".to_string(),
            session_attached: true,
            repo_name: None,
            toplevel: None,
            worktree_name: None,
            windows: vec![TmuxWindow {
                window_index: 0,
                window_name: "claude".to_string(),
                window_active: true,
                panes,
            }],
        }
    }

    fn make_pane(pane_id: &str, agent_state: Option<AgentState>) -> TmuxPane {
        TmuxPane {
            pane_id: pane_id.to_string(),
            pane_index: 0,
            pane_current_command: "claude".to_string(),
            pane_current_path: "/home".to_string(),
            pane_title: String::new(),
            pane_active: true,
            agent_state,
            git_info: None,
        }
    }

    #[test]
    fn test_collect_agent_entries_empty() {
        let entries = collect_agent_entries(&[], &HashMap::new());
        assert!(entries.is_empty());
    }

    #[test]
    fn test_collect_agent_entries_with_agent() {
        let state = make_agent_state("%0", AgentStatus::Running);
        let sessions = vec![make_session(vec![make_pane("%0", Some(state))])];
        let entries = collect_agent_entries(&sessions, &HashMap::new());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].agent_state.state, AgentStatus::Running);
        assert_eq!(entries[0].tmux_target, "main:0.0");
    }

    #[test]
    fn test_collect_agent_entries_skips_no_agent() {
        let sessions = vec![make_session(vec![make_pane("%0", None)])];
        let entries = collect_agent_entries(&sessions, &HashMap::new());
        assert!(entries.is_empty());
    }

    #[test]
    fn test_format_duration() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert_eq!(format_duration(now), "0s");
        assert_eq!(format_duration(now - 30), "30s");
        assert_eq!(format_duration(now - 90), "1m 30s");
    }

    #[test]
    fn test_build_office_lines_empty() {
        let lines = build_office_lines(&[], 80, 0, 0);
        assert!(!lines.is_empty()); // Should show "No active agents"
    }

    #[test]
    fn test_build_office_lines_with_entry() {
        let entries = vec![AgentEntry {
            pane_id: "%0".to_string(),
            tmux_target: "main:0.0".to_string(),
            agent_state: make_agent_state("%0", AgentStatus::Running),
            subagents: Vec::new(),
            completed_count: 0,
        }];
        let lines = build_office_lines(&entries, 80, 0, 0);
        assert!(lines.len() > 4); // Border + content + tools + duration + border
    }

    #[test]
    fn test_agent_count() {
        let state = make_agent_state("%0", AgentStatus::Running);
        let sessions = vec![make_session(vec![make_pane("%0", Some(state))])];
        assert_eq!(agent_count(&sessions, &HashMap::new()), 1);
    }

    #[test]
    fn test_selected_tmux_target() {
        let state = make_agent_state("%0", AgentStatus::Running);
        let sessions = vec![make_session(vec![make_pane("%0", Some(state))])];
        assert_eq!(
            selected_tmux_target(&sessions, &HashMap::new(), 0),
            Some("main:0.0".to_string())
        );
        assert_eq!(selected_tmux_target(&sessions, &HashMap::new(), 1), None);
    }

    #[test]
    fn test_build_office_lines_permission_highlight() {
        let mut state = make_agent_state("%0", AgentStatus::Permission);
        state.event_emoji = Some("🔐".to_string());
        let entries = vec![AgentEntry {
            pane_id: "%0".to_string(),
            tmux_target: "main:0.0".to_string(),
            agent_state: state,
            subagents: Vec::new(),
            completed_count: 0,
        }];
        // Use selected=1 (out of range) so is_selected is false, allowing permission bg to show
        let lines = build_office_lines(&entries, 80, 1, 0);
        assert!(lines.len() > 4);
        // Verify permission background is applied — check that spans have the permission bg color
        let first_line_spans = &lines[0].spans;
        let has_permission_bg = first_line_spans
            .iter()
            .any(|s| s.style.bg == Some(theme::COLOR_PERMISSION_BG));
        assert!(
            has_permission_bg,
            "Permission entry should have permission background"
        );
    }

    #[test]
    fn test_selected_line_range() {
        let state = make_agent_state("%0", AgentStatus::Running);
        let sessions = vec![make_session(vec![make_pane("%0", Some(state))])];
        let (start, end, total) = selected_line_range(&sessions, &HashMap::new(), 80, 0, 0);
        assert!(start <= end);
        assert!(end < total);
        assert!(total > 4);
    }
}
