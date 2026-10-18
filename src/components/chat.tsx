import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";

import {
  AlignLeft,
  AlignRight,
  ArrowUp,
  Brain,
  Check,
  ChevronDown,
  ChevronRight,
  CircleSlash,
  FileCode2,
  FolderPlus,
  FolderSearch,
  ImagePlus,
  Mic,
  Monitor,
  Paperclip,
  Pencil,
  Plus,
  RefreshCw,
  Search,
  SlidersHorizontal,
  Sparkles,
  Square,
  Terminal,
  Type,
  X,
} from "lucide-react";
import { Markdown } from "./markdown";
import { Collapsible, CopyButton, DiffView } from "./diff";
import { Badge, Button, ImageLightbox, Popover, cx } from "./ui";
import { useChat } from "../store/chat";
import { api } from "../lib/ipc";
import type {
  Attachment,
  Message,
  PermissionMode,
  ToolCall,
  TodoItem,
} from "../lib/types";
import { PERMISSION_HINTS, PERMISSION_LABELS } from "../lib/types";

const IMAGE_EXTENSIONS = ["png", "jpg", "jpeg", "gif", "webp", "bmp"];

function imageMime(path: string): string {
  const ext = (path.split(".").pop() ?? "").toLowerCase();
  return ext === "jpg" ? "jpeg" : ext || "png";
}

/** Converts a Windows path to the form the asset protocol expects. */
function assetUrl(path: string): string {
  return `asset://localhost/${path.replace(/\\/g, "/").replace(/^\/+/, "")}`;
}

/** Reads a file as base64 through the Rust side, which is the only reader with
 *  filesystem reach inside a Tauri webview. */
async function readFile(path: string): Promise<string> {
  return invoke<string>("panel_read_base64", { path });
}

/**
 * Shared full-size image viewer.
 *
 * Both the transcript and the composer open the same overlay, so a pending
 * attachment can be checked before it is sent and a sent one afterwards without
 * two independent viewers drifting apart.
 */
export function useImageViewer() {
  const [images, setImages] = useState<{ id: string; src: string; label: string }[]>([]);
  const [index, setIndex] = useState(0);

  const open = useCallback((list: { id: string; src: string; label: string }[], at: number) => {
    setImages(list);
    setIndex(at);
  }, []);
  const close = useCallback(() => {
    setImages([]);
  }, []);

  const viewer = images.length > 0 ? (
    <ImageLightbox images={images} index={index} onIndex={setIndex} onClose={close} />
  ) : null;

  return { open, close, viewer };
}

export function ChatView() {
  const messages = useChat((s) => s.messages);
  const streaming = useChat((s) => s.streaming);
  const toolCalls = useChat((s) => s.toolCalls);
  const todos = useChat((s) => s.todos);
  const approvals = useChat((s) => s.approvals);
  const activeId = useChat((s) => s.activeSessionId);
  const showReasoning = useChat((s) => s.settings.showReasoning);
  const [pending, setPending] = useState(false);

  const scrollRef = useRef<HTMLDivElement>(null);
  const pinned = useRef(true);
  const live = streaming[activeId ?? ""];

  // The composer sits in the middle of the pane on an empty chat and drops to
  // the bottom once there is a conversation, matching Codex and opencode.
  const empty = messages.length === 0 && !live && todos.length === 0;

  useEffect(() => {
    const el = scrollRef.current;
    if (el && pinned.current) el.scrollTop = el.scrollHeight;
  }, [messages, live?.text, toolCalls.length]);

  const onScroll = () => {
    const el = scrollRef.current;
    if (!el) return;
    pinned.current = el.scrollHeight - el.scrollTop - el.clientHeight < 120;
  };

  const toolsByMessage = useMemo(() => groupTools(toolCalls), [toolCalls]);
  const viewer = useImageViewer();

return (
    <div className="oc-chat" data-empty={empty}>
      <div
        className={cx("oc-transcript", "oc-scroll")}
        ref={scrollRef}
        onScroll={onScroll}
      >
        <div className="oc-thread">
{messages.map((m) => (
            <MessageRow
              key={m.id}
              message={m}
              tools={toolsByMessage.get(m.id) ?? []}
              showReasoning={showReasoning}
              onOpenImage={viewer.open}
            />
          ))}

          {live ? (
            <div className="oc-msg" data-role="assistant">
              <div className="oc-msg-avatar">OC</div>
              <div className="oc-msg-body">
                {showReasoning && live.reasoning ? (
                  <Collapsible
                    label={`Reasoning · ${live.reasoning.length} chars`}
                    icon={<Brain size={13} />}
                  >
                    <div className="oc-tool-body">{live.reasoning}</div>
                  </Collapsible>
                ) : null}
                {live.text ? (
                  <Markdown text={live.text} />
                ) : (
                  <div className="oc-row oc-muted" style={{ gap: 8, fontSize: "0.9em" }}>
                    <span className="oc-spinner" /> thinking
                  </div>
                )}
                <span className="oc-caret" />
              </div>
            </div>
          ) : null}

          {todos.length > 0 ? <TodoCard todos={todos} /> : null}
        </div>
      </div>

<div className="oc-composer-dock">
        {activeId && approvals[activeId] ? (
          <ApprovalCard request={approvals[activeId]!} />
        ) : null}

        {/* On an empty chat the dock becomes the centre stage: context selector
            above, composer in the middle, suggestions below. The composer is
            never unmounted, so typing survives the first send. */}
        {empty ? <StageHeader /> : null}

        <Composer onPendingChange={setPending} busy={pending} onOpenImage={viewer.open} />

        {empty ? <StageFooter /> : null}
      </div>

      {viewer.viewer}
    </div>
  );
}

