import { useEffect, useMemo, useRef, useState } from "react";
import {
  FolderPlus,
  Moon,
  Palette,
  Plus,
  Search,
  Settings as SettingsIcon,
  Sun,
} from "lucide-react";
import { useChat } from "../store/chat";
import { themesForScheme, themeForScheme } from "../theme/apply";
import { findTheme } from "../theme/registry";
import type { Scheme } from "../theme/apply";
import { PERMISSION_LABELS, type PermissionMode, type Settings } from "../lib/types";

interface Command {
  id: string;
  label: string;
  hint?: string;
  group: string;
  keys?: string[];
  run: () => void;
}

/** Fuzzy subsequence match, so "ntp" finds "New chat". */
function fuzzy(needle: string, haystack: string): number | null {
  if (!needle) return 0;
  const n = needle.toLowerCase();
  const h = haystack.toLowerCase();
  let score = 0;
  let hi = 0;
  let streak = 0;
  for (const ch of n) {
    const found = h.indexOf(ch, hi);
    if (found === -1) return null;
    streak = found === hi ? streak + 1 : 0;
    score += 10 + streak * 4 - Math.min(found - hi, 8);
    hi = found + 1;
  }
  // Prefer matches that start early in the label.
  return score - h.indexOf(n[0] ?? "") * 0.5;
}

