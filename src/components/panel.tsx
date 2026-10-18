import { useCallback, useEffect, useMemo, useState } from "react";
import {
  ChevronDown,
  ChevronRight,
  Eye,
  FileCode2,
  FileDiff,
  Files,
  FolderOpen,
  Globe,
  Pencil,
  RefreshCw,
  Save,
  X,
} from "lucide-react";
import { api, type GitState } from "../lib/ipc";
import { useChat } from "../store/chat";
import { RawBlock } from "./markdown";
import { DiffView } from "./diff";
import { Button, cx, EmptyState, Spinner } from "./ui";

export type PanelTab = "files" | "changes" | "preview" | "editor";

export interface TreeNode {
  name: string;
  path: string;
  isDir: boolean;
  size: number;
  children: TreeNode[];
}

const TABS: { id: PanelTab; label: string; icon: React.ReactNode }[] = [
  { id: "files", label: "Files", icon: <Files size={13} /> },
  { id: "changes", label: "Changes", icon: <FileDiff size={13} /> },
  { id: "editor", label: "Editor", icon: <FileCode2 size={13} /> },
  { id: "preview", label: "Preview", icon: <Eye size={13} /> },
];

export function RightPanel({ width }: { width: number }) {
  const sessionId = useChat((s) => s.activeSessionId);
  const setRightPanel = useChat((s) => s.setRightPanel);
  const projects = useChat((s) => s.projects);
  const session = useChat((s) => s.sessions.find((x) => x.id === s.activeSessionId) ?? null);
  const project = projects.find((p) => p.id === session?.projectId) ?? null;
  const hasProject = Boolean(project?.roots.length);

  const [tab, setTab] = useState<PanelTab>("files");

  // Without a project there is nothing safe to browse, so the panel keeps its
  // tab bar (so it is never a dead end) and explains itself in the body.
  return (
    <aside className="oc-panel" style={{ width }} aria-label="Panel">
      <div className="oc-panel-head">
        {TABS.map((t) => (
          <button
            key={t.id}
            className="oc-panel-tab"
            data-active={tab === t.id}
            disabled={!hasProject}
            onClick={() => {
              setTab(t.id);
              setRightPanel(t.id);
            }}
            title={t.label}
          >
            {t.icon}
          </button>
        ))}
        <div className="oc-spacer" />
        {sessionId && hasProject ? <RefreshButton sessionId={sessionId} /> : null}
        <Button size="icon" variant="ghost" title="Hide panel" onClick={() => setRightPanel(null)}>
          <X size={13} />
        </Button>
      </div>

      {!hasProject ? (
        <EmptyState
          icon={<FolderOpen size={22} />}
          title="No project open"
          hint="Pick or create a project and this panel shows its files, the agent's changes, an editor and a preview."
        />
      ) : (
        <div className="oc-panel-body oc-scroll">
          {!sessionId ? null : tab === "files" ? (
            <FilesTab sessionId={sessionId} />
          ) : tab === "changes" ? (
            <ChangesTab sessionId={sessionId} />
          ) : tab === "editor" ? (
            <EditorTab sessionId={sessionId} />
          ) : (
            <PreviewTab sessionId={sessionId} />
          )}
        </div>
      )}
    </aside>
  );
}

/** Bumps a nonce so every panel tab reloads its data. */
function RefreshButton({ sessionId }: { sessionId: string }) {
  const running = useChat((s) => (sessionId ? s.running[sessionId] : false));
  return (
    <Button
      size="icon"
      variant="ghost"
      title="Refresh"
      onClick={() => window.dispatchEvent(new CustomEvent("octopus:refresh-panel"))}
    >
      <RefreshCw size={12} className={running ? "oc-spin" : undefined} />
    </Button>
  );
}

