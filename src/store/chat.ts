import { create } from "zustand";
import { api, BackendError, events, type ApprovalRequest, type ResolvedProvider } from "../lib/ipc";
import type { PanelTab } from "../components/panel";
import type {
  Attachment,
  Message,
  PermissionMode,
  Project,
  Session,
  Settings,
  TodoItem,
  ToolCall,
  ToolOutcome,
  Transcript,
} from "../lib/types";
import { DEFAULT_SETTINGS } from "./defaults";

export interface Toast {
  id: string;
  kind: "info" | "success" | "warn" | "error";
  text: string;
}

interface ChatState {
  ready: boolean;
  bootError: string | null;
  toasts: Toast[];

  settings: Settings;
  projects: Project[];
  sessions: Session[];
  providers: ResolvedProvider[];

  activeProjectId: string | null;
  activeSessionId: string | null;

  messages: Message[];
  toolCalls: ToolCall[];
  todos: TodoItem[];
  /** Live deltas for the streaming assistant turn, keyed by message id. */
  streaming: Record<string, { text: string; reasoning: string }>;

  running: Record<string, boolean>;
  approvals: Record<string, ApprovalRequest>;
  queue: string[];
  statusText: Record<string, string>;

  rightPanel: PanelTab | null;
  sidebarOpen: boolean;
  paletteOpen: boolean;
  settingsOpen: boolean;
  attachments: Attachment[];

  // ---------------------------------------------------------------- actions
  boot: () => Promise<void>;
  toast: (kind: Toast["kind"], text: string) => void;
  dismissToast: (id: string) => void;

  saveSettings: (patch: Partial<Settings>) => Promise<void>;
  refreshProviders: () => Promise<void>;

  selectProject: (id: string | null) => Promise<void>;
  createProject: (name: string, roots: string[]) => Promise<void>;
  updateProject: (p: Project) => Promise<void>;
  deleteProject: (id: string) => Promise<void>;

  openSession: (id: string) => Promise<void>;
  newSession: (projectId?: string | null) => Promise<void>;
  renameSession: (id: string, title: string) => Promise<void>;
  setSessionModel: (id: string, provider: string, model: string) => Promise<void>;
  setSessionMode: (id: string, mode: "plan" | "build") => Promise<void>;
  setSessionPermission: (id: string, mode: PermissionMode) => Promise<void>;
  setSessionEffort: (id: string, effort: string | null) => Promise<void>;
  deleteSession: (id: string) => Promise<void>;

  send: (text: string, images?: string[]) => Promise<void>;
  sendQueued: () => Promise<void>;
  removeQueued: (index: number) => void;
  rewindTo: (messageId: string, editedText: string) => Promise<void>;
  stopRun: () => Promise<void>;
  respondApproval: (allow: boolean, always?: boolean) => Promise<void>;

  setRightPanel: (p: ChatState["rightPanel"]) => void;
  toggleRightPanel: () => void;
  toggleSidebar: () => void;
  setPaletteOpen: (open: boolean) => void;
  setSettingsOpen: (open: boolean) => void;
}

let toastSeq = 0;
const uid = (p: string) => `${p}_${Date.now().toString(36)}${(toastSeq++).toString(36)}`;

function errText(e: unknown): string {
  if (e instanceof BackendError) return e.message;
  if (e instanceof Error) return e.message;
  return String(e);
}

