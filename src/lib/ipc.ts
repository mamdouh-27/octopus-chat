/**
 * Typed bindings for the Rust command surface.
 *
 * Every backend error arrives as `{ kind, message }`, so `invoke` failures are
 * normalised here into a real `Error` with a usable message instead of a string
 * that has to be re-parsed at each call site.
 */
import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  Checkpoint,
  Message,
  Mode,
  PermissionMode,
  Project,
  RunState,
  Session,
  Settings,
  TodoItem,
  ToolCall,
  ToolOutcome,
  Transcript,
} from "./types";

export class BackendError extends Error {
  readonly kind: string;
  constructor(kind: string, message: string) {
    super(message);
    this.name = "BackendError";
    this.kind = kind;
  }
}

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await tauriInvoke<T>(cmd, args);
  } catch (e) {
    if (e && typeof e === "object" && "kind" in e && "message" in e) {
      const w = e as { kind: string; message: string };
      throw new BackendError(w.kind, w.message);
    }
    throw new BackendError("unknown", typeof e === "string" ? e : JSON.stringify(e));
  }
}

export interface ResolvedProvider {
  id: string;
  label: string;
  dialect: "openai" | "anthropic" | "gemini";
  baseUrl: string;
  local: boolean;
  needsKey: boolean;
  hasKey: boolean;
  keyUrl: string | null;
  docsUrl: string | null;
  custom: boolean;
  models: ModelSpec[];
}

export interface ModelSpec {
  id: string;
  label: string;
  contextWindow: number | null;
  maxOutput: number | null;
  supportsTools: boolean;
  supportsVision: boolean;
  reasoning: boolean;
  inputCost: number | null;
  outputCost: number | null;
}

export interface CustomModel {
  id: string;
  label: string;
  dialect: "openai" | "anthropic" | "gemini";
  contextWindow: number | null;
  maxOutput: number | null;
  supportsTools: boolean;
  supportsVision: boolean;
  reasoning: boolean;
  inputCost: number | null;
  outputCost: number | null;
}

export interface CustomProvider {
  id: string;
  label: string;
  dialect: "openai" | "anthropic" | "gemini";
  baseUrl: string;
  local: boolean;
  apiKey: string | null;
  headers: [string, string][];
  models: CustomModel[];
}

export interface WorkspaceInfo {
  roots: string[];
  gitBranch: string | null;
  isRepo: boolean;
  entryPoints: string[];
}

export interface StorageInfo {
  databaseBytes: number;
  databasePath: string;
}

export interface AppPaths {
  data: string;
  database: string;
  config: string;
  themes: string;
  artifacts: string;
  logs: string;
  checkpoints: string;
}

export interface ApprovalRequest {
  callId: string;
  tool: string;
  summary: string;
  arguments: string;
  path: string | null;
}

export interface SendAck {
  message: Message;
}

export const api = {
  ping: () => invoke<string>("ping"),

  // Folder picking runs in Rust (see `pick_project_dirs`) rather than through
  // plugin-dialog, so it cannot be broken by a missing webview capability.
  pickProjectDirs: () => invoke<string[] | null>("pick_project_dirs"),
  validateProjectDir: (path: string) => invoke<string>("validate_project_dir", { path }),

  settingsGet: () => invoke<Settings>("settings_get"),
  settingsSave: (settings: Settings) => invoke<Settings>("settings_save", { settings }),

  projectsList: (includeArchived = false) =>
    invoke<Project[]>("projects_list", { includeArchived }),
  projectSave: (project: Project) => invoke<Project>("project_save", { project }),
  projectDelete: (id: string) => invoke<void>("project_delete", { id }),
  workspaceInspect: (roots: string[]) => invoke<WorkspaceInfo>("workspace_inspect", { roots }),
  instructionsScaffold: (roots: string[]) =>
    invoke<string>("instructions_scaffold", { roots }),

  sessionsList: (projectId: string | null, limit = 200) =>
    invoke<Session[]>("sessions_list", { projectId, limit }),
  sessionGet: (id: string) => invoke<Session>("session_get", { id }),
  sessionCreate: (provider?: string, model?: string, projectId?: string | null) =>
    invoke<Session>("session_create", { provider, model, projectId }),
  sessionUpdate: (session: Session) => invoke<Session>("session_update", { session }),
  sessionDelete: (id: string) => invoke<void>("session_delete", { id }),
  messagesList: (sessionId: string) => invoke<Transcript>("messages_list", { sessionId }),
  sessionRewind: (sessionId: string, messageId: string) =>
    invoke<Transcript>("session_rewind", { sessionId, messageId }),

  providersList: () => invoke<ResolvedProvider[]>("providers_list"),
  providerSetKey: (provider: string, key: string | null) =>
    invoke<void>("provider_set_key", { provider, key }),
  providerSaveCustom: (provider: CustomProvider) =>
    invoke<CustomProvider>("provider_save_custom", { provider }),
  providerDeleteCustom: (id: string) => invoke<void>("provider_delete_custom", { id }),
  providerModels: (provider: string) => invoke<ModelSpec[]>("provider_models", { provider }),
  providerProbeModels: (provider: string) => invoke<string[]>("provider_probe_models", { provider }),
  providerPing: (provider: string) => invoke<boolean>("provider_ping", { provider }),
  modelAdd: (provider: string, model: string, label?: string) =>
    invoke<CustomModel>("model_add", { provider, model, label }),

  rulesList: (projectId: string | null) => invoke<Rule[]>("rules_list", { projectId }),
  rulesSave: (projectId: string | null, rules: Rule[]) =>
    invoke<Rule[]>("rules_save", { projectId, rules }),

  sendMessage: (sessionId: string, text: string, images: string[] = []) =>
    invoke<SendAck>("send_message", { request: { sessionId, text, images } }),
  approve: (
    sessionId: string,
    callId: string,
    allow: boolean,
    always: boolean,
    reason?: string,
  ) => invoke<void>("approve", { sessionId, callId, allow, always, reason }),
  abortRun: (sessionId: string) => invoke<void>("abort_run", { sessionId }),
  sessionIsRunning: (sessionId: string) => invoke<boolean>("session_is_running", { sessionId }),

  todosSave: (sessionId: string, todos: TodoItem[]) =>
    invoke<void>("todos_save", { sessionId, todos }),

  checkpointCreate: (sessionId: string, messageId?: string, label?: string) =>
    invoke<Checkpoint>("checkpoint_create", { sessionId, messageId, label }),
  checkpointList: (sessionId: string) =>
    invoke<Checkpoint[]>("checkpoint_list", { sessionId }),
  gitState: (sessionId: string) => invoke<GitState>("git_state", { sessionId }),

  storageInfo: () => invoke<StorageInfo>("storage_info"),
  appPaths: () => invoke<AppPaths>("app_paths"),
  setLocalOnly: (localOnly: boolean) => invoke<void>("set_local_only", { localOnly }),

  panelTree: (sessionId: string, path = "", depth = 3) =>
    invoke<TreeNode>("panel_tree", { sessionId, path, depth }),
  panelRead: (sessionId: string, path: string) =>
    invoke<FileContent>("panel_read", { sessionId, path }),
  panelWrite: (sessionId: string, path: string, text: string) =>
    invoke<FileContent>("panel_write", { sessionId, path, text }),
  panelChanged: (sessionId: string) => invoke<string[]>("panel_changed", { sessionId }),

  windowMinimize: () => invoke<void>("window_minimize"),
  windowToggleMaximize: () => invoke<void>("window_toggle_maximize"),
  windowClose: () => invoke<void>("window_close"),
};