/** Project and machine context shown above the composer on an empty chat. */
function StageHeader() {
  const projects = useChat((s) => s.projects);
  const activeProjectId = useChat((s) => s.activeProjectId);
  const setSettingsOpen = useChat((s) => s.setSettingsOpen);
  const active = projects.find((p) => p.id === activeProjectId) ?? null;

  return (
    <div className="oc-stage-head">
      <button
        className="oc-stage-chip"
        onClick={() => setSettingsOpen(true)}
        title={active?.roots[0] ?? "No project open — add one in Settings"}
      >
        <FolderPlus size={13} />
        <span className="oc-truncate">{active ? active.name : "No project"}</span>
        <ChevronDown size={12} className="oc-faint" />
      </button>
      <span className="oc-stage-sep" />
      <span className="oc-stage-chip" data-static="true">
        <Monitor size={13} />
        This PC
      </span>
    </div>
  );
}

/** Starter prompts shown under the composer on an empty chat. */
function StageFooter() {
  const send = useChat((s) => s.send);
  const sessionId = useChat((s) => s.activeSessionId);
  const activeProjectId = useChat((s) => s.activeProjectId);

  const starter = (text: string) => () => {
    if (!sessionId) return;
    void send(text, []);
  };

  return (
    <div className="oc-stage-foot">
      <button
        className="oc-stage-pill"
        onClick={starter("Read this project and summarise what it does, then suggest one improvement.")}
      >
        Explain this project
      </button>
      <button
        className="oc-stage-pill"
        onClick={starter("Find the bug in the most recently changed file and fix it, running the tests first.")}
      >
        Fix a bug
      </button>
      {!activeProjectId ? (
        <button
          className="oc-stage-pill"
          onClick={() => void useChat.getState().newSession(null)}
        >
          Choose a folder to start
        </button>
      ) : null}
    </div>
  );
}

/** Indexes tool calls by the assistant message that requested them. */
function groupTools(calls: ToolCall[]): Map<string, ToolCall[]> {
  const out = new Map<string, ToolCall[]>();
  for (const c of calls) {
    if (!c.messageId) continue;
    const list = out.get(c.messageId);
    if (list) list.push(c);
    else out.set(c.messageId, [c]);
  }
  return out;
}

// -------------------------------------------------------------------- message

