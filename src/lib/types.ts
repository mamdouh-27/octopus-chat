/** Mirrors the Rust models in `octopus-core::models`. */

export type Mode = "plan" | "build";
export type PermissionMode = "ask" | "auto_review" | "full_access";
export type RunState =
  | "idle"
  | "running"
  | "waiting_approval"
  | "paused"
  | "done"
  | "error";
export type Role = "user" | "assistant" | "tool";
export type MessageState = "pending" | "streaming" | "done" | "error" | "aborted";
export type ToolState = "pending" | "running" | "done" | "error" | "denied";
export type TodoState = "pending" | "active" | "done";
export type Dialect = "openai" | "anthropic" | "gemini";

export interface Attachment {
  id: string;
  path: string;
  mime: string;
  bytes: number;
  preview: string | null;
}

export interface Message {
  id: string;
  sessionId: string;
  role: Role;
  content: string;
  reasoning: string | null;
  state: MessageState;
  model: string | null;
  error: string | null;
  tokensIn: number;
  tokensOut: number;
  supersedes: string | null;
  attachments: Attachment[];
  createdAt: number;
}

export interface ToolCall {
  id: string;
  sessionId: string;
  messageId: string;
  name: string;
  input: string;
  output: string | null;
  state: ToolState;
  deniedReason: string | null;
  diff: string | null;
  startedAt: number;
  endedAt: number | null;
}

/** Rich presentation hint from the Rust `Display` enum. */
export type ToolDisplay =
  | { kind: "file"; path: string; bytes: number; preview: string }
  | { kind: "diff"; path: string }
  | { kind: "todoList"; items: { id: string; content: string; status: TodoState }[] }
  | { kind: "command"; command: string; exitCode: number | null }
  | { kind: "matches"; count: number };

export interface ToolOutcome {
  content: string;
  diff?: string;
  display?: ToolDisplay;
  isError: boolean;
}

export interface TodoItem {
  id: string;
  content: string;
  status: TodoState;
}

export interface Checkpoint {
  id: string;
  sessionId: string;
  messageId: string | null;
  label: string;
  gitHead: string | null;
  createdAt: number;
}

export interface Transcript {
  messages: Message[];
  toolCalls: ToolCall[];
  todos: TodoItem[];
}

export interface Project {
  id: string;
  name: string;
  roots: string[];
  instructions: string;
  permissionMode: PermissionMode;
  accent: string | null;
  archived: boolean;
  createdAt: number;
  updatedAt: number;
}

export interface Session {
  id: string;
  projectId: string | null;
  title: string;
  mode: Mode;
  state: RunState;
  provider: string;
  model: string;
  reasoningEffort: string | null;
  temperature: number | null;
  maxTokens: number | null;
  permissionMode: PermissionMode;
  goal: string | null;
  forkedFrom: string | null;
  worktree: string | null;
  favorite: boolean;
  tokensIn: number;
  tokensOut: number;
  costUsd: number;
  createdAt: number;
  updatedAt: number;
}

export interface Settings {
  theme: string;
  appearance: string | null;
  fontFamily: string;
  fontSize: number;
  uiScale: number;
  codeFontFamily: string;
  lineHeight: number;
  reduceMotion: boolean;
  highContrast: boolean;
  keybinds: Record<string, string>;
  defaultProvider: string;
  defaultModel: string;
  defaultPermissionMode: PermissionMode;
  sendOnEnter: boolean;
  autoTitle: boolean;
  streamMarkdown: boolean;
  showReasoning: boolean;
  showTokenCost: boolean;
  preventSleepWhileRunning: boolean;
  notifyOnComplete: boolean;
  notifyOnInputNeeded: boolean;
  localOnly: boolean;
  telemetry: boolean;
  ignoreGlobs: string[];
  maxContextMessages: number;
  compactThresholdPercent: number;
}

export const PERMISSION_LABELS: Record<PermissionMode, string> = {
  ask: "Ask for approval",
  auto_review: "Approve for me",
  full_access: "Full access",
};

export const PERMISSION_HINTS: Record<PermissionMode, string> = {
  ask: "Every tool call is proposed before it runs.",
  auto_review: "Mutations are reviewed automatically instead of interrupting you.",
  full_access: "Nothing prompts. Only safe inside a sandbox.",
};