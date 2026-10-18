import { useEffect, useRef, useState } from "react";
import {
  ChevronDown,
  Compass,
  FilePlus2,
  FolderPlus,
  HelpCircle,
  Minus,
  PanelLeft,
  PanelRight,
  Pencil,
  Plus,
  Search,
  Settings,
  Square,
  Star,
  Trash2,
  X,
} from "lucide-react";
import { Badge, Menu, type MenuEntry } from "./ui";
import { useChat } from "../store/chat";
import { api } from "../lib/ipc";
import { NewProjectDialog } from "./settings";

/**
 * Application menu, in the style of Cursor and other editors.
 *
 * This is the only always-visible route to Settings and the command palette.
 * Those commands used to live in the window-control cluster, which made them
 * impossible to find and competed with minimize/maximize/close for the same
 * corner of the bar.
 */
export function MenuBar() {
  const [open, setOpen] = useState<string | null>(null);
  const [newProjectForMenu, setNewProjectForMenu] = useState(false);

  const newChat = useChat((s) => s.newSession);
  const setPaletteOpen = useChat((s) => s.setPaletteOpen);
  const setSettingsOpen = useChat((s) => s.setSettingsOpen);
  const toggleSidebar = useChat((s) => s.toggleSidebar);
  const toggleRight = useChat((s) => s.toggleRightPanel);
  const sidebarOpen = useChat((s) => s.sidebarOpen);
  const rightOpen = useChat((s) => s.rightPanel !== null);
  const keybinds = useChat((s) => s.settings.keybinds);

  const act = (fn: () => void) => () => {
    setOpen(null);
    fn();
  };

  const menus: { id: string; label: string; items: MenuEntry[] }[] = [
    {
      id: "file",
      label: "File",
      items: [
        {
          label: "New chat",
          icon: <FilePlus2 size={13} />,
          shortcut: pretty(keybinds.newChat ?? "mod+n"),
          onSelect: act(() => void newChat()),
        },
        {
          label: "New project…",
          icon: <FolderPlus size={13} />,
          onSelect: act(() => setNewProjectForMenu(true)),
        },
      ],
    },
    {
      id: "edit",
      label: "Edit",
      items: [
        {
          label: "Command palette…",
          icon: <Compass size={13} />,
          shortcut: pretty(keybinds.commandPalette ?? "mod+k"),
          onSelect: act(() => setPaletteOpen(true)),
        },
        {
          label: "Settings…",
          icon: <Settings size={13} />,
          shortcut: pretty(keybinds.settings ?? "mod+,"),
          onSelect: act(() => setSettingsOpen(true)),
        },
      ],
    },
    {
      id: "view",
      label: "View",
      items: [
        {
          label: sidebarOpen ? "Hide sidebar" : "Show sidebar",
          icon: <PanelLeft size={13} />,
          shortcut: pretty(keybinds.toggleSidebar ?? "mod+b"),
          onSelect: act(toggleSidebar),
        },
        {
          label: rightOpen ? "Hide side panel" : "Show side panel",
          icon: <PanelRight size={13} />,
          shortcut: pretty(keybinds.toggleArtifacts ?? "mod+shift+a"),
          onSelect: act(toggleRight),
        },
      ],
    },
    {
      id: "help",
      label: "Help",
      items: [
        {
          label: "Keyboard shortcuts",
          icon: <HelpCircle size={13} />,
          onSelect: act(() => setPaletteOpen(true)),
        },
      ],
    },
  ];

  const active = menus.find((m) => m.id === open) ?? null;

  return (
    <nav className="oc-menubar" aria-label="Application menu">
      {menus.map((m) => (
        <div className="oc-menubar-host oc-no-drag" key={m.id}>
          <button
            className="oc-menubar-item"
            data-open={open === m.id}
            aria-haspopup="menu"
            aria-expanded={open === m.id}
            onClick={() => setOpen(open === m.id ? null : m.id)}
            onMouseEnter={() => open && setOpen(m.id)}
          >
            {m.label}
          </button>
        </div>
      ))}
      <div className="oc-spacer" />
      <span className="oc-menubar-version">v0.1.0</span>

      {active ? <Menu items={active.items} onClose={() => setOpen(null)} /> : null}
      <NewProjectDialog open={newProjectForMenu} onClose={() => setNewProjectForMenu(false)} />
    </nav>
  );
}

/** Renders a binding such as `mod+k` the way editors do: `Ctrl K`. */
function pretty(binding: string): string {
  const key = (binding.split("+").pop() ?? binding).toUpperCase();
  return `${binding.includes("mod") ? "Ctrl" : ""} ${key}`.trim();
}