export function CommandPalette() {
  const open = useChat((s) => s.paletteOpen);
  const setOpen = useChat((s) => s.setPaletteOpen);
  const setSettingsOpen = useChat((s) => s.setSettingsOpen);
  const settings = useChat((s) => s.settings);
  const save = useChat((s) => s.saveSettings);
  const sessions = useChat((s) => s.sessions);
  const projects = useChat((s) => s.projects);
  const openSession = useChat((s) => s.openSession);
  const newSession = useChat((s) => s.newSession);
  const selectProject = useChat((s) => s.selectProject);

  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  const scheme: Scheme =
    settings.appearance === "light" || settings.appearance === "dark"
      ? settings.appearance
      : (findTheme(settings.theme)?.appearance ?? "dark");

  const commands = useMemo<Command[]>(() => {
    const list: Command[] = [
      {
        id: "new-chat",
        label: "New chat",
        group: "Actions",
        keys: ["mod", "n"],
        run: () => void newSession(),
      },
      {
        id: "new-project",
        label: "New project",
        group: "Actions",
        run: () => {
          // The dialog is owned by the sidebar; opening settings is the
          // reliable way in from here.
          setSettingsOpen(true);
        },
      },
      {
        id: "settings",
        label: "Open settings",
        group: "Actions",
        keys: ["mod", ","],
        run: () => setSettingsOpen(true),
      },
      {
        id: "toggle-sidebar",
        label: "Toggle sidebar",
        group: "View",
        keys: ["mod", "b"],
        run: () => window.dispatchEvent(new CustomEvent("octopus:toggle-sidebar")),
      },
      {
        id: "theme-dark",
        label: "Switch to a dark theme",
        group: "View",
        run: () => void save({ appearance: "dark", theme: themeForScheme(settings.theme, "dark") }),
      },
      {
        id: "theme-light",
        label: "Switch to a light theme",
        group: "View",
        run: () => void save({ appearance: "light", theme: themeForScheme(settings.theme, "light") }),
      },
      {
        id: "theme-picker",
        label: "Browse all themes",
        group: "View",
        run: () => setSettingsOpen(true),
      },
      {
        id: "perm-ask",
        label: `Permission: ${PERMISSION_LABELS.ask}`,
        group: "Safety",
        run: () => void save({ defaultPermissionMode: "ask" as PermissionMode }),
      },
      {
        id: "perm-auto",
        label: `Permission: ${PERMISSION_LABELS.auto_review}`,
        group: "Safety",
        run: () => void save({ defaultPermissionMode: "auto_review" as PermissionMode }),
      },
      {
        id: "perm-full",
        label: `Permission: ${PERMISSION_LABELS.full_access}`,
        group: "Safety",
        run: () => void save({ defaultPermissionMode: "full_access" as PermissionMode }),
      },
      {
        id: "local-only",
        label: settings.localOnly ? "Allow cloud providers" : "Go local only",
        group: "Safety",
        run: () => void save({ localOnly: !settings.localOnly }),
      },
    ];

    for (const p of projects) {
      list.push({
        id: `project-${p.id}`,
        label: `Go to project “${p.name}”`,
        group: "Projects",
        hint: p.roots[0],
        run: () => void selectProject(p.id),
      });
    }
    for (const s of sessions.slice(0, 40)) {
      list.push({
        id: `session-${s.id}`,
        label: s.title,
        group: "Chats",
        run: () => void openSession(s.id),
      });
    }
    for (const t of themesForScheme(scheme)) {
      list.push({
        id: `theme-${t.id}`,
        label: `Theme: ${t.name}`,
        group: "Themes",
        hint: t.author,
        run: () => void save({ theme: t.id }),
      });
    }
    return list;
  }, [projects, sessions, save, schema(settings), selectProject, setOpen, setSettingsOpen, settings, newSession, openSession, scheme]);

  const filtered = useMemo(() => {
    const scored = commands
      .map((c) => ({ c, score: fuzzy(query, c.label) }))
      .filter((x): x is { c: Command; score: number } => x.score !== null)
      .sort((a, b) => b.score - a.score);
    return scored.slice(0, 60).map((x) => x.c);
  }, [commands, query]);

  useEffect(() => {
    if (open) {
      setQuery("");
      setActive(0);
      requestAnimationFrame(() => inputRef.current?.focus());
    }
  }, [open]);

  useEffect(() => setActive(0), [query]);

  useEffect(() => {
    listRef.current?.querySelector('[data-active="true"]')?.scrollIntoView({ block: "nearest" });
  }, [active]);

  if (!open) return null;

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setActive((a) => Math.min(a + 1, filtered.length - 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActive((a) => Math.max(a - 1, 0));
    } else if (e.key === "Enter") {
      e.preventDefault();
      const cmd = filtered[active];
      if (cmd) {
        cmd.run();
        setOpen(false);
      }
    } else if (e.key === "Escape") {
      e.preventDefault();
      setOpen(false);
    }
  };

  let lastGroup = "";

  return (
    <div className="oc-scrim" onMouseDown={() => setOpen(false)}>
      <div
        className="oc-dialog"
        style={{ maxWidth: 620, alignSelf: "flex-start", marginTop: "12vh" }}
        onMouseDown={(e) => e.stopPropagation()}
      >
        <div className="oc-row" style={{ gap: 10, padding: "12px 15px", borderBottom: "1px solid var(--oc-border)" }}>
          <Search size={15} style={{ color: "var(--oc-text-faint)", flex: "0 0 auto" }} />
          <input
            ref={inputRef}
            className="oc-input"
            style={{ border: "none", background: "transparent", padding: 0 }}
            placeholder="Search chats, projects, themes, commands…"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={onKeyDown}
          />
          <kbd>esc</kbd>
        </div>

        <div ref={listRef} className="oc-scroll" style={{ maxHeight: "52vh", padding: 6 }}>
          {filtered.length === 0 ? (
            <div className="oc-empty" style={{ height: 120 }}>
              <div className="oc-faint">Nothing matches “{query}”</div>
            </div>
          ) : (
            filtered.map((c, i) => {
              const header = c.group !== lastGroup ? c.group : null;
              lastGroup = c.group;
              return (
                <div key={c.id}>
                  {header ? <div className="oc-menu-label">{header}</div> : null}
                  <button
                    className="oc-menu-item"
                    data-active={i === active}
                    onMouseEnter={() => setActive(i)}
                    onClick={() => {
                      c.run();
                      setOpen(false);
                    }}
                  >
                    {iconFor(c)}
                    <span style={{ flex: 1, minWidth: 0 }} className="oc-truncate">
                      {c.label}
                    </span>
                    {c.hint ? (
                      <span className="oc-faint oc-truncate" style={{ fontSize: "0.8em", maxWidth: 190 }}>
                        {c.hint}
                      </span>
                    ) : null}
                    {c.keys ? (
                      <span className="oc-row" style={{ gap: 3 }}>
                        {c.keys.map((k) => (
                          <kbd key={k}>{k}</kbd>
                        ))}
                      </span>
                    ) : null}
                  </button>
                </div>
              );
            })
          )}
        </div>
      </div>
    </div>
  );
}

/**
 * Identity key for the settings object. The command list only reads four
 * settings fields, so folding those into a string keeps the dependency honest
 * without pulling the whole object in.
 */
function schema(s: Settings): string {
  return JSON.stringify([s.theme, s.appearance, s.localOnly, s.defaultPermissionMode]);
}

function iconFor(c: Command) {
  if (c.id.startsWith("theme-")) return <Palette size={13} />;
  if (c.id.startsWith("project-")) return <FolderPlus size={13} />;
  if (c.id.startsWith("session-")) return <Plus size={13} />;
  if (c.id.startsWith("perm-")) return <SettingsIcon size={13} />;
  if (c.id === "theme-dark") return <Moon size={13} />;
  if (c.id === "theme-light") return <Sun size={13} />;
  return <SettingsIcon size={13} />;
}