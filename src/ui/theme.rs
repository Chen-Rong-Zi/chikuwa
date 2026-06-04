use ratatui::style::{Color, Style};

use crate::agent::state::AgentStatus;

// 3-color palette
pub const COLOR_WHITE: Color = Color::Rgb(0xff, 0xff, 0xff);
pub const COLOR_PURPLE: Color = Color::Rgb(0x92, 0x93, 0xfe);
pub const COLOR_LIGHT_PURPLE: Color = Color::Rgb(0xb6, 0xb9, 0xff);

// NerdFont icons
pub const ICON_CARET_RIGHT: &str = "\u{f0da}"; //
pub const ICON_SESSION: &str = "\u{ebc8}";
pub const ICON_GITHUB: &str = "\u{f09b}";
pub const ICON_GIT_BRANCH: &str = "\u{e725}"; //
pub const ICON_PR: &str = "\u{f407}"; //
pub const SPINNER_FRAMES: &[&str] = &["·", "✢", "*", "✶", "✻", "✽"];
pub const ICON_WAITING: &str = "\u{f28b}"; //
pub const ICON_PERMISSION: &str = "\u{f071}"; //
pub const ICON_STARTED: &str = "\u{f04b}"; //
pub const ICON_CLAUDE: &str = "\u{f06a9}"; // 󰚩
pub const ICON_NEOVIM: &str = "\u{e7c5}"; //
pub const ICON_TERMINAL: &str = "\u{f489}"; //
pub const ICON_WINDOW: &str = "\u{f10aa}"; // 󱂪
pub const ICON_BOLT: &str = "\u{f0e7}"; //
pub const ICON_TOOL: &str = "\u{f0ad}"; //
pub const ICON_FAILURE: &str = "\u{f00d}"; //  cross
pub const COLOR_YELLOW: Color = Color::Rgb(0xff, 0xd7, 0x00);
pub const COLOR_PERMISSION_BG: Color = Color::Rgb(0x3a, 0x1a, 0x3a);

// UI accent colors (exception: not in the 3-color palette, used for dim/selection)
pub const COLOR_DIM: Color = Color::Rgb(0x7a, 0x7a, 0x7a);
pub const COLOR_SELECTED_BG: Color = Color::DarkGray;
pub const COLOR_FAILURE: Color = Color::Rgb(0xff, 0x44, 0x44);
/// Return a color for a usage utilization value (0.0–1.0).
pub fn usage_color(utilization: f64) -> Color {
    if utilization < 0.8 {
        COLOR_LIGHT_PURPLE
    } else {
        COLOR_WHITE
    }
}

pub fn status_icon(status: &AgentStatus, anim_frame: usize) -> &'static str {
    match status {
        AgentStatus::Running => SPINNER_FRAMES[anim_frame % SPINNER_FRAMES.len()],
        AgentStatus::Waiting => ICON_WAITING,
        AgentStatus::Permission => ICON_PERMISSION,
        AgentStatus::Started => ICON_STARTED,
        AgentStatus::Ended => ICON_STARTED,
    }
}

pub fn status_color(status: &AgentStatus, session_attached: bool) -> Color {
    match status {
        AgentStatus::Permission | AgentStatus::Waiting => {
            if session_attached {
                COLOR_LIGHT_PURPLE
            } else {
                COLOR_PURPLE
            }
        }
        _ => COLOR_DIM,
    }
}

/// Face emoji for agent status, animated only when working or needing attention.
/// Running and Permission animate; all other states are static.
pub fn agent_face_emoji(
    status: &AgentStatus,
    has_failure: bool,
    anim_frame: usize,
) -> &'static str {
    match status {
        AgentStatus::Started => "🟢",
        AgentStatus::Running => {
            let frames = ["🤔", "🧐"];
            frames[anim_frame % frames.len()]
        }
        AgentStatus::Permission => {
            let frames = ["🥺✋", "😫🙋", "🥺✋", "😫🙋"];
            frames[anim_frame % frames.len()]
        }
        AgentStatus::Waiting => "😴",
        AgentStatus::Ended => {
            if has_failure {
                "❌"
            } else {
                "✅"
            }
        }
    }
}

/// Tool emoji with 2-frame animation per tool type.
pub fn tool_emoji(tool_name: &str, anim_frame: usize) -> &'static str {
    let (a, b) = match tool_name {
        "Bash" | "bash" => ("💻", "⌨️"),
        "Read" | "read" => ("📖", "👀"),
        "Write" | "write" => ("📝", "✍️"),
        "Edit" | "edit" => ("✏️", "🖊️"),
        "NotebookEdit" | "notebook_edit" => ("📓", "✍️"),
        "Grep" | "grep" => ("🔍", "🕵️"),
        "Glob" | "glob" => ("📂", "🗂️"),
        "WebFetch" | "web_fetch" => ("🌐", "⬇️"),
        "WebSearch" | "web_search" => ("🕵️", "🌐"),
        "Task" | "Agent" | "task" | "agent" => ("👶", "🥚"),
        "AskUserQuestion" => ("❓", "🤷"),
        "ExitPlanMode" => ("📋", "✅"),
        "file_edited" => ("✏️", "🖊️"),
        _ => {
            // MCP tools start with "mcp__"
            if tool_name.starts_with("mcp__") {
                ("⚙️", "🔧")
            } else {
                ("🔧", "⚙️")
            }
        }
    };
    if anim_frame.is_multiple_of(2) {
        a
    } else {
        b
    }
}

/// Static emoji for hook event names, used in title bar right side.
pub fn event_emoji(hook_event_name: &str) -> &'static str {
    match hook_event_name {
        "PreToolUse" => "🪝",
        "PostToolUse" => "🟩",
        "PostToolUseFailure" => "🟥",
        "UserPromptSubmit" | "UserPromptExpansion" => "✍️",
        "SubagentStart" => "👶",
        "SubagentStop" => "🔀",
        "Stop" => "✅",
        "StopFailure" => "❌",
        "PermissionRequest" | "PermissionDenied" => "🔐",
        "PreCompact" | "PostCompact" => "🗜️",
        "SessionStart" => "🟢",
        "SessionEnd" => "🏁",
        _ => "🔧",
    }
}

pub fn status_style(status: &AgentStatus, session_attached: bool) -> Style {
    Style::default().fg(status_color(status, session_attached))
}

pub fn branch_style() -> Style {
    Style::default().fg(COLOR_LIGHT_PURPLE)
}