export function TitleBar() {
  const sessions = useChat((s) => s.sessions);
  const activeId = useChat((s) => s.activeSessionId);
  const toggleSidebar = useChat((s) => s.toggleSidebar);
  const toggleRight = useChat((s) => s.toggleRightPanel);
  const sidebarOpen = useChat((s) => s.sidebarOpen);
  const rightOpen = useChat((s) => s.rightPanel !== null);
  const title = sessions.find((s) => s.id === activeId)?.title ?? "New chat";

  return (
    <header className="oc-titlebar">
      <button
        className="oc-pane-toggle"
        title={sidebarOpen ? "Hide sidebar" : "Show sidebar"}
        aria-label={sidebarOpen ? "Hide sidebar" : "Show sidebar"}
        onClick={toggleSidebar}
      >
        <PanelLeft size={15} />
      </button>

      <div className="oc-brand">
        <span>Octopus</span>
        <span className="oc-brand-sub oc-truncate">· {title}</span>
      </div>

      <div className="oc-spacer" />

      <button
        className="oc-pane-toggle"
        title={rightOpen ? "Hide side panel" : "Show side panel"}
        aria-label={rightOpen ? "Hide side panel" : "Show side panel"}
        onClick={toggleRight}
      >
        <PanelRight size={15} />
      </button>

      {/* Native window buttons: the window is undecorated, so the app has to
          supply its own frame controls. */}
      <div className="oc-window-buttons">
        <button title="Minimize" onClick={() => void api.windowMinimize()}>
          <Minus size={14} />
        </button>
        <button title="Maximize / restore" onClick={() => void api.windowToggleMaximize()}>
          <Square size={11} />
        </button>
        <button className="oc-win-close" title="Close" onClick={() => void api.windowClose()}>
          <X size={14} />
        </button>
      </div>
    </header>
  );
}

export function Sidebar({ width }: { width: number }) {
  const projects = useChat((s) => s.projects);
  const sessions = useChat((s) => s.sessions);
  const activeProjectId = useChat((s) => s.activeProjectId);
  const activeSessionId = useChat((s) => s.activeSessionId);
  const running = useChat((s) => s.running);
  const selectProject = useChat((s) => s.selectProject);
  const openSession = useChat((s) => s.openSession);
  const newSession = useChat((s) => s.newSession);
  const toggleSidebar = useChat((s) => s.toggleSidebar);
  const setPaletteOpen = useChat((s) => s.setPaletteOpen);
  const setSettingsOpen = useChat((s) => s.setSettingsOpen);

  const [menuFor, setMenuFor] = useState<string | null>(null);
  const [newProject, setNewProject] = useState(false);
  const active = projects.find((p) => p.id === activeProjectId) ?? null;

  return (
    <>
      <nav className="oc-sidebar" style={{ width }} aria-label="Chats">
        {/* New chat sits above the project picker. It is the action taken most
            often, so it belongs at the top of the pane rather than pinned to
            the bottom next to the collapse control. */}
        <button className="oc-new-chat" onClick={() => void newSession(activeProjectId)}>
          <FilePlus2 size={14} />
          New chat
        </button>

        {/* The project frame is one button: clicking anywhere on it starts a
            new project, which is the only action it offers. */}
        <button className="oc-project-frame" onClick={() => setNewProject(true)} title="New project">
          <span className="oc-project-bar">
            <FolderPlus size={14} className="oc-project-icon" />
            <span className="oc-truncate" style={{ flex: 1 }}>
              {active ? active.name : "No project"}
            </span>
            <ChevronDown size={13} className="oc-faint" />
          </span>
          <span className="oc-project-add">
            <Plus size={15} />
          </span>
        </button>

        {active?.roots.length ? (
          <div className="oc-project-roots">
            {active.roots.slice(0, 2).map((r) => (
              <div key={r} className="oc-faint oc-truncate oc-mono" title={r}>
                {r}
              </div>
            ))}
          </div>
        ) : null}

        <div className="oc-side-label">
          Repositories
          <div className="oc-spacer" />
          <button title="New project" onClick={() => setNewProject(true)}>
            <Plus size={13} />
          </button>
        </div>

        <div className="oc-repo-list">
          {projects.length === 0 ? (
            <div className="oc-side-empty">No repositories yet.</div>
          ) : (
            projects.map((p) => (
              <button
                key={p.id}
                className="oc-repo"
                data-active={p.id === activeProjectId}
                title={p.roots[0] ?? p.name}
                onClick={() => void selectProject(p.id)}
                onContextMenu={(e) => {
                  e.preventDefault();
                  setMenuFor(`repo:${p.id}`);
                }}
              >
                <FolderPlus size={14} />
                <span className="oc-repo-name">{p.name}</span>
              </button>
            ))
          )}
        </div>

        <div className="oc-side-label">
          Chats
          <div className="oc-spacer" />
        </div>

        <div className="oc-session-list oc-scroll">
          {sessions.length === 0 ? (
            <div className="oc-side-empty">No chats yet.</div>
          ) : (
            sessions.map((s) => (
              <button
                key={s.id}
                className="oc-session-item"
                data-active={s.id === activeSessionId}
                onClick={() => void openSession(s.id)}
                onContextMenu={(e) => {
                  e.preventDefault();
                  setMenuFor(s.id);
                }}
              >
                {s.favorite ? <Star size={12} className="oc-session-fav" /> : null}
                {s.id && running[s.id] ? <span className="oc-run-dot" /> : null}
                <span className="oc-session-title">{s.title}</span>
                {s.mode === "plan" ? <Badge tone="primary">plan</Badge> : null}
              </button>
            ))
          )}
        </div>

        <div className="oc-sidebar-foot">
          <button className="oc-nav-row" onClick={() => setPaletteOpen(true)}>
            <Search size={13} />
            Search
          </button>
          <button className="oc-nav-row" onClick={() => setSettingsOpen(true)}>
            <Settings size={13} />
            Settings
          </button>
          <div className="oc-spacer" />
          <button
            className="oc-pane-toggle"
            title="Collapse sidebar"
            aria-label="Collapse sidebar"
            onClick={toggleSidebar}
          >
            <PanelLeft size={15} />
          </button>
        </div>
      </nav>

      {menuFor?.startsWith("repo:") ? (
        <RepoMenu
          id={menuFor.slice("repo:".length)}
          onClose={() => setMenuFor(null)}
        />
      ) : menuFor ? (
        <SessionMenu id={menuFor} onClose={() => setMenuFor(null)} />
      ) : null}

      <NewProjectDialog open={newProject} onClose={() => setNewProject(false)} />
    </>
  );
}

