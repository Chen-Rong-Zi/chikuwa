/**
 * Chikuwa plugin for OpenCode
 *
 * Integrates OpenCode with chikuwa TUI for agent state tracking.
 * Sends agent state updates via IPC to the chikuwa daemon.
 */

import { appendFileSync, mkdirSync, readdirSync } from "fs";
import { join } from "path";

const LOG_FILE = "/tmp/chikuwa-opencode-plugin.log";
const CHIKUWA_STATE_DIR = process.env.XDG_RUNTIME_DIR
	? join(process.env.XDG_RUNTIME_DIR, "chikuwa")
	: "/tmp/chikuwa";
const AGENT_STATES_FILE = join(CHIKUWA_STATE_DIR, "agent_states.jsonl");

const log = (msg: string, data?: unknown) => {
	const timestamp = new Date().toISOString();
	const logMsg = `[${timestamp}] ${msg}${data ? " " + JSON.stringify(data) : ""}\n`;
	try {
		appendFileSync(LOG_FILE, logMsg);
	} catch {
		// ignore
	}
};

interface OpenCodeState {
	session_id?: string;
	status: "started" | "running" | "waiting" | "permission" | "ended";
	event_type?: string;
	event_emoji?: string;
	tool_name?: string;
	tool_detail?: string;
	active_tools: ActiveTool[];
	is_busy: boolean;
}

interface AgentState {
	tmux_pane: string;
	updated_at: number;
	data: {
		type: "opencode";
	} & OpenCodeState;
}

interface ActiveTool {
	key: { type: "opencode"; name: string; detail?: string };
	name: string;
	detail?: string;
}

const now = () => Math.floor(Date.now() / 1000);
const getTmuxPane = () => process.env.TMUX_PANE || "";

const appendAgentState = (state: AgentState) => {
	try {
		mkdirSync(CHIKUWA_STATE_DIR, { recursive: true });
		appendFileSync(AGENT_STATES_FILE, JSON.stringify(state) + "\n", "utf8");
	} catch (e) {
		log("Failed to append agent state", { error: String(e) });
	}
};

const makeState = (tmux_pane: string, opencode: Omit<OpenCodeState, never>): AgentState => ({
	tmux_pane,
	updated_at: now(),
	data: {
		type: "opencode",
		...opencode,
	},
});

const sendToIpc = async ($: any, state: AgentState) => {
	try {
		const files = readdirSync(CHIKUWA_STATE_DIR);
		const socketFiles = files.filter(f => f.endsWith(".sock"));
		if (socketFiles.length === 0) {
			log("No socket file found");
			return;
		}
		const json = JSON.stringify(state);
		// Send to all socket files (like Rust broadcast_state does).
		// Stale sockets will fail silently — only live TUI instances receive the message.
		for (const socketFile of socketFiles) {
			const socketPath = join(CHIKUWA_STATE_DIR, socketFile);
			await $`echo ${json} | nc -U ${socketPath}`.nothrow();
		}
	} catch (e) {
		log("Failed to send IPC", { error: String(e) });
	}
};

const extractToolDetail = (tool: string, args: Record<string, unknown>): string | undefined => {
	switch (tool) {
		case "bash": return args.command as string | undefined;
		case "read": {
			const path = args.filePath as string | undefined;
			const offset = args.offset as number | undefined;
			return offset ? `${path}:${offset}` : path;
		}
		case "write":
		case "edit": return args.filePath as string | undefined;
		case "grep":
		case "glob": return args.pattern as string | undefined;
		case "web_fetch": return args.url as string | undefined;
		case "web_search": return args.query as string | undefined;
		case "task": return args.description as string | undefined;
		default: return undefined;
	}
};

