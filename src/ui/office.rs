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

    // Find the selected entry's visual block.
    // Main rooms start with ┌─  (unique marker). Subagent cards use ┌─── (no space after ┌).
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

        // Main agent room
        let room_lines = render_agent_room(
            &entry.agent_state,
            content_width,
            is_selected,
            is_permission,
            anim_frame,
        );
        lines.extend(room_lines);

        // Subagent cards below the main room
        if !entry.subagents.is_empty() {
            lines.push(Line::from(""));
            let card_lines = render_subagent_cards(&entry.subagents, content_width, anim_frame);
            lines.extend(card_lines);
        }

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

fn render_subagent_cards(
    subagents: &[SubagentInfo],
    content_width: usize,
    _anim_frame: usize,
) -> Vec<Line<'static>> {
    if subagents.is_empty() {
        return Vec::new();
    }

    let dim_style = Style::default().fg(theme::COLOR_DIM);
    let border_style = Style::default().fg(theme::COLOR_PURPLE);
    let white_border = Style::default().fg(theme::COLOR_WHITE);
    let permission_style = Style::default()
        .fg(theme::COLOR_LIGHT_PURPLE)
        .bg(theme::COLOR_PERMISSION_BG);

    struct CardLayout {
        name: String,
        status: String,
        activity: String,
        info: String,
        inner_width: usize,
        is_permission: bool,
    }

    let cards: Vec<CardLayout> = subagents
        .iter()
        .map(|sub| {
            let name = format!("🤖 {}", sub.short_id);
            let is_permission = matches!(sub.state, SubagentStatus::Waiting);

            let (status_emoji, status_label) = match sub.state {
                SubagentStatus::Running => ("🔧", "Running"),
                SubagentStatus::Waiting => ("💤", "Waiting"),
                SubagentStatus::Ended => ("✅", "Done"),
            };
            let status = format!("{} {}", status_emoji, status_label);

            let duration = format_duration(sub.updated_at);
            let tool_count = sub.tools.len();

            let (activity, info) = match sub.state {
                SubagentStatus::Running => {
                    let act = sub
                        .tools
                        .first()
                        .map(|t| match &t.detail {
                            Some(d) => format!("{} {}", t.name, d),
                            None => t.name.clone(),
                        })
                        .unwrap_or_default();
                    let inf = format!("⏱️ {}", duration);
                    (act, inf)
                }
                SubagentStatus::Waiting => {
                    ("🔐 Permission".to_string(), "⚠️ Needs you!".to_string())
                }
                SubagentStatus::Ended => {
                    let act = format!("{} ago", duration);
                    let inf = if tool_count > 0 {
                        format!("📋 {} tools", tool_count)
                    } else {
                        format!("⏱️ {}", duration)
                    };
                    (act, inf)
                }
            };

            let max_content_width = [
                name.as_str(),
                status.as_str(),
                activity.as_str(),
                info.as_str(),
            ]
            .iter()
            .map(|s| s.width())
            .max()
            .unwrap_or(0)
            .max(12);
            let inner_width = (max_content_width + 2).min(30);

            CardLayout {
                name,
                status,
                activity,
                info,
                inner_width,
                is_permission,
            }
        })
        .collect();

    if cards.is_empty() {
        return Vec::new();
    }

    let mut result = Vec::new();
    let mut idx = 0;

    while idx < cards.len() {
        let mut row_cards = vec![idx];
        let mut used = cards[idx].inner_width + 2;
        let mut j = idx + 1;
        while j < cards.len() {
            let card_total = cards[j].inner_width + 2;
            if used + 1 + card_total <= content_width {
                used += 1 + card_total;
                row_cards.push(j);
                j += 1;
            } else {
                break;
            }
        }

        for line_kind in 0..6 {
            let mut spans: Vec<Span<'static>> = Vec::new();
            for (ri, &ci) in row_cards.iter().enumerate() {
                if ri > 0 {
                    spans.push(Span::raw(" "));
                }
                let card = &cards[ci];
                let bstyle = if card.is_permission {
                    white_border
                } else {
                    border_style
                };

                match line_kind {
                    0 => {
                        spans.push(Span::styled("┌", bstyle));
                        spans.push(Span::styled("─".repeat(card.inner_width), bstyle));
                        spans.push(Span::styled("┐", bstyle));
                    }
                    5 => {
                        spans.push(Span::styled("└", bstyle));
                        spans.push(Span::styled("─".repeat(card.inner_width), bstyle));
                        spans.push(Span::styled("┘", bstyle));
                    }
                    n @ 1..=4 => {
                        let content = match (n - 1) as usize {
                            0 => &card.name,
                            1 => &card.status,
                            2 => &card.activity,
                            3 => &card.info,
                            _ => unreachable!(),
                        };
                        let text = format!(
                            "{}{:<width$}{}",
                            "│",
                            content,
                            "│",
                            width = card.inner_width
                        );
                        if card.is_permission {
                            spans.push(Span::styled(text, permission_style));
                        } else {
                            spans.push(Span::styled(text, dim_style));
                        }
                    }
                    _ => unreachable!(),
                }
            }
            result.push(Line::from(spans));
        }

        idx = j;
    }

    result
}