export interface TreeNode {
  name: string;
  path: string;
  isDir: boolean;
  size: number;
  children: TreeNode[];
}

export interface FileContent {
  path: string;
  bytes: number;
  language: string;
  text: string;
  binary: boolean;
}

export interface StatusEntry {
  path: string;
  status: string;
  staged: boolean;
}

export interface GitState {
  isRepo: boolean;
  head: string | null;
  branch: string | null;
  changed: StatusEntry[];
  diff: string;
}

export interface Rule {
  id: string;
  effect: "allow" | "deny";
  pattern: string;
  askFirst: boolean;
  projectId: string | null;
  enabled: boolean;
}

/** Payload shapes emitted by the backend while a turn runs. */
export const events = {
  onText: (fn: (sessionId: string, delta: string) => void): Promise<UnlistenFn> =>
    listen<string[]>("chat:text", (e) => {
      const [sessionId, delta] = e.payload as string[];
      fn(sessionId, delta);
    }),
  onReasoning: (fn: (sessionId: string, delta: string) => void): Promise<UnlistenFn> =>
    listen<string[]>("chat:reasoning", (e) => {
      const [sessionId, delta] = e.payload as string[];
      fn(sessionId, delta);
    }),
  onToolStart: (
    fn: (sessionId: string, callId: string, name: string, args: string) => void,
  ): Promise<UnlistenFn> =>
    listen<string[]>("chat:tool-start", (e) => {
      const [sessionId, callId, name, args] = e.payload as string[];
      fn(sessionId, callId, name, args);
    }),
  onToolEnd: (
    fn: (sessionId: string, callId: string, outcome: ToolOutcome) => void,
  ): Promise<UnlistenFn> =>
    listen<[string, string, ToolOutcome]>("chat:tool-end", (e) => {
      const [sessionId, callId, outcome] = e.payload;
      fn(sessionId, callId, outcome);
    }),
  onApproval: (
    fn: (sessionId: string, request: ApprovalRequest) => void,
  ): Promise<UnlistenFn> =>
    listen<[string, ApprovalRequest]>("chat:approval", (e) => {
      const [sessionId, request] = e.payload;
      fn(sessionId, request);
    }),
  onStatus: (fn: (sessionId: string, text: string) => void): Promise<UnlistenFn> =>
    listen<string[]>("chat:status", (e) => {
      const [sessionId, text] = e.payload as string[];
      fn(sessionId, text);
    }),
  onError: (fn: (sessionId: string, message: string) => void): Promise<UnlistenFn> =>
    listen<string[]>("chat:error", (e) => {
      const [sessionId, message] = e.payload as string[];
      fn(sessionId, message);
    }),
  onDone: (fn: (sessionId: string, message: Message) => void): Promise<UnlistenFn> =>
    listen<[string, Message]>("chat:done", (e) => {
      const [sessionId, message] = e.payload;
      fn(sessionId, message);
    }),
  onAborted: (fn: (sessionId: string, message: Message) => void): Promise<UnlistenFn> =>
    listen<[string, Message]>("chat:aborted", (e) => {
      const [sessionId, message] = e.payload;
      fn(sessionId, message);
    }),
};

export type { Message, Mode, PermissionMode, Project, RunState, Session, Settings, TodoItem, ToolCall, ToolOutcome, Transcript };