export const ChikuwaPlugin = async ({ client, directory, $ }: { client: any; directory: string; $: any }) => {
	const tmuxPane = getTmuxPane();
	if (!tmuxPane) {
		log("Not in tmux, plugin disabled");
		return {};
	}

	log("Plugin loading", { tmuxPane, directory });

	let currentSessionId = "";
	let activeTools: ActiveTool[] = [];
	let isBusy = false;

	const sendState = async (state: AgentState) => {
		appendAgentState(state);
		await sendToIpc($, state);
	};

	const updateSession = (sessionId: string) => {
		if (sessionId && sessionId !== currentSessionId) {
			currentSessionId = sessionId;
			log("Session updated", { sessionId });
		}
	};

	return {
		// Tool execution hooks
		"tool.execute.before": async (input: { tool: string; sessionID?: string }, output: { args: Record<string, unknown> }) => {
			const toolName = input.tool;
			const toolDetail = extractToolDetail(toolName, output.args);
			log("Tool executing", { tool: toolName, detail: toolDetail });

			if (input.sessionID) updateSession(input.sessionID);

			const tool: ActiveTool = {
				key: { type: "opencode", name: toolName, detail: toolDetail },
				name: toolName,
				detail: toolDetail,
			};
			if (!activeTools.some(t => t.key.name === toolName && t.key.detail === toolDetail)) {
				activeTools.push(tool);
			}

			await sendState(makeState(tmuxPane, {
				session_id: currentSessionId || undefined,
				status: "running",
				event_type: "tool.execute",
				tool_name: toolName,
				tool_detail: toolDetail,
				active_tools: [...activeTools],
				is_busy: true,
			}));
		},

		// Main event handler - OpenCode sends all events through this
		event: async ({ event }: { event: { type: string; properties?: any } }) => {
			const eventType = event.type;
			const props = event.properties || {};

			log("Event received", { type: eventType });

			// Extract session ID from various event formats
			const sessionId = props.sessionID || props.session?.id || props.info?.id || props.info?.sessionID;
			if (sessionId) updateSession(sessionId);

			switch (eventType) {
				// Session status - busy/idle transitions
				case "session.status": {
					const status = props.status;
					if (!status) return;

					log("Session status event", { statusType: status.type });

					if (status.type === "busy" && !isBusy) {
						isBusy = true;
						activeTools = [];
						await sendState(makeState(tmuxPane, {
							session_id: currentSessionId || undefined,
							status: "started",
							event_type: "session.busy",
							event_emoji: "🚀",
							active_tools: [],
							is_busy: true,
						}));
					} else if (status.type === "idle" && isBusy) {
						isBusy = false;
						activeTools = [];
						await sendState(makeState(tmuxPane, {
							session_id: currentSessionId || undefined,
							status: "waiting",
							event_type: "session.idle",
							event_emoji: "💤",
							active_tools: [],
							is_busy: false,
						}));
					}
					break;
				}

				case "session.idle": {
					log("Session idle event");
					isBusy = false;
					activeTools = [];
					await sendState(makeState(tmuxPane, {
						session_id: currentSessionId || undefined,
						status: "waiting",
						event_type: "session.idle",
						event_emoji: "💤",
						active_tools: [],
						is_busy: false,
					}));
					break;
				}

				case "session.created": {
					const newSessionId = props.session?.id || props.id || sessionId;
					if (newSessionId) updateSession(newSessionId);
					await sendState(makeState(tmuxPane, {
						session_id: newSessionId || currentSessionId || undefined,
						status: "started",
						event_type: "session.created",
						event_emoji: "🚀",
						active_tools: [],
						is_busy: false,
					}));
					break;
				}

				case "session.deleted": {
					if (props.sessionID === currentSessionId) {
						await sendState(makeState(tmuxPane, {
							session_id: currentSessionId,
							status: "ended",
							event_type: "session.deleted",
							event_emoji: "🏁",
							active_tools: [],
							is_busy: false,
						}));
						currentSessionId = "";
						isBusy = false;
						activeTools = [];
					}
					break;
				}

				// Tool execution via message parts
				case "message.part.updated": {
					const part = props.part;
					if (!part || part.type !== "tool") return;

					const toolName = part.tool;
					const toolStatus = part.state?.status;
					const toolInput = part.state?.input;

					log("Tool part updated", { tool: toolName, status: toolStatus });

					if (toolStatus === "running") {
						const toolDetail = toolInput ? extractToolDetail(toolName, toolInput) : undefined;
						const tool: ActiveTool = {
							key: { type: "opencode", name: toolName, detail: toolDetail },
							name: toolName,
							detail: toolDetail,
						};
						if (!activeTools.some(t => t.key.name === toolName && t.key.detail === toolDetail)) {
							activeTools.push(tool);
						}
						await sendState(makeState(tmuxPane, {
							session_id: currentSessionId || undefined,
							status: "running",
							event_type: "tool.running",
							event_emoji: "🔧",
							tool_name: toolName,
							tool_detail: toolDetail,
							active_tools: [...activeTools],
							is_busy: true,
						}));
					} else if (toolStatus === "completed" || toolStatus === "error") {
						const toolDetail = toolInput ? extractToolDetail(toolName, toolInput) : undefined;
						// Match by key (name+detail) to correctly handle parallel tool calls
						activeTools = activeTools.filter(
							t => !(t.key.name === toolName && t.key.detail === toolDetail)
						);
						// Include the removing tool so Rust merge can match by key
						const removingTool: ActiveTool = {
							key: { type: "opencode", name: toolName, detail: toolDetail },
							name: toolName,
							detail: toolDetail,
						};
						await sendState(makeState(tmuxPane, {
							session_id: currentSessionId || undefined,
							status: "running",
							event_type: `tool.${toolStatus}`,
							tool_name: toolName,
							active_tools: [removingTool, ...activeTools],
							is_busy: true,
						}));
					}
					break;
				}

				// File edits
				case "file.edited": {
					const filePath = props.path || props.filePath;
					if (!filePath) return;
					const tool: ActiveTool = {
						key: { type: "opencode", name: "edit", detail: filePath },
						name: "edit",
						detail: filePath,
					};
					await sendState(makeState(tmuxPane, {
						session_id: currentSessionId || undefined,
						status: "running",
						event_type: "file.edited",
						event_emoji: "📝",
						tool_name: "edit",
						tool_detail: filePath,
						active_tools: [tool],
						is_busy: true,
					}));
					break;
				}

				// Permission events
				case "permission.asked":
				case "permission.updated": {
					await sendState(makeState(tmuxPane, {
						session_id: currentSessionId || undefined,
						status: "permission",
						event_type: "permission.asked",
						event_emoji: "🔐",
						tool_detail: props.permission?.tool || props.permission?.type,
						active_tools: [...activeTools],
						is_busy: false,
					}));
					break;
				}

				case "permission.replied": {
					if (isBusy) {
						await sendState(makeState(tmuxPane, {
							session_id: currentSessionId || undefined,
							status: "running",
							event_type: "permission.replied",
							active_tools: [...activeTools],
							is_busy: true,
						}));
					}
					break;
				}

				// Command executed
				case "command.executed": {
					const command = props.command;
					if (Array.isArray(command) && command[0] === "bash") {
						await sendState(makeState(tmuxPane, {
							session_id: currentSessionId || undefined,
							status: "running",
							event_type: "command.executed",
							tool_name: "bash",
							tool_detail: command.slice(1).join(" "),
							active_tools: [...activeTools],
							is_busy: true,
						}));
					}
					break;
				}
			}
		},
	};
};