export const useChat = create<ChatState>((set, get) => ({
  ready: false,
  bootError: null,
  toasts: [],

  settings: DEFAULT_SETTINGS,
  projects: [],
  sessions: [],
  providers: [],

  activeProjectId: null,
  activeSessionId: null,

  messages: [],
  toolCalls: [],
  todos: [],
  streaming: {},

  running: {},
  approvals: {},
  queue: [],
  statusText: {},

  rightPanel: "files",
  sidebarOpen: true,
  paletteOpen: false,
  settingsOpen: false,
  attachments: [],

  // ------------------------------------------------------------------- boot

  boot: async () => {
    try {
      const [settings, projects, providers] = await Promise.all([
        api.settingsGet(),
        api.projectsList(false),
        api.providersList(),
      ]);
      set({ settings, projects, providers, ready: true });

      // Open the most recent chat, or start one so the app is never empty.
      const sessions = await api.sessionsList(null, 200);
      set({ sessions });
      if (sessions.length > 0) {
        await get().openSession(sessions[0].id);
      } else {
        await get().newSession();
      }
      await subscribe(set, get);
    } catch (e) {
      set({ bootError: errText(e), ready: true });
    }
  },

  toast: (kind, text) => {
    const id = uid("toast");
    set((s) => ({ toasts: [...s.toasts, { id, kind, text }] }));
    setTimeout(() => get().dismissToast(id), kind === "error" ? 8000 : 3600);
  },
  dismissToast: (id) => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),

  // --------------------------------------------------------------- settings

  saveSettings: async (patch) => {
    const next = { ...get().settings, ...patch };
    // Optimistic: theme and toggles must feel instant.
    set({ settings: next });
    try {
      const saved = await api.settingsSave(next);
      set({ settings: saved });
    } catch (e) {
      set({ settings: get().settings });
      get().toast("error", errText(e));
    }
  },

  refreshProviders: async () => {
    try {
      set({ providers: await api.providersList() });
    } catch (e) {
      get().toast("error", errText(e));
    }
  },

  // --------------------------------------------------------------- projects

  selectProject: async (id) => {
    set({ activeProjectId: id });
    const sessions = await api.sessionsList(id, 200);
    set({ sessions });
    if (sessions.length > 0) {
      await get().openSession(sessions[0].id);
    } else {
      await get().newSession(id);
    }
  },

  createProject: async (name, roots) => {
    const now = Date.now();
    const project: Project = {
      id: "",
      name,
      roots,
      instructions: "",
      permissionMode: get().settings.defaultPermissionMode,
      accent: null,
      archived: false,
      createdAt: now,
      updatedAt: now,
    };
    const saved = await api.projectSave(project);
    set((s) => ({ projects: [saved, ...s.projects] }));
    await get().selectProject(saved.id);
    get().toast("success", `Project “${saved.name}” ready`);
  },

  updateProject: async (p) => {
    const saved = await api.projectSave({ ...p, updatedAt: Date.now() });
    set((s) => ({ projects: s.projects.map((x) => (x.id === saved.id ? saved : x)) }));
  },

  deleteProject: async (id) => {
    await api.projectDelete(id);
    const projects = get().projects.filter((p) => p.id !== id);
    set({ projects });
    if (get().activeProjectId === id) await get().selectProject(null);
  },

  // --------------------------------------------------------------- sessions

  openSession: async (id) => {
    set({ activeSessionId: id, messages: [], toolCalls: [], todos: [], queue: [] });
    try {
      const t: Transcript = await api.messagesList(id);
      set({ messages: t.messages, toolCalls: t.toolCalls, todos: t.todos });
      const sessions = await api.sessionsList(get().activeProjectId, 200);
      set({ sessions });
    } catch (e) {
      get().toast("error", errText(e));
    }
  },

newSession: async (projectId) => {
    const target = projectId === undefined ? get().activeProjectId : projectId;
    const current = get().sessions.find((s) => s.id === get().activeSessionId);

    // Reuse the current chat when nothing has happened in it yet.
    //
    // Without this, every press of "New chat" adds another empty row, and the
    // sidebar fills with identical blanks. An untouched chat holds no state
    // worth preserving, and keeping the one that is open means the model, mode
    // and permission the user just chose survive instead of resetting.
    const untouched =
      current !== undefined &&
      (current.projectId ?? null) === (target ?? null) &&
      current.title === "New chat" &&
      get().messages.length === 0 &&
      !get().running[current.id];
    if (untouched) return;

    const s = get().settings;
    const session = await api.sessionCreate(
      s.defaultProvider || "ollama",
      s.defaultModel,
      target,
    );
    set((st) => ({
      sessions: [session, ...st.sessions],
      activeSessionId: session.id,
      messages: [],
      toolCalls: [],
      todos: [],
      queue: [],
    }));
  },

  renameSession: async (id, title) => {
    const session = get().sessions.find((s) => s.id === id);
    if (!session) return;
    const updated = await api.sessionUpdate({ ...session, title });
    set((s) => ({ sessions: s.sessions.map((x) => (x.id === id ? updated : x)) }));
  },

  setSessionModel: async (id, provider, model) => {
    const session = get().sessions.find((s) => s.id === id);
    if (!session) return;
    const updated = await api.sessionUpdate({ ...session, provider, model });
    set((s) => ({ sessions: s.sessions.map((x) => (x.id === id ? updated : x)) }));
  },

  setSessionMode: async (id, mode) => {
    const session = get().sessions.find((s) => s.id === id);
    if (!session) return;
    const updated = await api.sessionUpdate({ ...session, mode });
    set((s) => ({ sessions: s.sessions.map((x) => (x.id === id ? updated : x)) }));
  },

setSessionPermission: async (id, permissionMode) => {
    const session = get().sessions.find((s) => s.id === id);
    if (!session) return;
    const updated = await api.sessionUpdate({ ...session, permissionMode });
    set((s) => ({ sessions: s.sessions.map((x) => (x.id === id ? updated : x)) }));
  },

  setSessionEffort: async (id, reasoningEffort) => {
    const session = get().sessions.find((s) => s.id === id);
    if (!session) return;
    // `null` means "let the provider decide", which is the default. An empty
    // string is normalised to that rather than sent as a blank effort.
    const next = reasoningEffort?.trim() ? reasoningEffort : null;
    if ((session.reasoningEffort ?? null) === next) return;
    const updated = await api.sessionUpdate({ ...session, reasoningEffort: next });
    set((s) => ({ sessions: s.sessions.map((x) => (x.id === id ? updated : x)) }));
  },

  deleteSession: async (id) => {
    await api.sessionDelete(id);
    const sessions = get().sessions.filter((s) => s.id !== id);
    set({ sessions });
    if (get().activeSessionId === id) {
      if (sessions.length > 0) await get().openSession(sessions[0].id);
      else await get().newSession();
    }
  },

  // ------------------------------------------------------------------ send

  send: async (text, images = []) => {
    const id = get().activeSessionId;
    if (!id || (!text.trim() && images.length === 0)) return;

    if (get().running[id]) {
      // Keep typing while the agent works: hold the message until the run ends.
      set((s) => ({ queue: [...s.queue, text] }));
      return;
    }

    set((s) => ({ running: { ...s.running, [id]: true }, statusText: { ...s.statusText, [id]: "" } }));
    try {
      // Snapshot the workspace before the agent can change it, so fork &
      // rewind has a tree to restore. A non-repository project is not an error.
      await api.checkpointCreate(id).catch(() => undefined);
      const ack = await api.sendMessage(id, text, images);
      set((s) => ({ messages: [...s.messages, ack.message] }));
    } catch (e) {
      set((s) => {
        const running = { ...s.running };
        delete running[id];
        return { running };
      });
      get().toast("error", errText(e));
    }
  },

  sendQueued: async () => {
    const q = [...get().queue];
    if (q.length === 0) return;
    set({ queue: [] });
    for (const text of q) {
      const id = get().activeSessionId;
      if (!id) break;
      try {
        const ack = await api.sendMessage(id, text);
        set((s) => ({ messages: [...s.messages, ack.message] }));
      } catch (e) {
        get().toast("error", errText(e));
        break;
      }
    }
  },

  removeQueued: (index) => set((s) => ({ queue: s.queue.filter((_, i) => i !== index) })),

  /**
   * Rewinds the conversation and the workspace to a message, then replays an
   * edited prompt from there. Losing later turns is the point, so this asks for
   * confirmation first.
   */
  rewindTo: async (messageId: string, editedText: string) => {
    const id = get().activeSessionId;
    if (!id) return;
    if (get().running[id]) {
      get().toast("warn", "Stop the run before rewinding");
      return;
    }
    try {
      const t = await api.sessionRewind(id, messageId);
      set({ messages: t.messages, toolCalls: t.toolCalls, todos: t.todos });
      if (editedText.trim() && editedText.trim() !== t.messages.at(-1)?.content.trim()) {
        await get().send(editedText);
      } else {
        get().toast("info", "Rewound. The workspace is back to that point.");
      }
    } catch (e) {
      get().toast("error", errText(e));
    }
  },

  stopRun: async () => {
    const id = get().activeSessionId;
    if (!id) return;
    try {
      await api.abortRun(id);
    } catch (e) {
      get().toast("warn", errText(e));
    }
  },

  respondApproval: async (allow, always = false) => {
    const id = get().activeSessionId;
    const req = id ? get().approvals[id] : undefined;
    if (!id || !req) return;
    set((s) => {
      const approvals = { ...s.approvals };
      delete approvals[id];
      return { approvals };
    });
    try {
      await api.approve(id, req.callId, allow, always, allow ? undefined : "the user declined");
    } catch (e) {
      get().toast("error", errText(e));
    }
  },

  setRightPanel: (p) => set({ rightPanel: p }),
  toggleRightPanel: () =>
    set((s) => ({ rightPanel: s.rightPanel === null ? "files" : null })),
  toggleSidebar: () => set((s) => ({ sidebarOpen: !s.sidebarOpen })),
  setPaletteOpen: (open) => set({ paletteOpen: open }),
  setSettingsOpen: (open) => set({ settingsOpen: open }),
}));