/** Reloads on demand and whenever a run finishes. */
function usePanelRefresh(deps: unknown[], load: () => void) {
  useEffect(() => {
    const onRefresh = () => load();
    window.addEventListener("octopus:refresh-panel", onRefresh);
    return () => window.removeEventListener("octopus:refresh-panel", onRefresh);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);
  useEffect(load, deps);
}

// -------------------------------------------------------------------- files

function FilesTab({ sessionId }: { sessionId: string }) {
  const [tree, setTree] = useState<TreeNode | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [openPath, setOpenPath] = useState<string | null>(null);
  const [preview, setPreview] = useState<{ path: string; text: string; language: string } | null>(
    null,
  );

  const running = useChat((s) => (sessionId ? s.running[sessionId] : false));
  usePanelRefresh([sessionId, running], () => {
    void api
      .panelTree(sessionId, "", 4)
      .then((t) => {
        setTree(t);
        setError(null);
        // Open the root level so the tree is not a single collapsed row.
        setExpanded(new Set(t.children.filter((c) => c.isDir).map((c) => c.path)));
      })
      .catch((e) => setError(String(e)));
  });

  const open = async (path: string) => {
    setOpenPath(path);
    setPreview(null);
    try {
      const f = await api.panelRead(sessionId, path);
      if (f.binary) {
        setPreview({ path, text: "(binary file)", language: "" });
      } else {
        setPreview({ path: f.path, text: f.text, language: f.language });
      }
    } catch (e) {
      setPreview({ path, text: String(e), language: "" });
    }
  };

  if (error) {
    return <EmptyState title="Could not list files" hint={error} />;
  }
  if (!tree) {
    return (
      <div className="oc-panel-loading">
        <Spinner /> Loading…
      </div>
    );
  }

  return (
    <div className="oc-split">
      <div className="oc-tree oc-scroll">
        {tree.children.map((child) => (
          <TreeRow
            key={child.path}
            node={child}
            depth={0}
            expanded={expanded}
            toggle={(p) =>
              setExpanded((s) => {
                const next = new Set(s);
                if (next.has(p)) next.delete(p);
                else next.add(p);
                return next;
              })
            }
            onOpen={(p) => void open(p)}
            active={openPath === child.path}
          />
        ))}
      </div>
      {preview ? (
        <div className="oc-fileview oc-scroll">
          <div className="oc-fileview-head oc-truncate">{preview.path}</div>
          {preview.text === "(binary file)" ? (
            <div className="oc-faint" style={{ padding: 12, fontSize: "0.86em" }}>
              Binary file
            </div>
          ) : (
            <RawBlock text={preview.text} lang={preview.language} />
          )}
        </div>
      ) : (
        <div className="oc-faint" style={{ padding: 12, fontSize: "0.85em" }}>
          Select a file to preview it.
        </div>
      )}
    </div>
  );
}

function TreeRow({
  node,
  depth,
  expanded,
  toggle,
  onOpen,
  active,
}: {
  node: TreeNode;
  depth: number;
  expanded: Set<string>;
  toggle: (path: string) => void;
  onOpen: (path: string) => void;
  active: boolean;
}) {
  const isOpen = expanded.has(node.path);
  return (
    <>
      <button
        className="oc-tree-row"
        data-active={active}
        style={{ paddingLeft: 8 + depth * 12 }}
        onClick={() => (node.isDir ? toggle(node.path) : onOpen(node.path))}
        title={node.path}
      >
        {node.isDir ? (
          isOpen ? (
            <ChevronDown size={12} className="oc-tree-caret" />
          ) : (
            <ChevronRight size={12} className="oc-tree-caret" />
          )
        ) : (
          <span className="oc-tree-caret" />
        )}
        {node.isDir ? <FolderOpen size={13} /> : <FileCode2 size={13} />}
        <span className="oc-truncate">{node.name}</span>
      </button>
      {node.isDir && isOpen
        ? node.children.map((child) => (
            <TreeRow
              key={child.path}
              node={child}
              depth={depth + 1}
              expanded={expanded}
              toggle={toggle}
              onOpen={onOpen}
              active={active}
            />
          ))
        : null}
    </>
  );
}

// ------------------------------------------------------------------ changes

function ChangesTab({ sessionId }: { sessionId: string }) {
  const [git, setGit] = useState<GitState | null>(null);
  const running = useChat((s) => (sessionId ? s.running[sessionId] : false));

  usePanelRefresh([sessionId, running], () => {
    let stale = false;
    void api
      .gitState(sessionId)
      .then((g) => !stale && setGit(g))
      .catch(() => !stale && setGit(null));
  });

  if (!git) return <div className="oc-panel-loading"><Spinner /> Reading git…</div>;
  if (!git.isRepo) {
    return (
      <EmptyState
        icon={<Globe size={20} />}
        title="Not a git repository"
        hint="Changes and rewind need git. Initialise it in the project folder to enable both."
      />
    );
  }
  if (git.changed.length === 0) {
    return <EmptyState title="Working tree clean" hint="Nothing changed since the last commit." />;
  }

  return (
    <div className="oc-col">
      <div className="oc-panel-sub">
        {git.changed.length} changed on <span className="oc-mono">{git.branch}</span>
      </div>
      <div className="oc-changelist oc-scroll">
        {git.changed.map((c) => (
          <div className="oc-changerow" key={c.path}>
            <span className="oc-change-code" data-staged={c.staged || undefined}>
              {c.status.trim()}
            </span>
            <span className="oc-truncate oc-mono">{c.path}</span>
          </div>
        ))}
      </div>
      {git.diff ? (
        <>
          <div className="oc-panel-sub">Diff</div>
          <DiffView diff={git.diff} />
        </>
      ) : null}
    </div>
  );
}

// ------------------------------------------------------------------- editor

function EditorTab({ sessionId }: { sessionId: string }) {
  const [path, setPath] = useState<string | null>(null);
  const [text, setText] = useState("");
  const [original, setOriginal] = useState("");
  const [language, setLanguage] = useState("");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [files, setFiles] = useState<TreeNode | null>(null);

  useEffect(() => {
    void api.panelTree(sessionId, "", 3).then(setFiles).catch(() => setFiles(null));
  }, [sessionId]);

  const load = useCallback(
    async (p: string) => {
      setPath(p);
      setError(null);
      try {
        const f = await api.panelRead(sessionId, p);
        if (f.binary) {
          setError("This file is binary.");
          return;
        }
        setText(f.text);
        setOriginal(f.text);
        setLanguage(f.language);
      } catch (e) {
        setError(String(e));
      }
    },
    [sessionId],
  );

  const dirty = text !== original;

  const save = async () => {
    if (!path) return;
    setSaving(true);
    try {
      const f = await api.panelWrite(sessionId, path, text);
      setText(f.text);
      setOriginal(f.text);
      setError(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="oc-col" style={{ height: "100%", minHeight: 0 }}>
      <div className="oc-editor-bar">
        <select
          className="oc-select"
          style={{ flex: 1, minWidth: 0 }}
          value={path ?? ""}
          onChange={(e) => void load(e.target.value)}
        >
          <option value="">Open a file…</option>
          {flatten(files).map((f) => (
            <option key={f.path} value={f.path}>
              {f.path}
            </option>
          ))}
        </select>
        <Button size="sm" variant="primary" disabled={!dirty || saving} onClick={() => void save()}>
          <Save size={12} /> Save
        </Button>
      </div>
      {error ? (
        <div className="oc-panel-error">{error}</div>
      ) : path ? (
        <>
          <div className="oc-editor-status">
            <span className="oc-truncate">{path}</span>
            <div className="oc-spacer" />
            {dirty ? <span style={{ color: "var(--oc-warning)" }}>unsaved</span> : null}
            <span className="oc-faint">{language}</span>
          </div>
          <textarea
            className="oc-editor-area oc-scroll"
            value={text}
            spellCheck={false}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              if ((e.ctrlKey || e.metaKey) && e.key === "s") {
                e.preventDefault();
                void save();
              }
            }}
          />
        </>
      ) : (
        <EmptyState icon={<Pencil size={20} />} title="No file open" hint="Pick a file to edit it." />
      )}
    </div>
  );
}

function flatten(node: TreeNode | null, out: TreeNode[] = []): TreeNode[] {
  if (!node) return out;
  for (const c of node.children) {
    if (c.isDir) flatten(c, out);
    else out.push(c);
  }
  return out;
}

// ------------------------------------------------------------------ preview

function PreviewTab({ sessionId }: { sessionId: string }) {
  const [file, setFile] = useState<{ path: string; html: string } | null>(null);
  const [files, setFiles] = useState<TreeNode | null>(null);
  const [url, setUrl] = useState("");

  useEffect(() => {
    void api.panelTree(sessionId, "", 4).then(setFiles).catch(() => setFiles(null));
  }, [sessionId]);

  const htmlFiles = useMemo(
    () => flatten(files).filter((f) => /\.html?$/i.test(f.name)),
    [files],
  );

  // A local file is rendered in a sandboxed iframe with no allow-scripts, so
  // previewing a document the agent just wrote cannot execute anything.
  const src = file ? `data:text/html;charset=utf-8,${encodeURIComponent(file.html)}` : "";

  return (
    <div className="oc-col" style={{ height: "100%", minHeight: 0 }}>
      <div className="oc-editor-bar">
        <select
          className="oc-select"
          style={{ flex: 1, minWidth: 0 }}
          value={file?.path ?? ""}
          onChange={async (e) => {
            const p = e.target.value;
            if (!p) {
              setFile(null);
              return;
            }
            const f = await api.panelRead(sessionId, p);
            setFile({ path: f.path, html: f.text });
          }}
        >
          <option value="">Preview an html file…</option>
          {htmlFiles.map((f) => (
            <option key={f.path} value={f.path}>
              {f.path}
            </option>
          ))}
        </select>
      </div>
      <div className="oc-editor-bar">
        <input
          className="oc-input"
          placeholder="http://127.0.0.1:3000"
          value={url}
          onChange={(e) => setUrl(e.target.value)}
        />
        <Button
          size="sm"
          disabled={!/^https?:\/\//i.test(url)}
          onClick={() => setFile({ path: url, html: `<!doctype html><meta charset="utf-8"><body style="font:14px system-ui;padding:16px;color:#888">Loading ${url}…<script>document.write('<iframe src="${url}" style="width:100%;height:100%;border:0"></iframe>')</script></body>` })}
        >
          <Globe size={12} /> Open
        </Button>
      </div>
      {file ? (
        <>
          <div className="oc-editor-status oc-truncate">
            {file.path}
            <span className="oc-faint"> · scripts disabled</span>
          </div>
          <iframe
            className="oc-preview"
            title="Preview"
            sandbox=""
            src={src.startsWith("data:text/html") ? src : undefined}
          />
          {file.path.startsWith("http") ? (
            <iframe className="oc-preview" title="Preview" src={file.path} sandbox="" />
          ) : null}
        </>
      ) : (
        <EmptyState
          icon={<Eye size={20} />}
          title="Nothing to preview"
          hint="Pick an html file the agent wrote, or point at a local dev server."
        />
      )}
    </div>
  );
}

export const panelClasses = cx;