fn render_agent_room(
    agent: &AgentState,
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

    let status_label = match agent.status() {
        AgentStatus::Started => "starting",
        AgentStatus::Running => "running",
        AgentStatus::Waiting => "waiting",
        AgentStatus::Permission => "needs input!",
        AgentStatus::Ended => "ended",
    };

    let status_emoji = agent.event_emoji().unwrap_or(match agent.status() {
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
    let name_text = format!("🤖 {}", agent.event_label());
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
    let icon = theme::status_icon(&agent.status(), anim_frame);

    // Empty line after title
    let mut empty_spans = vec![Span::styled("│ ".to_string(), border_style)];
    let pad = content_width.saturating_sub(3);
    empty_spans.push(Span::styled(
        " ".repeat(pad),
        Style::default().fg(Color::Reset),
    ));
    empty_spans.push(Span::styled("│".to_string(), border_style));
    apply_bg(&mut empty_spans, bg_color, is_selected);
    lines.push(Line::from(empty_spans));

    // Activity line
    let act_icon = agent.event_emoji().unwrap_or(theme::ICON_TOOL);
    let activity = if let Some(tool_name) = agent.current_tool_name() {
        if let Some(detail) = agent.current_tool_detail() {
            format!("{} {} {}", act_icon, tool_name, detail)
        } else {
            format!("{} {}", act_icon, tool_name)
        }
    } else if let Some(emoji) = agent.event_emoji() {
        emoji.to_string()
    } else {
        "idle".to_string()
    };

    let mut activity_spans = vec![
        Span::styled("│ ".to_string(), border_style),
        Span::styled(
            format!(" {} ", icon),
            theme::status_style(&agent.status(), true),
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
    let active_tools = agent.active_tools();
    let visible_tools: Vec<_> = if active_tools.len() > 3 {
        active_tools[active_tools.len() - 3..].iter().collect()
    } else {
        active_tools.iter().collect()
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

    // Empty line after tools
    let mut tool_end_spans = vec![Span::styled("│ ".to_string(), border_style)];
    let pad2 = content_width.saturating_sub(3);
    tool_end_spans.push(Span::styled(
        " ".repeat(pad2),
        Style::default().fg(Color::Reset),
    ));
    tool_end_spans.push(Span::styled("│".to_string(), border_style));
    apply_bg(&mut tool_end_spans, bg_color, is_selected);
    lines.push(Line::from(tool_end_spans));

    // Failure detail line (red)
    if let Some(failure) = agent.failure_detail() {
        let failure_style = Style::default().fg(theme::COLOR_FAILURE);
        let mut failure_spans = vec![
            Span::styled("│ ".to_string(), border_style),
            Span::styled(
                format!(" {} {}", theme::ICON_FAILURE, failure),
                failure_style,
            ),
        ];
        let used: usize = failure_spans.iter().map(|s| s.content.width()).sum();
        let pad = content_width.saturating_sub(used + 1);
        failure_spans.push(Span::styled(
            " ".repeat(pad),
            Style::default().fg(Color::Reset),
        ));
        failure_spans.push(Span::styled("│".to_string(), border_style));
        apply_bg(&mut failure_spans, bg_color, is_selected);
        lines.push(Line::from(failure_spans));
    }

    // Duration + summary line
    let duration = format_duration(agent.updated_at);
    let tool_count = if active_tools.is_empty() {
        String::new()
    } else {
        format!("  📋 {} tools", active_tools.len())
    };
    let info = format!("⏱️ {}{}", duration, tool_count);

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
