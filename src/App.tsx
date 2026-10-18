import { useCallback, useEffect, useRef, useState } from "react";
import { AlertTriangle, CheckCircle2, Info, X, XCircle } from "lucide-react";
import { MenuBar, TitleBar, Sidebar, Resizer } from "./components/shell";
import { ChatView } from "./components/chat";
import { RightPanel } from "./components/panel";
import { CommandPalette } from "./components/palette";
import { SettingsDialog } from "./components/settings";
import { useChat } from "./store/chat";
import { applyTheme } from "./theme/apply";
import { Button } from "./components/ui";
import { chromeCss } from "./store/defaults";

const MIN_SIDEBAR = 190;
const MAX_SIDEBAR = 460;
const MIN_PANEL = 300;
const MAX_PANEL = 900;

export default function App() {
  const boot = useChat((s) => s.boot);
  const ready = useChat((s) => s.ready);
  const bootError = useChat((s) => s.bootError);
  const settings = useChat((s) => s.settings);
  const toasts = useChat((s) => s.toasts);
  const dismissToast = useChat((s) => s.dismissToast);
  const sidebarOpen = useChat((s) => s.sidebarOpen);
  const rightPanel = useChat((s) => s.rightPanel);
  const sessionMode = useChat((s) => {
    const id = s.activeSessionId;
    return s.sessions.find((x) => x.id === id)?.mode ?? "build";
  });

  const [sidebarWidth, setSidebarWidth] = useState(240);
  const [panelWidth, setPanelWidth] = useState(400);

  useEffect(() => {
    void boot();
  }, [boot]);

  // Theme and chrome are applied to the document rather than through React, so
  // switching either costs one style recalculation and no re-render.
  useEffect(() => {
    applyTheme(settings.theme, {
      appearance: settings.appearance,
      highContrast: settings.highContrast,
    });
  }, [settings.theme, settings.appearance, settings.highContrast]);

  useEffect(() => {
    const style = document.createElement("style");
    style.id = "octopus-chrome";
    style.textContent = chromeCss(settings);
    document.head.appendChild(style);
    return () => style.remove();
  }, [settings]);

  useEffect(() => {
    document.body.dataset.sessionMode = sessionMode;
  }, [sessionMode]);

  useEffect(() => {
    document.body.dataset.reduceMotion = String(settings.reduceMotion);
  }, [settings.reduceMotion]);

  const onSidebarResize = useCallback((delta: number) => {
    setSidebarWidth((w) => Math.min(MAX_SIDEBAR, Math.max(MIN_SIDEBAR, w + delta)));
  }, []);
  const onPanelResize = useCallback((delta: number) => {
    setPanelWidth((w) => Math.min(MAX_PANEL, Math.max(MIN_PANEL, w - delta)));
  }, []);

  useGlobalKeys(settings.keybinds);

  return (
    <div className="oc-app oc-surface">
      <MenuBar />
      <TitleBar />

      <div className="oc-main">
        {sidebarOpen ? (
          <>
            <Sidebar width={sidebarWidth} />
            <Resizer onResize={onSidebarResize} />
          </>
        ) : null}

        <ChatView />

        {rightPanel ? (
          <>
            <Resizer onResize={onPanelResize} />
            <RightPanel width={panelWidth} />
          </>
        ) : null}
      </div>

      <CommandPalette />
      <SettingsDialog />
      <Toasts items={toasts} onDismiss={dismissToast} />

      {bootError ? (
        <div className="oc-scrim">
          <div className="oc-dialog" style={{ maxWidth: 460 }}>
            <div className="oc-dialog-head">
              <div className="oc-dialog-title">Octopus could not start</div>
            </div>
            <div className="oc-dialog-body">
              <p style={{ marginTop: 0 }}>{bootError}</p>
              <p className="oc-field-hint">
                The app database lives in your user data folder. If it is locked by another copy
                of Octopus, close that copy and restart.
              </p>
            </div>
            <div className="oc-dialog-foot">
              <Button variant="primary" onClick={() => location.reload()}>
                Restart
              </Button>
            </div>
          </div>
        </div>
      ) : null}

      {!ready && !bootError ? <BootSplash /> : null}
    </div>
  );
}

function BootSplash() {
  return (
    <div className="oc-scrim" style={{ background: "var(--oc-bg)" }}>
      <div className="oc-col" style={{ alignItems: "center", gap: 14 }}>
        <span className="oc-spinner" style={{ width: 22, height: 22 }} />
        <span className="oc-faint">Starting Octopus…</span>
      </div>
    </div>
  );
}

function Toasts({
  items,
  onDismiss,
}: {
  items: { id: string; kind: string; text: string }[];
  onDismiss: (id: string) => void;
}) {
  if (items.length === 0) return null;
  return (
    <div className="oc-toasts">
      {items.map((t) => (
        <div key={t.id} className="oc-toast">
          <span className="oc-toast-icon">
            {t.kind === "error" ? (
              <XCircle size={15} style={{ color: "var(--oc-danger)" }} />
            ) : t.kind === "warn" ? (
              <AlertTriangle size={15} style={{ color: "var(--oc-warning)" }} />
            ) : t.kind === "success" ? (
              <CheckCircle2 size={15} style={{ color: "var(--oc-success)" }} />
            ) : (
              <Info size={15} style={{ color: "var(--oc-info)" }} />
            )}
          </span>
          <span style={{ flex: 1 }}>{t.text}</span>
          <button onClick={() => onDismiss(t.id)} aria-label="Dismiss">
            <X size={13} />
          </button>
        </div>
      ))}
    </div>
  );
}

/** Parses an editor-style binding such as `mod+shift+m`. */
function matches(e: KeyboardEvent, binding: string): boolean {
  const parts = binding.toLowerCase().split("+").map((p) => p.trim());
  const key = parts[parts.length - 1];
  const wantMod = parts.includes("mod");
  const wantShift = parts.includes("shift");
  const wantAlt = parts.includes("alt");

  const mod = e.ctrlKey || e.metaKey;
  if (wantMod !== mod) return false;
  if (wantShift !== e.shiftKey) return false;
  if (wantAlt !== e.altKey) return false;
  return e.key.toLowerCase() === key;
}

function useGlobalKeys(keybinds: Record<string, string>) {
  const ref = useRef(keybinds);
  ref.current = keybinds;

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const b = ref.current;
      const store = useChat.getState();

      if (matches(e, b.commandPalette ?? "mod+k")) {
        e.preventDefault();
        store.setPaletteOpen(!store.paletteOpen);
        return;
      }
      if (matches(e, b.settings ?? "mod+,")) {
        e.preventDefault();
        store.setSettingsOpen(true);
        return;
      }
      if (matches(e, b.newChat ?? "mod+n")) {
        e.preventDefault();
        void store.newSession();
        return;
      }
      if (matches(e, b.toggleSidebar ?? "mod+b")) {
        e.preventDefault();
        store.toggleSidebar();
        return;
      }
      if (matches(e, b.toggleArtifacts ?? "mod+shift+a")) {
        e.preventDefault();
        store.toggleRightPanel();
        return;
      }
      if (matches(e, b.stop ?? "escape")) {
        const id = store.activeSessionId;
        if (id && store.running[id]) {
          e.preventDefault();
          void store.stopRun();
        }
        return;
      }
      if (matches(e, b.toggleMode ?? "mod+shift+m")) {
        e.preventDefault();
        const id = store.activeSessionId;
        if (!id) return;
        const s = store.sessions.find((x) => x.id === id);
        if (s) void store.setSessionMode(id, s.mode === "plan" ? "build" : "plan");
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
}