/**
 * Context menu for one project. Removing a project is only reachable from here,
 * because the repository row has no room for a button of its own.
 */
function RepoMenu({ id, onClose }: { id: string; onClose: () => void }) {
  const project = useChat((s) => s.projects.find((p) => p.id === id) ?? null);
  const remove = useChat((s) => s.deleteProject);
  if (!project) return null;

  return (
    <Menu
      onClose={onClose}
      items={[
        { header: project.name },
        {
          label: "Remove project",
          icon: <Trash2 size={13} />,
          danger: true,
          onSelect: () => void remove(project.id),
        },
      ]}
    />
  );
}

/** Context menu for one chat. Split out so it only reads the store it needs. */
function SessionMenu({ id, onClose }: { id: string; onClose: () => void }) {
  const session = useChat((s) => s.sessions.find((x) => x.id === id) ?? null);
  const rename = useChat((s) => s.renameSession);
  const remove = useChat((s) => s.deleteSession);
  if (!session) return null;

  return (
    <Menu
      onClose={onClose}
      items={[
        {
          label: "Rename…",
          icon: <Pencil size={13} />,
          onSelect: () => {
            const next = window.prompt("Chat title", session.title);
            if (next) void rename(session.id, next);
          },
        },
        { separator: true },
        {
          label: "Delete",
          icon: <Trash2 size={13} />,
          danger: true,
          onSelect: () => void remove(session.id),
        },
      ]}
    />
  );
}

/** Horizontal drag handle that reports a width delta. */
export function Resizer({
  onResize,
}: {
  onResize: (delta: number) => void;
}) {
  const last = useRef<number | null>(null);
  const [dragging, setDragging] = useState(false);

  useEffect(() => {
    if (!dragging) return;
    const onMove = (e: MouseEvent) => {
      if (last.current === null) last.current = e.clientX;
      const delta = e.clientX - last.current;
      if (delta !== 0) {
        onResize(delta);
        last.current = e.clientX;
      }
    };
    const onUp = () => {
      last.current = null;
      setDragging(false);
      document.body.style.cursor = "";
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    return () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
  }, [dragging, onResize]);

  return (
    <div
      className="oc-resizer"
      data-dragging={dragging}
      onMouseDown={() => {
        setDragging(true);
        document.body.style.cursor = "col-resize";
      }}
      role="separator"
      aria-orientation="vertical"
    />
  );
}