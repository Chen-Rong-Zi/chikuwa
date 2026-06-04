use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::agent::state::{AgentState, AgentStatus, AgentView};
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

/// Compute elapsed seconds since a Unix timestamp.
fn elapsed_secs(updated_at: u64) -> u64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    now.saturating_sub(updated_at)
}

/// Format an elapsed duration as human-readable string.
fn format_elapsed(elapsed: u64) -> String {
    if elapsed < 60 {
        format!("{}s", elapsed)
    } else if elapsed < 3600 {
        format!("{}m {}s", elapsed / 60, elapsed % 60)
    } else {
        format!("{}h {}m", elapsed / 3600, (elapsed % 3600) / 60)
    }
}

/// Format elapsed seconds since a Unix timestamp as human-readable string.
fn format_duration(updated_at: u64) -> String {
    format_elapsed(elapsed_secs(updated_at))
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

    // Find the selected entry's visual block.
    // Main rooms start with ┌─  (unique marker).
    // We scan for ┌─  to find main room starts, and track which entry index we're on.
    // The block ends at the next ┌─ , a ··· separator, or end of lines.
    let mut entry_idx = 0;
    let mut block_start = 0;
    let mut block_end = 0;
    let mut found = false;

    for (line_idx, line) in lines.iter().enumerate() {
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        if text.contains("┌─ ") {
            if entry_idx == selected {
                block_start = line_idx;
                found = true;
            } else if found {
                // Hit the next entry's main room
                block_end = line_idx - 1;
                break;
            }
            if !found {
                entry_idx += 1;
            }
        } else if text.trim().starts_with("···") && found {
            // Separator between entries
            block_end = line_idx;
            break;
        }
    }

    if found && block_end == 0 {
        block_end = total - 1;
    }

    if found {
        (block_start, block_end, total)
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

    // Title header
    lines.push(Line::from(Span::styled(
        "  🏢 Agent Office",
        Style::default()
            .fg(theme::COLOR_WHITE)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    for (idx, entry) in entries.iter().enumerate() {
        let is_selected = idx == selected;
        let is_permission = entry.agent_state.status() == AgentStatus::Permission;

        // Main agent room (includes subagent lines)
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
        match entry.agent_state.status() {
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

    let border_style = if is_permission {
        Style::default().fg(theme::COLOR_WHITE)
    } else {
        Style::default().fg(theme::COLOR_PURPLE)
    };

    let has_failure = agent.failure_detail().is_some();
    let elapsed = elapsed_secs(agent.updated_at);
    let face = theme::agent_face_emoji(&agent.status(), has_failure, anim_frame, elapsed);

    // Right indicator for title bar
    let right_indicator = match agent.status() {
        AgentStatus::Running => theme::tool_spinner(anim_frame).to_string(),
        AgentStatus::Permission => String::new(),
        AgentStatus::Waiting => "💤".repeat(theme::idle_zzz_count(elapsed)),
        _ => agent
            .event_emoji()
            .unwrap_or_else(|| theme::event_emoji(agent.event_label()))
            .to_string(),
    };

    let mut lines = Vec::new();

    // Title bar: ┌─ face ──── right_indicator ──┐
    let face_width = face.width();
    let right_width = right_indicator.width();
    let inner_width = content_width.saturating_sub(2); // ┌ and ┐
    let fill_len = if right_indicator.is_empty() {
        inner_width.saturating_sub(face_width + 2) // ┌─ face ──┐
    } else {
        inner_width.saturating_sub(face_width + right_width + 4) // ┌─ face ──── right ──┐
    };
    let fill = "─".repeat(fill_len);

    let mut border_spans = vec![
        Span::styled("┌─ ".to_string(), border_style),
        Span::styled(face.to_string(), name_style),
    ];
    if right_indicator.is_empty() {
        border_spans.push(Span::styled(format!(" {}┐", fill), border_style));
    } else {
        border_spans.push(Span::styled(format!(" {} ", fill), border_style));
        border_spans.push(Span::styled(right_indicator, dim_style));
        border_spans.push(Span::styled(" ─┐".to_string(), border_style));
    }
    apply_bg(&mut border_spans, bg_color, is_selected);
    lines.push(Line::from(border_spans));

    // Empty line after title
    lines.push(make_empty_line(
        content_width,
        border_style,
        bg_color,
        is_selected,
    ));

    // Tool lines (all with ◐ spinner)
    let active_tools = agent.active_tools();
    for (i, tool) in active_tools.iter().enumerate() {
        let spinner = theme::tool_spinner(anim_frame + i);
        let detail = tool.detail.as_deref().unwrap_or("");
        let tool_text = format!("{} {}", spinner, detail);
        let tool_text = truncate_to_width(&tool_text, content_width.saturating_sub(4));
        let mut tool_spans = vec![
            Span::styled("│ ".to_string(), border_style),
            Span::styled(format!(" {}", tool_text), dim_style),
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

    // Empty line before subagents (if any subagents or completed count)
    if !subagents.is_empty() || completed_count > 0 {
        lines.push(make_empty_line(
            content_width,
            border_style,
            bg_color,
            is_selected,
        ));
    }

    // Subagent lines
    for (si, sub) in subagents.iter().enumerate() {
        let sub_elapsed = elapsed_secs(sub.updated_at);
        let sub_status = match sub.state {
            SubagentStatus::Running => AgentStatus::Running,
            SubagentStatus::Waiting => AgentStatus::Permission,
            SubagentStatus::Ended => AgentStatus::Ended,
        };
        let sub_face = theme::agent_face_emoji(&sub_status, false, anim_frame + si, sub_elapsed);
        let duration = format_duration(sub.updated_at);

        if sub.tools.is_empty() {
            // No tools: │ 👶 face    duration │
            let text = format!("👶 {}  {}", sub_face, duration);
            let text = truncate_to_width(&text, content_width.saturating_sub(4));
            let mut spans = vec![
                Span::styled("│ ".to_string(), border_style),
                Span::styled(format!(" {}", text), dim_style),
            ];
            let used: usize = spans.iter().map(|s| s.content.width()).sum();
            let pad = content_width.saturating_sub(used + 1);
            spans.push(Span::styled(
                " ".repeat(pad),
                Style::default().fg(Color::Reset),
            ));
            spans.push(Span::styled("│".to_string(), border_style));
            apply_bg(&mut spans, bg_color, is_selected);
            lines.push(Line::from(spans));
        } else {
            // First tool: │ 👶 face spinner detail  duration │
            let first_tool = &sub.tools[0];
            let spinner = theme::tool_spinner(anim_frame + si);
            let detail = first_tool.detail.as_deref().unwrap_or("");
            let text = format!("👶 {} {} {}", sub_face, spinner, detail);
            let dur_text = format!("  {}", duration);
            let max_detail_width = content_width
                .saturating_sub(4)
                .saturating_sub(dur_text.width());
            let text = truncate_to_width(&text, max_detail_width);
            let mut spans = vec![
                Span::styled("│ ".to_string(), border_style),
                Span::styled(format!(" {}", text), dim_style),
            ];
            // Right-align duration
            let used: usize = spans.iter().map(|s| s.content.width()).sum();
            let pad = content_width.saturating_sub(used + dur_text.width() + 1);
            spans.push(Span::styled(
                " ".repeat(pad),
                Style::default().fg(Color::Reset),
            ));
            spans.push(Span::styled(dur_text, dim_style));
            spans.push(Span::styled("│".to_string(), border_style));
            apply_bg(&mut spans, bg_color, is_selected);
            lines.push(Line::from(spans));

            // Subsequent tools: │    spinner detail │
            for (ti, tool) in sub.tools[1..].iter().enumerate() {
                let spinner = theme::tool_spinner(anim_frame + si + ti + 1);
                let detail = tool.detail.as_deref().unwrap_or("");
                let text = format!("   {} {}", spinner, detail);
                let text = truncate_to_width(&text, content_width.saturating_sub(4));
                let mut spans = vec![
                    Span::styled("│".to_string(), border_style),
                    Span::styled(format!(" {}", text), dim_style),
                ];
                let used: usize = spans.iter().map(|s| s.content.width()).sum();
                let pad = content_width.saturating_sub(used + 1);
                spans.push(Span::styled(
                    " ".repeat(pad),
                    Style::default().fg(Color::Reset),
                ));
                spans.push(Span::styled("│".to_string(), border_style));
                apply_bg(&mut spans, bg_color, is_selected);
                lines.push(Line::from(spans));
            }
        }
    }

    // Completed subagent count
    if completed_count > 0 {
        let text = format!("✓ {} completed", completed_count);
        let mut spans = vec![
            Span::styled("│ ".to_string(), border_style),
            Span::styled(format!(" {}", text), dim_style),
        ];
        let used: usize = spans.iter().map(|s| s.content.width()).sum();
        let pad = content_width.saturating_sub(used + 1);
        spans.push(Span::styled(
            " ".repeat(pad),
            Style::default().fg(Color::Reset),
        ));
        spans.push(Span::styled("│".to_string(), border_style));
        apply_bg(&mut spans, bg_color, is_selected);
        lines.push(Line::from(spans));
    }

    // Permission warning text
    if agent.status() == AgentStatus::Permission {
        let warning = theme::permission_warning_text(elapsed);
        let mut spans = vec![
            Span::styled("│ ".to_string(), border_style),
            Span::styled(format!(" {}", warning), name_style),
        ];
        let used: usize = spans.iter().map(|s| s.content.width()).sum();
        let pad = content_width.saturating_sub(used + 1);
        spans.push(Span::styled(
            " ".repeat(pad),
            Style::default().fg(Color::Reset),
        ));
        spans.push(Span::styled("│".to_string(), border_style));
        apply_bg(&mut spans, bg_color, is_selected);
        lines.push(Line::from(spans));
    }

    // Failure detail line
    if let Some(failure) = agent.failure_detail() {
        let failure_style = Style::default().fg(theme::COLOR_FAILURE);
        let mut spans = vec![
            Span::styled("│ ".to_string(), border_style),
            Span::styled(format!(" 💥 {}", failure), failure_style),
        ];
        let used: usize = spans.iter().map(|s| s.content.width()).sum();
        let pad = content_width.saturating_sub(used + 1);
        spans.push(Span::styled(
            " ".repeat(pad),
            Style::default().fg(Color::Reset),
        ));
        spans.push(Span::styled("│".to_string(), border_style));
        apply_bg(&mut spans, bg_color, is_selected);
        lines.push(Line::from(spans));
    }

    // Empty line before duration
    lines.push(make_empty_line(
        content_width,
        border_style,
        bg_color,
        is_selected,
    ));

    // Duration line with contextual label (reuse elapsed for consistency)
    let label = theme::duration_label(&agent.status());
    let duration = format_elapsed(elapsed);
    let info = if label.is_empty() {
        format!("⏱️ {}", duration)
    } else {
        format!("⏱️ {}: {}", label, duration)
    };
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

/// Truncate a string to fit within max_width Unicode columns, appending "…" if truncated.
fn truncate_to_width(text: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    let total_width = text.width();
    if total_width <= max_width {
        return text.to_string();
    }
    // Leave room for "…"
    let target = max_width.saturating_sub(1);
    let mut width = 0;
    let mut result = String::new();
    for ch in text.chars() {
        let ch_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + ch_width > target {
            break;
        }
        result.push(ch);
        width += ch_width;
    }
    result.push('…');
    result
}

/// Make an empty content line with borders: │   │
fn make_empty_line(
    content_width: usize,
    border_style: Style,
    bg_color: Option<Color>,
    is_selected: bool,
) -> Line<'static> {
    let pad = content_width.saturating_sub(3);
    let mut spans = vec![Span::styled("│ ".to_string(), border_style)];
    spans.push(Span::styled(
        " ".repeat(pad),
        Style::default().fg(Color::Reset),
    ));
    spans.push(Span::styled("│".to_string(), border_style));
    apply_bg(&mut spans, bg_color, is_selected);
    Line::from(spans)
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
    use crate::agent::claude::ClaudeState;
    use crate::agent::state::{ActiveTool, AgentData, AgentStatus, ToolKey};
    use crate::tmux::types::{TmuxPane, TmuxSession, TmuxWindow};
    use std::collections::HashMap;

    fn make_agent_state(pane_id: &str, status: AgentStatus) -> AgentState {
        AgentState::new(
            pane_id.to_string(),
            AgentData::Claude(ClaudeState {
                session_id: None,
                agent_id: None,
                status,
                hook_event_name: "PreToolUse".to_string(),
                event_emoji: "🔧".to_string(),
                tool_name: Some("Read".to_string()),
                tool_detail: Some("src/main.rs".to_string()),
                active_tools: vec![ActiveTool {
                    key: ToolKey::Claude {
                        tool_use_id: "toolu_test".to_string(),
                    },
                    name: "Read".to_string(),
                    detail: Some("src/main.rs".to_string()),
                    failure_detail: None,
                }],
                failure_detail: None,
            }),
        )
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
        assert_eq!(entries[0].agent_state.status(), AgentStatus::Running);
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
        if let AgentData::Claude(ref mut c) = state.data {
            c.event_emoji = "🔐".to_string();
        }
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
        // Title is at line 0, room starts at line 2
        // Verify permission background is applied on room border line
        let room_border_spans = &lines[2].spans;
        let has_permission_bg = room_border_spans
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