function MessageRow({
  message,
  tools,
  showReasoning,
  onOpenImage,
}: {
  message: Message;
  tools: ToolCall[];
  showReasoning: boolean;
  onOpenImage: (list: { id: string; src: string; label: string }[], at: number) => void;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(message.content);
  const [confirming, setConfirming] = useState(false);
  const rewindTo = useChat((s) => s.rewindTo);
  const isLast = useChat((s) => (s.messages.at(-1)?.id ?? null) === message.id);
  const isUser = message.role === "user";

  return (
    <div className="oc-msg" data-role={message.role}>
      <div className="oc-msg-avatar">{isUser ? "YOU" : "OC"}</div>
      <div className="oc-msg-body">
        <div className="oc-msg-meta">
          <span>{isUser ? "You" : "Octopus"}</span>
          {message.model ? <span className="oc-mono">{message.model}</span> : null}
          {message.state === "aborted" ? <Badge tone="warn">stopped</Badge> : null}
          {message.error ? <Badge tone="danger">error</Badge> : null}
          <div className="oc-msg-actions">
            <CopyButton text={message.content} label="Copy message" />
            {isUser && !isLast ? (
              <Button
                size="icon"
                variant="ghost"
                title="Edit and re-run from here"
                onClick={() => {
                  setDraft(message.content);
                  setEditing(true);
                }}
              >
                <Pencil size={13} />
              </Button>
            ) : null}
          </div>
        </div>

        {editing ? (
          <EditPrompt
            initial={draft}
            onChange={setDraft}
            onCancel={() => {
              setEditing(false);
              setConfirming(false);
            }}
            confirming={confirming}
            onSubmit={() => {
              if (confirming) {
                void rewindTo(message.id, draft);
                setEditing(false);
                setConfirming(false);
                return;
              }
              setConfirming(true);
            }}
          />
        ) : isUser ? (
          <div className="oc-user-text" dir="auto">{message.content}</div>
        ) : (
          <>
            {showReasoning && message.reasoning ? (
              <Collapsible label="Reasoning" icon={<Brain size={13} />}>
                <div className="oc-tool-body">{message.reasoning}</div>
              </Collapsible>
            ) : null}
            <Markdown text={message.content} />
          </>
        )}

{message.attachments.length > 0 ? (
          <div className="oc-attachments">
            {message.attachments.map((a, i) =>
              a.preview ? (
                <button
                  key={a.id}
                  className="oc-attachment"
                  title={`${a.path} — click to enlarge`}
                  onClick={() =>
                    onOpenImage(
                      message.attachments
                        .filter((x) => x.preview)
                        .map((x) => ({ id: x.id, src: x.preview!, label: x.path })),
                      message.attachments.slice(0, i).filter((x) => x.preview).length,
                    )
                  }
                >
                  <img src={a.preview} alt={a.path} />
                </button>
              ) : (
                <Badge key={a.id}>{a.path}</Badge>
              ),
            )}
          </div>
        ) : null}

        {tools.length > 0 ? (
          <div className="oc-tools">
            {tools.map((t) => (
              <ToolCard key={t.id} call={t} />
            ))}
          </div>
        ) : null}
      </div>
    </div>
  );
}

function EditPrompt({
  initial,
  onChange,
  onCancel,
  onSubmit,
  confirming,
}: {
  initial: string;
  onChange: (v: string) => void;
  onCancel: () => void;
  onSubmit: () => void;
  confirming: boolean;
}) {
  return (
    <div className="oc-col" style={{ gap: 8 }}>
      <textarea
        className="oc-textarea"
        value={initial}
        onChange={(e) => onChange(e.target.value)}
        autoFocus
      />
      {confirming ? (
        <div className="oc-approval" style={{ margin: 0 }}>
          <div className="oc-approval-head">
            <RefreshCw size={14} />
            Rewind this conversation?
          </div>
          <div style={{ fontSize: "0.9em", marginBottom: 9 }}>
            Every message after this one is deleted, and in a git project the working tree is
            restored to how it was here. Files changed since then are removed.
          </div>
          <div className="oc-approval-actions">
            <Button variant="primary" size="sm" onClick={onSubmit}>
              <RefreshCw size={12} /> Yes, rewind and re-run
            </Button>
            <Button size="sm" variant="ghost" onClick={onCancel}>
              Keep everything
            </Button>
          </div>
        </div>
      ) : (
        <div className="oc-row" style={{ gap: 7 }}>
          <Button variant="primary" size="sm" onClick={onSubmit}>
            <RefreshCw size={12} /> Re-run from here
          </Button>
          <Button size="sm" variant="ghost" onClick={onCancel}>
            Cancel
          </Button>
        </div>
      )}
    </div>
  );
}

// ----------------------------------------------------------------- tool card

const TOOL_ICONS: Record<string, React.ReactNode> = {
  read: <FileCode2 size={13} />,
  write: <Pencil size={13} />,
  edit: <Pencil size={13} />,
  delete: <CircleSlash size={13} />,
  glob: <FolderSearch size={13} />,
  grep: <Search size={13} />,
  list: <FolderSearch size={13} />,
  bash: <Terminal size={13} />,
  todo_write: <Check size={13} />,
  todo_read: <Check size={13} />,
};

function ToolCard({ call }: { call: ToolCall }) {
  return (
    <Collapsible
      label={summarize(call)}
      icon={TOOL_ICONS[call.name] ?? <Terminal size={13} />}
      tone={call.state === "error" ? "error" : call.state === "denied" ? "denied" : undefined}
      defaultOpen={call.state === "error"}
    >
      {call.diff ? <DiffView diff={call.diff} /> : null}
      {call.output ? (
        <div className="oc-tool-body">
          {call.output.length > 6000 ? `${call.output.slice(0, 6000)}\n…` : call.output}
        </div>
      ) : null}
    </Collapsible>
  );
}

/** Turns raw tool arguments into one readable line. */
function summarize(call: ToolCall): string {
  let args: Record<string, unknown> = {};
  try {
    args = JSON.parse(call.input) as Record<string, unknown>;
  } catch {
    return call.name;
  }
  const pick = (k: string) => {
    const v = args[k];
    return typeof v === "string" ? v : undefined;
  };
  switch (call.name) {
    case "read":
      return `Read ${pick("path") ?? "?"}`;
    case "write":
      return `Write ${pick("path") ?? "?"}`;
    case "edit":
      return `Edit ${pick("path") ?? "?"}`;
    case "delete":
      return `Delete ${pick("path") ?? "?"}`;
    case "glob":
      return `Find ${pick("pattern") ?? "?"}`;
    case "grep":
      return `Search /${pick("pattern") ?? "?"}/`;
    case "list":
      return `List ${pick("path") ?? "."}`;
    case "bash":
      return pick("command") ?? "Run a command";
    case "todo_write":
      return "Update the task list";
    default:
      return call.name;
  }
}

// ---------------------------------------------------------------- todo card

function TodoCard({ todos }: { todos: TodoItem[] }) {
  const done = todos.filter((t) => t.status === "done").length;
  return (
    <div className="oc-todos">
      <div className="oc-row" style={{ marginBottom: 6, gap: 8 }}>
        <strong style={{ fontSize: "0.88em" }}>Plan</strong>
        <span className="oc-faint" style={{ fontSize: "0.82em" }}>
          {done}/{todos.length}
        </span>
        <div className="oc-spacer" />
        <div
          style={{
            width: 90,
            height: 4,
            borderRadius: 99,
            background: "var(--oc-bg-active)",
            overflow: "hidden",
          }}
        >
          <div
            style={{
              width: `${(done / Math.max(1, todos.length)) * 100}%`,
              height: "100%",
              background: "var(--oc-success)",
              transition: "width 200ms ease",
            }}
          />
        </div>
      </div>
      {todos.map((t) => (
        <div className="oc-todo" data-status={t.status} key={t.id}>
          <span className="oc-todo-mark">
            {t.status === "done" ? <Check size={9} strokeWidth={4} /> : null}
          </span>
          <span>{t.content}</span>
        </div>
      ))}
    </div>
  );
}

// ------------------------------------------------------------------ approval

function ApprovalCard({ request }: { request: { tool: string; summary: string; arguments: string } }) {
  const respond = useChat((s) => s.respondApproval);
  return (
    <div className="oc-approval">
      <div className="oc-approval-head">
        <ChevronRight size={14} />
        Approval needed
      </div>
      <div style={{ marginBottom: 8, fontSize: "0.92em" }}>{request.summary}</div>
      <div className="oc-approval-args">{request.arguments}</div>
      <div className="oc-approval-actions">
        <Button variant="primary" size="sm" onClick={() => respond(true, false)}>
          <Check size={12} /> Allow once
        </Button>
        <Button size="sm" onClick={() => respond(true, true)}>
          Always allow {request.tool}
        </Button>
        <Button size="sm" variant="danger" onClick={() => respond(false)}>
          <X size={12} /> Deny
        </Button>
      </div>
    </div>
  );
}

// ------------------------------------------------------------------ composer

function Composer({
  onPendingChange,
  busy,
  onOpenImage,
}: {
  onPendingChange: (v: boolean) => void;
  busy: boolean;
  onOpenImage: (list: { id: string; src: string; label: string }[], at: number) => void;
}) {
  const [text, setText] = useState("");
  const [images, setImages] = useState<Attachment[]>([]);
  const [menu, setMenu] = useState<null | "attach" | "mode" | "model" | "effort">(null);
  const [dir, setDir] = useState<"auto" | "ltr" | "rtl">("auto");

  const sendOnEnter = useChat((s) => s.settings.sendOnEnter);
  const sessionId = useChat((s) => s.activeSessionId);
  const running = useChat((s) => (sessionId ? s.running[sessionId] : false));
  const queue = useChat((s) => s.queue);
  const send = useChat((s) => s.send);
  const stop = useChat((s) => s.stopRun);
  const removeQueued = useChat((s) => s.removeQueued);
  const sendQueued = useChat((s) => s.sendQueued);
  const setSettingsOpen = useChat((s) => s.setSettingsOpen);

  const sessions = useChat((s) => s.sessions);
  const session = sessions.find((s) => s.id === sessionId) ?? null;
const setSessionMode = useChat((s) => s.setSessionMode);
  const setSessionModel = useChat((s) => s.setSessionModel);
  const setSessionEffort = useChat((s) => s.setSessionEffort);
  const providers = useChat((s) => s.providers);

  const ref = useRef<HTMLTextAreaElement>(null);
  const fileRef = useRef<HTMLInputElement>(null);
  // The toolbar is what the popovers point at, so it is the anchor they measure
  // against rather than the composer as a whole.
  const barRef = useRef<HTMLDivElement>(null);

  const autosize = useCallback(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(280, el.scrollHeight)}px`;
  }, []);
  useEffect(autosize, [text, autosize]);

  // Focus follows a new chat so the user can type immediately.
  useEffect(() => {
    if (sessionId) ref.current?.focus();
  }, [sessionId]);

  const attachPaths = useCallback(async (paths: string[]) => {
    const imagesOnly = paths.filter((p) =>
      IMAGE_EXTENSIONS.includes(p.split(".").pop()?.toLowerCase() ?? ""),
    );
    if (imagesOnly.length === 0) return;
    // The path is sent to the backend, which reads and inlines the real file.
    // The data url here is only for the thumbnail shown before sending.
    const added: Attachment[] = imagesOnly.map((p) => ({
      id: `${Date.now()}${Math.random().toString(36).slice(2, 7)}`,
      path: p,
      mime: `image/${imageMime(p)}`,
      bytes: 0,
      preview: assetUrl(p),
    }));
    setImages((prev) => [...prev, ...added]);
    hydrateThumbnails(imagesOnly, added);
  }, []);

  // Reads each picked file in the webview so the thumbnail appears at once.
  const hydrateThumbnails = useCallback(async (paths: string[], added: Attachment[]) => {
    await Promise.all(
      added.map(async (a, i) => {
        try {
          const bytes = await readFile(paths[i]);
          const mime = a.mime;
          const dataUrl = `${mime};base64,${bytes}`;
          setImages((prev) =>
            prev.map((img) =>
              img.id === a.id
                ? { ...img, preview: `data:${dataUrl}`, bytes: Math.floor((dataUrl.length * 3) / 4) }
                : img,
            ),
          );
        } catch {
          // The path still reaches the backend, so sending works even when the
          // preview cannot be rendered.
        }
      }),
    );
  }, []);

  const attachDataUrl = useCallback((dataUrl: string) => {
    setImages((prev) => [
      ...prev,
      {
        id: `${Date.now()}${Math.random().toString(36).slice(2, 7)}`,
        // A pasted image has no path, so the data url is sent as-is.
        path: dataUrl,
        mime: dataUrl.slice(5, dataUrl.indexOf(";")),
        bytes: Math.floor((dataUrl.length * 3) / 4),
        preview: dataUrl,
      },
    ]);
  }, []);

  // Tauri intercepts drops at the OS level, so the webview's HTML5 drop event
  // never fires; the platform event is the one to listen to.
  useEffect(() => {
    let dispose: (() => void) | undefined;
    let cancelled = false;
    void getCurrentWebview()
      .onDragDropEvent((event) => {
        if (event.payload.type === "drop") {
          void attachPaths(event.payload.paths);
        }
      })
      .then((un) => {
        if (cancelled) un();
        else dispose = un;
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
      dispose?.();
    };
  }, [attachPaths]);

  const onPaste = useCallback(
    (e: React.ClipboardEvent<HTMLTextAreaElement>) => {
      const files = Array.from(e.clipboardData?.files ?? []);
      if (files.length === 0) return;
      e.preventDefault();
      files.forEach((file) => {
        const reader = new FileReader();
        reader.onload = () => {
          if (typeof reader.result === "string") attachDataUrl(reader.result);
        };
        reader.readAsDataURL(file);
      });
    },
    [attachDataUrl],
  );

  const submit = () => {
    const value = text.trim();
    if ((!value && images.length === 0) || !sessionId) return;
    setText("");
    setImages([]);
    requestAnimationFrame(autosize);
    void send(value, images.map((i) => i.path));
  };

  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key !== "Enter") return;
    const wantsNewline = e.shiftKey;
    if (sendOnEnter ? !wantsNewline : wantsNewline) {
      e.preventDefault();
      submit();
    }
  };

  const provider = providers.find((p) => p.id === session?.provider);

  // Effort only applies to a model that advertises reasoning, so the control is
  // hidden rather than shown-and-ignored.
  const reasoningModel = Boolean(
    provider?.models.find((m) => m.id === session?.model)?.reasoning,
  );

  const cycleDir = () => setDir((d) => (d === "auto" ? "ltr" : d === "ltr" ? "rtl" : "auto"));

  return (
    <div className="oc-composer-inner">
      {queue.length > 0 ? (
        <div className="oc-queued">
          <div className="oc-row" style={{ marginBottom: 4, gap: 8 }}>
            <strong style={{ fontSize: "0.92em" }}>Queued</strong>
            <span className="oc-faint">{queue.length} waiting</span>
            <div className="oc-spacer" />
            <Button size="sm" variant="primary" onClick={() => void sendQueued()}>
              Send now
            </Button>
          </div>
          {queue.map((q, i) => (
            <div className="oc-queued-item" key={i} dir="auto">
              <span className="oc-truncate">{q}</span>
              <div className="oc-spacer" />
              <Button size="icon" variant="ghost" title="Remove" onClick={() => removeQueued(i)}>
                <X size={12} />
              </Button>
            </div>
          ))}
        </div>
      ) : null}

<div className={cx("oc-composer", busy && "oc-composer-busy")}>
        {/* Inside the bordered box on purpose: a pending image is part of what
            is being composed, so it belongs within the composer rather than
            floating above it as a separate panel. */}
        {images.length > 0 ? (
          <div className="oc-attach-strip">
            {images.map((img, i) => (
              <div className="oc-attach-chip" key={img.id}>
                <button
                  className="oc-attach-open"
                  title="Click to enlarge"
                  onClick={() =>
                    onOpenImage(
                      images
                        .filter((x) => x.preview)
                        .map((x) => ({ id: x.id, src: x.preview!, label: x.path })),
                      i,
                    )
                  }
                >
                  <img src={img.preview ?? ""} alt="" />
                </button>
                <button
                  onClick={() => setImages((prev) => prev.filter((x) => x.id !== img.id))}
                  aria-label="Remove image"
                >
                  <X size={11} />
                </button>
              </div>
            ))}
          </div>
        ) : null}

        <textarea
          ref={ref}
          className="oc-composer-input"
          placeholder="Ask anything, / for commands, @ for context…"
          value={text}
          dir={dir}
          onChange={(e) => {
            setText(e.target.value);
            autosize();
            onPendingChange(true);
          }}
          onKeyDown={onKeyDown}
          onPaste={onPaste}
          rows={1}
        />


        <div className="oc-composer-bar" ref={barRef}>
          <input
            ref={fileRef}
            type="file"
            accept="image/*"
            multiple
            hidden
            onChange={(e) => {
              // Picked files have a real path, so they go to the backend by
              // path; pasted images have none and travel as data urls.
              const files = Array.from(e.target.files ?? []);
              const withPath = files.filter((f) => Boolean((f as File & { path?: string }).path));
              const paths = withPath.map((f) => (f as File & { path?: string }).path!);
              if (paths.length > 0) void attachPaths(paths);
              files
                .filter((f) => !(f as File & { path?: string }).path)
                .forEach((file) => {
                  const reader = new FileReader();
                  reader.onload = () => {
                    if (typeof reader.result === "string") attachDataUrl(reader.result);
                  };
                  reader.readAsDataURL(file);
                });
              e.target.value = "";
            }}
          />

          {/* The `+` adds context: images from disk, or a file to mention. */}
          <button
            className="oc-composer-btn"
            title="Add context"
            onClick={() => setMenu(menu === "attach" ? null : "attach")}
          >
            <Plus size={16} />
          </button>

          {session ? (
            <>
              <button
                className="oc-composer-select"
                onClick={() => setMenu(menu === "mode" ? null : "mode")}
              >
                {session.mode === "plan" ? "Plan" : "Build"}
                <ChevronDown size={12} />
              </button>

              <button
                className="oc-composer-select"
                onClick={() => setMenu(menu === "model" ? null : "model")}
                title={provider?.label ?? "Choose a model"}
              >
                <span className="oc-truncate" style={{ maxWidth: 190 }}>
                  {session.model || "Pick a model"}
                </span>
                <ChevronDown size={12} />
              </button>

              <button
                className="oc-composer-select"
                onClick={() => setSettingsOpen(true)}
                title={`${PERMISSION_LABELS[session.permissionMode]} — ${
                  PERMISSION_HINTS[session.permissionMode]
                } (click to change in Settings)`}
              >
{shortPermission(session.permissionMode)}
                <ChevronDown size={12} />
              </button>

              {/* Always shown rather than hidden, so the control is
                  discoverable. A model that does not report reasoning support
                  gets a disabled chip that says why, which beats an absent
                  control that looks like an oversight. */}
              <button
                className="oc-composer-select"
                data-unsupported={!reasoningModel}
                onClick={() => setMenu(menu === "effort" ? null : "effort")}
                disabled={!reasoningModel}
                title={
                  reasoningModel
                    ? "How much the model should think before answering. Higher is slower and costlier."
                    : `${session.model || "This model"} does not report reasoning support, so effort has no effect.`
                }
              >
                {reasoningModel ? effortLabel(session.reasoningEffort) : "No reasoning"}
                <ChevronDown size={12} />
              </button>
            </>
          ) : null}

          <button
            className="oc-composer-btn"
            data-active={dir !== "auto"}
            onClick={cycleDir}
            title={`Text direction: ${dir}. Click to change.`}
            aria-label={`Text direction: ${dir}`}
          >
            {dir === "rtl" ? <AlignRight size={15} /> : dir === "ltr" ? <AlignLeft size={15} /> : <Type size={15} />}
          </button>

<div className="oc-spacer" />

          {running ? (
            <Button size="icon" variant="danger" title="Stop" onClick={() => void stop()}>
              <Square size={12} />
            </Button>
          ) : (
            <>
              <MicButton
                onTranscript={(t) =>
                  setText((prev) => (prev && !prev.endsWith(" ") ? `${prev} ${t}` : prev + t))
                }
              />
              <button
                className="oc-send"
                disabled={!text.trim() && images.length === 0}
                onClick={submit}
                title="Send"
              >
                <ArrowUp size={16} />
              </button>
            </>
          )}
        </div>
      </div>

      {menu === "attach" ? (
        <AttachMenu
          anchor={barRef.current}
          onClose={() => setMenu(null)}
          onPickImages={() => {
            setMenu(null);
            fileRef.current?.click();
          }}
          onSettings={() => {
            setMenu(null);
            setSettingsOpen(true);
          }}
        />
      ) : null}

      {menu === "mode" && session ? (
        <PickerMenu
          anchor={barRef.current}
          onClose={() => setMenu(null)}
          title="Mode"
          items={[
            {
              label: "Build",
              hint: "May edit files and run commands",
              active: session.mode === "build",
              onSelect: () => void setSessionMode(session.id, "build"),
            },
            {
              label: "Plan",
              hint: "Read-only; produces a plan",
              active: session.mode === "plan",
              onSelect: () => void setSessionMode(session.id, "plan"),
            },
          ]}
        />
      ) : null}

      {menu === "effort" && session ? (
        <PickerMenu
          anchor={barRef.current}
          onClose={() => setMenu(null)}
          title="Reasoning effort"
          items={EFFORT_CHOICES.map((e) => ({
            label: e.label,
            hint: e.hint,
            active: (session.reasoningEffort ?? "") === e.value,
            onSelect: () => void setSessionEffort(session.id, e.value),
          }))}
        />
      ) : null}

      {menu === "model" && session ? (
        <ModelPicker
          anchor={barRef.current}
          onClose={() => setMenu(null)}
          onPick={(providerId, modelId) => {
            void setSessionModel(session.id, providerId, modelId);
            setMenu(null);
          }}
          onManage={() => {
            setMenu(null);
            setSettingsOpen(true);
          }}
        />
      ) : null}
    </div>
  );
}

/** Compresses the permission label so it fits the composer toolbar. */
function shortPermission(mode: PermissionMode | undefined): string {
  switch (mode) {
    case "auto_review":
      return "Auto";
    case "full_access":
      return "Full";
    default:
      return "Ask";
  }
}

/**
 * Reasoning-effort options.
 *
 * These are the values the OpenAI-compatible and Gemini dialects accept; an
 * endpoint that does not know one of them rejects the request rather than
 * ignoring it, so the default "Auto" sends nothing and lets the provider pick.
 */
const EFFORT_CHOICES: { value: string; label: string; hint: string }[] = [
  { value: "", label: "Auto", hint: "Let the model decide" },
  { value: "low", label: "Low", hint: "Quickest, cheapest" },
  { value: "medium", label: "Medium", hint: "Balanced" },
  { value: "high", label: "High", hint: "Slowest, most thorough" },
];

function effortLabel(value: string | null | undefined): string {
  const found = EFFORT_CHOICES.find((e) => e.value === (value ?? ""));
  return found && found.value ? found.label : "Auto";
}

/**
 * Dictation button.
 *
 * Uses the Web Speech API exposed by the WebView2 runtime. That API is not
 * present in every runtime build, and it streams audio to the platform speech
 * service, so this reports clearly instead of failing silently: an unsupported
 * runtime says so rather than showing a button that does nothing.
 */
function MicButton({ onTranscript }: { onTranscript: (text: string) => void }) {
  const [listening, setListening] = useState(false);
  const supported = useMemo(
    () =>
      typeof window !== "undefined" &&
      ("SpeechRecognition" in window || "webkitSpeechRecognition" in window),
    [],
  );
  const recognition = useRef<SpeechRecognitionLike | null>(null);

  useEffect(() => {
    return () => {
      try {
        recognition.current?.stop();
      } catch {
        // Already stopped; nothing to clean up.
      }
    };
  }, []);

  const toggle = () => {
    if (listening) {
      try {
        recognition.current?.stop();
      } catch {
        // Ignore: the end event below still clears the indicator.
      }
      setListening(false);
      return;
    }

    const Ctor =
      (window as unknown as { SpeechRecognition?: SpeechCtor }).SpeechRecognition ??
      (window as unknown as { webkitSpeechRecognition?: SpeechCtor }).webkitSpeechRecognition;
    if (!Ctor) return;

    const rec = new Ctor();
    rec.lang = navigator.language || "en-US";
    rec.interimResults = false;
    rec.continuous = false;
    rec.onresult = (e: SpeechEventLike) => {
      const said = Array.from(e.results ?? [])
        .map((r) => r?.[0]?.transcript ?? "")
        .join("")
        .trim();
      if (said) onTranscript(said);
    };
    rec.onerror = () => setListening(false);
    rec.onend = () => setListening(false);

    recognition.current = rec;
    setListening(true);
    try {
      rec.start();
    } catch {
      // Starting twice throws; treat it as "already listening".
      setListening(false);
    }
  };

  return (
    <button
      className="oc-composer-btn"
      data-listening={listening}
      onClick={toggle}
      disabled={!supported}
      title={
        supported
          ? listening
            ? "Stop dictation"
            : "Dictate"
          : "Dictation needs the Web Speech API, which this runtime does not provide"
      }
      aria-label={listening ? "Stop dictation" : "Dictate"}
    >
      <Mic size={16} />
    </button>
  );
}

/* The Web Speech API is not in the DOM lib, so the two shapes actually used are
   declared here rather than widening the compiler's view of `window`. */
interface SpeechCtor {
  new (): SpeechRecognitionLike;
}
interface SpeechRecognitionLike {
  lang: string;
  interimResults: boolean;
  continuous: boolean;
  onresult: ((e: SpeechEventLike) => void) | null;
  onerror: (() => void) | null;
  onend: (() => void) | null;
  start(): void;
  stop(): void;
}
interface SpeechEventLike {
  results?: ArrayLike<ArrayLike<{ transcript: string }>>;
}

function AttachMenu({
  onClose,
  anchor,
  onPickImages,
  onSettings,
}: {
  onClose: () => void;
  anchor: HTMLElement | null;
  onPickImages: () => void;
  onSettings: () => void;
}) {
  return (
    <Popover anchor={anchor} onClose={onClose}>
      <button className="oc-popmenu-item" onClick={onPickImages}>
        <ImagePlus size={14} /> Images
      </button>
      <button className="oc-popmenu-item" onClick={onSettings}>
        <Paperclip size={14} /> Add a provider or model
      </button>
    </Popover>
  );
}

function PickerMenu({
  title,
  items,
  onClose,
  anchor,
}: {
  title: string;
  items: { label: string; hint?: string; active?: boolean; onSelect: () => void }[];
  onClose: () => void;
  anchor: HTMLElement | null;
}) {
  return (
    <Popover anchor={anchor} onClose={onClose}>
      <div className="oc-menu-label">{title}</div>
      {items.map((i) => (
        <button key={i.label} className="oc-popmenu-item" onClick={i.onSelect}>
          <span style={{ flex: 1 }}>{i.label}</span>
          {i.active ? <Check size={13} /> : null}
          {i.hint ? <span className="oc-faint" style={{ fontSize: "0.8em" }}>{i.hint}</span> : null}
        </button>
      ))}
    </Popover>
  );
}

function ModelPicker({
  onPick,
  onManage,
  onClose,
  anchor,
}: {
  onPick: (provider: string, model: string) => void;
  onManage: () => void;
  onClose: () => void;
  anchor: HTMLElement | null;
}) {
  const providers = useChat((s) => s.providers);
  const toast = useChat((s) => s.toast);
  const refresh = useChat((s) => s.refreshProviders);
  const [adding, setAdding] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const searchRef = useRef<HTMLInputElement>(null);

  // Several endpoints serve well over a hundred models, so the list is filtered
  // as you type. Focus is taken on open because reaching for the mouse to start
  // typing is the opposite of what a filter field is for.
  useEffect(() => {
    searchRef.current?.focus();
  }, []);

  const needle = query.trim().toLowerCase();
  const visible = useMemo(
    () =>
      providers
        .map((p) => ({
          ...p,
          models: needle
            ? p.models.filter(
                (m) =>
                  m.id.toLowerCase().includes(needle) ||
                  m.label.toLowerCase().includes(needle),
              )
            : p.models,
        }))
        // A provider with nothing matching is dropped entirely, so the list
        // never shows a heading over an empty section.
        .filter((p) => !needle || p.models.length > 0),
    [providers, needle],
  );

  const matches = visible.reduce((n, p) => n + p.models.length, 0);

  return (
    <Popover anchor={anchor} onClose={onClose} wide>
      <div className="oc-model-search">
        <Search size={13} className="oc-faint" />
        <input
          ref={searchRef}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Search models…"
          spellCheck={false}
          aria-label="Search models"
          onKeyDown={(e) => {
            // Escape closes the popover rather than clearing the box, so one
            // keystroke always gets you back to the conversation.
            if (e.key === "Escape") {
              e.preventDefault();
              e.stopPropagation();
              onClose();
            }
          }}
        />
        {query ? (
          <button
            className="oc-model-search-clear"
            onClick={() => {
              setQuery("");
              searchRef.current?.focus();
            }}
            aria-label="Clear search"
          >
            <X size={12} />
          </button>
        ) : null}
      </div>

      <div className="oc-model-list oc-scroll">
        {matches === 0 ? (
          <div className="oc-model-empty">
            {needle ? `No model matches “${query.trim()}”.` : "No models listed."}
          </div>
        ) : (
          visible.map((p) => (
            <div key={p.id}>
              <div className="oc-model-group">
                {p.label}
                {p.local ? <Badge tone="success">local</Badge> : null}
                <div className="oc-spacer" />
                <button
                  className="oc-model-probe"
                  title="Ask the endpoint which models it serves"
                  disabled={adding === p.id}
                  onClick={async () => {
                    setAdding(p.id);
                    try {
                      const found = await api.providerProbeModels(p.id);
                      await Promise.all(
                        found.map((m) => api.modelAdd(p.id, m).catch(() => undefined)),
                      );
                      await refresh();
                      toast("success", `${p.label}: added ${found.length} models`);
                    } catch (e) {
                      toast("warn", String(e));
                    } finally {
                      setAdding(null);
                    }
                  }}
                >
                  {adding === p.id ? <span className="oc-spinner" /> : <RefreshCw size={12} />}
                </button>
              </div>
              {p.models.map((m) => (
                <button
                  key={m.id}
                  className="oc-popmenu-item"
                  onClick={() => onPick(p.id, m.id)}
                >
                  <span className="oc-truncate" style={{ flex: 1 }}>
                    {m.label}
                  </span>
                  {m.contextWindow ? (
                    <span className="oc-faint" style={{ fontSize: "0.76em" }}>
                      {Math.round(m.contextWindow / 1000)}k
                    </span>
                  ) : null}
                  {m.reasoning ? <Sparkles size={12} className="oc-faint" /> : null}
                </button>
              ))}
            </div>
          ))
        )}
      </div>
      <button className="oc-popmenu-item" onClick={onManage}>
        <SlidersHorizontal size={14} /> Manage providers, keys and models
      </button>
    </Popover>
  );
}