// ---------------------------------------------------------------- streaming

type Set = (partial: Partial<ChatState> | ((s: ChatState) => Partial<ChatState>)) => void;

/** Wires backend events into the store. Called once from `boot`. */
async function subscribe(set: Set, get: () => ChatState): Promise<void> {
  const activeId = () => get().activeSessionId;

  await events.onText((sessionId, delta) => {
    if (sessionId !== activeId()) return;
    set((s) => {
      const key = sessionId;
      const cur = s.streaming[key] ?? { text: "", reasoning: "" };
      return { streaming: { ...s.streaming, [key]: { ...cur, text: cur.text + delta } } };
    });
  });

  await events.onReasoning((sessionId, delta) => {
    if (sessionId !== activeId()) return;
    set((s) => {
      const cur = s.streaming[sessionId] ?? { text: "", reasoning: "" };
      return {
        streaming: { ...s.streaming, [sessionId]: { ...cur, reasoning: cur.reasoning + delta } },
      };
    });
  });

  await events.onToolStart((sessionId, callId, name, args) => {
    if (sessionId !== activeId()) return;
    const call: ToolCall = {
      id: callId,
      sessionId,
      messageId: "",
      name,
      input: args,
      output: null,
      state: "running",
      deniedReason: null,
      diff: null,
      startedAt: Date.now(),
      endedAt: null,
    };
    set((s) => ({ toolCalls: [...s.toolCalls, call] }));
  });

  await events.onToolEnd((sessionId, callId, outcome: ToolOutcome) => {
    if (sessionId !== activeId()) return;
    set((s) => ({
      toolCalls: s.toolCalls.map((c) =>
        c.id === callId
          ? {
              ...c,
              output: outcome.content,
              diff: outcome.diff ?? null,
              state: outcome.isError ? "error" : "done",
              endedAt: Date.now(),
            }
          : c,
      ),
    }));
    // The todo widget is driven by the tool payload rather than a second event.
    if (outcome.display?.kind === "todoList") {
      set({ todos: outcome.display.items as TodoItem[] });
    }
  });

  await events.onApproval((sessionId, request) => {
    if (sessionId !== activeId()) return;
    set((s) => ({
      approvals: { ...s.approvals, [sessionId]: request },
      running: { ...s.running, [sessionId]: true },
    }));
  });

  await events.onStatus((sessionId, text) => {
    set((s) => ({ statusText: { ...s.statusText, [sessionId]: text } }));
  });

  await events.onError((sessionId, message) => {
    set((s) => {
      const running = { ...s.running };
      delete running[sessionId];
      return { running };
    });
    get().toast("error", message);
  });

  await events.onDone((sessionId, message) => {
    set((s) => {
      const streaming = { ...s.streaming };
      delete streaming[sessionId];
      const running = { ...s.running };
      delete running[sessionId];
      // Replace any optimistic copy of this turn, otherwise append it.
      const withoutTemp = s.messages.filter((m) => m.id !== message.id);
      return {
        streaming,
        running,
        messages: [...withoutTemp, message],
      };
    });
    void flushQueue(set, get, sessionId);
    void refreshSessions(set, get, sessionId);
  });

  await events.onAborted((sessionId, message) => {
    set((s) => {
      const streaming = { ...s.streaming };
      delete streaming[sessionId];
      const running = { ...s.running };
      delete running[sessionId];
      return { streaming, running };
    });
    if (message.content.trim()) {
      set((s) => ({ messages: [...s.messages, message] }));
    }
  });
}

/** Sends anything the user typed while the previous turn was running. */
async function flushQueue(set: Set, get: () => ChatState, sessionId: string): Promise<void> {
  const queue = get().queue;
  if (queue.length === 0 || get().activeSessionId !== sessionId) return;
  set({ queue: [] });
  for (const text of queue) {
    try {
      const ack = await api.sendMessage(sessionId, text);
      set((s) => ({ messages: [...s.messages, ack.message] }));
    } catch (e) {
      get().toast("error", errText(e));
      return;
    }
  }
}

async function refreshSessions(set: Set, get: () => ChatState, sessionId: string): Promise<void> {
  const session = await api.sessionGet(sessionId).catch(() => null);
  if (!session) return;
  set((s) => ({
    sessions: s.sessions.map((x) => (x.id === sessionId ? session : x)),
  }));
  void get;
}