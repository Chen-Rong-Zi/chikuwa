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

interface AgentState {
	tmux_pane: string;
	session_id?: string;
	state: "started" | "running" | "waiting" | "permission" | "ended";
	updated_at: number;
	hook_event_name?: string;
	tool_name?: string;
	tool_detail?: string;
	tools?: Array<{ name: string; detail?: string }>;
}

interface ToolInfo {
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

const sendToIpc = async ($: any, state: AgentState) => {
	try {
		const files = readdirSync(CHIKUWA_STATE_DIR);
		const socketFile = files.find(f => f.endsWith(".sock"));
		if (!socketFile) {
			log("No socket file found");
			return;
		}
		const socketPath = join(CHIKUWA_STATE_DIR, socketFile);
		const json = JSON.stringify(state);
		await $`echo ${json} | nc -U ${socketPath}`.nothrow();
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
	let activeTools: ToolInfo[] = [];
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

			const toolInfo: ToolInfo = { name: toolName, detail: toolDetail };
			if (!activeTools.some(t => t.name === toolName && t.detail === toolDetail)) {
				activeTools.push(toolInfo);
			}

			await sendState({
				tmux_pane: tmuxPane,
				session_id: currentSessionId || undefined,
				state: "running",
				updated_at: now(),
				hook_event_name: "tool.execute",
				tool_name: toolName,
				tool_detail: toolDetail,
				tools: [...activeTools],
			});
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
						await sendState({
							tmux_pane: tmuxPane,
							session_id: currentSessionId || undefined,
							state: "started",
							updated_at: now(),
							hook_event_name: "session.busy",
						});
					} else if (status.type === "idle" && isBusy) {
						isBusy = false;
						activeTools = [];
						await sendState({
							tmux_pane: tmuxPane,
							session_id: currentSessionId || undefined,
							state: "waiting",
							updated_at: now(),
							hook_event_name: "session.idle",
						});
					}
					break;
				}

				case "session.idle": {
					log("Session idle event");
					isBusy = false;
					activeTools = [];
					await sendState({
						tmux_pane: tmuxPane,
						session_id: currentSessionId || undefined,
						state: "waiting",
						updated_at: now(),
						hook_event_name: "session.idle",
					});
					break;
				}

				case "session.created": {
					const newSessionId = props.session?.id || props.id || sessionId;
					if (newSessionId) updateSession(newSessionId);
					await sendState({
						tmux_pane: tmuxPane,
						session_id: newSessionId || currentSessionId || undefined,
						state: "started",
						updated_at: now(),
						hook_event_name: "session.created",
					});
					break;
				}

				case "session.deleted": {
					if (props.sessionID === currentSessionId) {
						await sendState({
							tmux_pane: tmuxPane,
							session_id: currentSessionId,
							state: "ended",
							updated_at: now(),
							hook_event_name: "session.deleted",
						});
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
						const toolInfo: ToolInfo = { name: toolName, detail: toolDetail };
						if (!activeTools.some(t => t.name === toolName && t.detail === toolDetail)) {
							activeTools.push(toolInfo);
						}
						await sendState({
							tmux_pane: tmuxPane,
							session_id: currentSessionId || undefined,
							state: "running",
							updated_at: now(),
							hook_event_name: "tool.running",
							tool_name: toolName,
							tool_detail: toolDetail,
							tools: [...activeTools],
						});
					} else if (toolStatus === "completed" || toolStatus === "error") {
						activeTools = activeTools.filter(t => t.name !== toolName);
						await sendState({
							tmux_pane: tmuxPane,
							session_id: currentSessionId || undefined,
							state: "running",
							updated_at: now(),
							hook_event_name: `tool.${toolStatus}`,
							tool_name: toolName,
							tools: [...activeTools],
						});
					}
					break;
				}

				// File edits
				case "file.edited": {
					const filePath = props.path || props.filePath;
					if (!filePath) return;
					await sendState({
						tmux_pane: tmuxPane,
						session_id: currentSessionId || undefined,
						state: "running",
						updated_at: now(),
						hook_event_name: "file.edited",
						tool_name: "edit",
						tool_detail: filePath,
					});
					break;
				}

				// Permission events
				case "permission.asked":
				case "permission.updated": {
					await sendState({
						tmux_pane: tmuxPane,
						session_id: currentSessionId || undefined,
						state: "permission",
						updated_at: now(),
						hook_event_name: "permission.asked",
						tool_detail: props.permission?.tool || props.permission?.type,
					});
					break;
				}

				case "permission.replied": {
					if (isBusy) {
						await sendState({
							tmux_pane: tmuxPane,
							session_id: currentSessionId || undefined,
							state: "running",
							updated_at: now(),
							hook_event_name: "permission.replied",
							tools: [...activeTools],
						});
					}
					break;
				}

				// Command executed
				case "command.executed": {
					const command = props.command;
					if (Array.isArray(command) && command[0] === "bash") {
						await sendState({
							tmux_pane: tmuxPane,
							session_id: currentSessionId || undefined,
							state: "running",
							updated_at: now(),
							hook_event_name: "command.executed",
							tool_name: "bash",
							tool_detail: command.slice(1).join(" "),
						});
					}
					break;
				}
			}
		},
	};
};
