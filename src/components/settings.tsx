import { useEffect, useMemo, useState } from "react";
import {
  Check,
  FolderOpen,
  Plus,
  RefreshCw,
  Search,
  Trash2,
  Wifi,
  WifiOff,
} from "lucide-react";
import { Badge, Button, Dialog, Field, Switch } from "./ui";
import { useChat } from "../store/chat";
import { api, BackendError, type CustomProvider, type ResolvedProvider } from "../lib/ipc";
import { THEMES, findTheme } from "../theme/registry";
import { themesForScheme, type Scheme } from "../theme/apply";
import type { PermissionMode, Settings } from "../lib/types";
import { PERMISSION_HINTS, PERMISSION_LABELS } from "../lib/types";

type Tab = "general" | "appearance" | "providers" | "permissions" | "keys" | "about";

const TABS: { id: Tab; label: string }[] = [
  { id: "general", label: "General" },
  { id: "appearance", label: "Appearance" },
  { id: "providers", label: "Providers" },
  { id: "permissions", label: "Permissions" },
  { id: "keys", label: "Shortcuts" },
  { id: "about", label: "About" },
];

export function SettingsDialog() {
  const open = useChat((s) => s.settingsOpen);
  const setOpen = useChat((s) => s.setSettingsOpen);
  const [tab, setTab] = useState<Tab>("general");

  return (
    <Dialog open={open} title="Settings" onClose={() => setOpen(false)} width={860}>
      <div style={{ display: "grid", gridTemplateColumns: "176px 1fr", gap: 22, minHeight: 420 }}>
        <nav className="oc-col" style={{ gap: 1 }}>
          {TABS.map((t) => (
            <button
              key={t.id}
              className="oc-menu-item"
              data-active={tab === t.id}
              onClick={() => setTab(t.id)}
              style={{ fontWeight: tab === t.id ? 600 : 450 }}
            >
              {t.label}
            </button>
          ))}
        </nav>
        <div className="oc-scroll" style={{ maxHeight: "62vh", paddingRight: 6 }}>
          {tab === "general" ? <GeneralTab /> : null}
          {tab === "appearance" ? <AppearanceTab /> : null}
          {tab === "providers" ? <ProvidersTab /> : null}
          {tab === "permissions" ? <PermissionsTab /> : null}
          {tab === "keys" ? <ShortcutsTab /> : null}
          {tab === "about" ? <AboutTab /> : null}
        </div>
      </div>
    </Dialog>
  );
}

// ------------------------------------------------------------------- general

function GeneralTab() {
  const settings = useChat((s) => s.settings);
  const save = useChat((s) => s.saveSettings);

  return (
    <>
      <Field
        label="Default provider and model"
        hint="New chats start here. You can override per chat."
      >
        <ProviderModelPickers settings={settings} onSave={save} />
      </Field>

      <Field label="Default permission mode" hint={PERMISSION_HINTS[settings.defaultPermissionMode]}>
        <select
          className="oc-select"
          value={settings.defaultPermissionMode}
          onChange={(e) => void save({ defaultPermissionMode: e.target.value as PermissionMode })}
        >
          {(Object.keys(PERMISSION_LABELS) as PermissionMode[]).map((m) => (
            <option key={m} value={m}>
              {PERMISSION_LABELS[m]}
            </option>
          ))}
        </select>
      </Field>

      <Toggle
        label="Send on Enter"
        hint="When off, Enter inserts a newline and the modifier sends."
        value={settings.sendOnEnter}
        onChange={(v) => void save({ sendOnEnter: v })}
      />
      <Toggle
        label="Name chats automatically"
        hint="Uses your first message as the chat title."
        value={settings.autoTitle}
        onChange={(v) => void save({ autoTitle: v })}
      />
      <Toggle
        label="Keep the machine awake while working"
        value={settings.preventSleepWhileRunning}
        onChange={(v) => void save({ preventSleepWhileRunning: v })}
      />
      <Toggle
        label="Notify when a run finishes"
        value={settings.notifyOnComplete}
        onChange={(v) => void save({ notifyOnComplete: v })}
      />
      <Toggle
        label="Local only"
        hint="Restricts the app to local models and on-device endpoints."
        value={settings.localOnly}
        onChange={async (v) => {
          await save({ localOnly: v });
          await api.setLocalOnly(v).catch(() => undefined);
        }}
      />

      <Field
        label="Ignored paths"
        hint="Skipped when indexing, searching and letting the agent browse. One glob per line."
      >
        <textarea
          className="oc-textarea"
          value={settings.ignoreGlobs.join("\n")}
          onChange={(e) =>
            void save({
              ignoreGlobs: e.target.value.split("\n").map((s) => s.trim()).filter(Boolean),
            })
          }
          style={{ minHeight: 120 }}
        />
      </Field>
    </>
  );
}

function Toggle({
  label,
  hint,
  value,
  onChange,
}: {
  label: string;
  hint?: string;
  value: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <div
      className="oc-row"
      style={{ gap: 12, padding: "9px 0", borderTop: "1px solid var(--oc-border-subtle)" }}
    >
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ fontSize: "0.92em", fontWeight: 500 }}>{label}</div>
        {hint ? <div className="oc-field-hint">{hint}</div> : null}
      </div>
      <Switch checked={value} onChange={onChange} label={label} />
    </div>
  );
}

function ProviderModelPickers({
  settings,
  onSave,
}: {
  settings: Settings;
  onSave: (patch: Partial<Settings>) => Promise<void>;
}) {
  const providers = useChat((s) => s.providers);
  const provider = providers.find((p) => p.id === settings.defaultProvider);

  return (
    <div className="oc-row" style={{ gap: 8 }}>
      <select
        className="oc-select"
        value={settings.defaultProvider}
        onChange={(e) => void onSave({ defaultProvider: e.target.value })}
      >
        {providers.map((p) => (
          <option key={p.id} value={p.id}>
            {p.label}
            {p.local ? " · local" : ""}
          </option>
        ))}
      </select>
      <select
        className="oc-select"
        value={settings.defaultModel}
        onChange={(e) => void onSave({ defaultModel: e.target.value })}
      >
        <option value="">(no default)</option>
        {(provider?.models ?? []).map((m) => (
          <option key={m.id} value={m.id}>
            {m.label}
          </option>
        ))}
      </select>
    </div>
  );
}

// ---------------------------------------------------------------- appearance

function AppearanceTab() {
  const settings = useChat((s) => s.settings);
  const save = useChat((s) => s.saveSettings);
  const [query, setQuery] = useState("");

  const scheme: Scheme =
    settings.appearance === "light" || settings.appearance === "dark"
      ? settings.appearance
      : (findTheme(settings.theme)?.appearance ?? "dark");

  const list = useMemo(() => {
    const all = themesForScheme(scheme);
    if (!query.trim()) return all;
    const q = query.trim().toLowerCase();
    return all.filter((t) => t.name.toLowerCase().includes(q) || t.author.toLowerCase().includes(q));
  }, [scheme, query]);

  return (
    <>
      <Field label="Appearance" hint="System follows your operating system's light or dark setting.">
        <div className="oc-row" style={{ gap: 6 }}>
          {(["system", "dark", "light"] as const).map((a) => (
            <Button
              key={a}
              size="sm"
              variant={(settings.appearance ?? "system") === a ? "primary" : "default"}
              onClick={() => void save({ appearance: a })}
            >
              {a[0].toUpperCase() + a.slice(1)}
            </Button>
          ))}
        </div>
      </Field>

      <Field label="Theme" hint={`${THEMES.length} bundled. ${list.length} shown for ${scheme}.`}>
        <input
          className="oc-input"
          placeholder="Search themes or authors…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
      </Field>

      <div
        style={{
          display: "grid",
          gridTemplateColumns: "repeat(auto-fill, minmax(150px, 1fr))", gap: 8,
        }}
      >
        {list.map((t) => (
          <button
            key={t.id}
            className="oc-btn"
            onClick={() => void save({ theme: t.id })}
            style={{
              height: "auto",
              padding: "9px 11px",
              flexDirection: "column",
              alignItems: "stretch",
              gap: 7,
              borderColor:
                settings.theme === t.id ? "var(--oc-primary)" : "var(--oc-border)",
              background: settings.theme === t.id ? "var(--oc-bg-selected)" : "var(--oc-bg-elevated)",
            }}
          >
            <span className="oc-row" style={{ gap: 4 }}>
              {t.swatch.map((c, i) => (
                <span
                  key={i}
                  style={{
                    width: 15,
                    height: 15,
                    borderRadius: 4,
                    background: c,
                    border: "1px solid rgb(255 255 255 / 0.14)",
                  }}
                />
              ))}
              <span className="oc-spacer" />
              {settings.theme === t.id ? (
                <Check size={12} style={{ color: "var(--oc-primary)" }} />
              ) : null}
            </span>
            <span style={{ fontSize: "0.86em", fontWeight: 550, textAlign: "left" }}>{t.name}</span>
            <span className="oc-faint" style={{ fontSize: "0.72em", textAlign: "left" }}>
              {t.author}
            </span>
          </button>
        ))}
      </div>

      <Field label="Interface font">
        <select
          className="oc-select"
          value={settings.fontFamily}
          onChange={(e) => void save({ fontFamily: e.target.value })}
        >
          {["Inter", "Segoe UI", "IBM Plex Sans", "JetBrains Sans", "System"].map((f) => (
            <option key={f} value={f}>
              {f}
            </option>
          ))}
        </select>
      </Field>

      <Field label="Code font">
        <select
          className="oc-select"
          value={settings.codeFontFamily}
          onChange={(e) => void save({ codeFontFamily: e.target.value })}
        >
          {["JetBrains Mono", "Cascadia Code", "IBM Plex Mono", "SF Mono", "Consolas", "Fira Code"].map(
            (f) => (
              <option key={f} value={f}>
                {f}
              </option>
            ),
          )}
        </select>
      </Field>

      <Slider
        label="Text size"
        value={settings.fontSize}
        min={11}
        max={20}
        suffix="px"
        onChange={(v) => void save({ fontSize: v })}
      />
      <Slider
        label="Line height"
        value={settings.lineHeight}
        min={1.2}
        max={2}
        step={0.05}
        onChange={(v) => void save({ lineHeight: v })}
      />
      <Toggle
        label="Reduce motion"
        value={settings.reduceMotion}
        onChange={(v) => void save({ reduceMotion: v })}
      />
      <Toggle
        label="High contrast"
        hint="Strengthens borders and comment contrast in every theme."
        value={settings.highContrast}
        onChange={(v) => void save({ highContrast: v })}
      />
    </>
  );
}

function Slider({
  label,
  value,
  min,
  max,
  step = 1,
  suffix = "",
  onChange,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  step?: number;
  suffix?: string;
  onChange: (v: number) => void;
}) {
  return (
    <div className="oc-field">
      <span className="oc-field-label">{label}</span>
      <div className="oc-row" style={{ gap: 10 }}>
        <input
          type="range"
          min={min}
          max={max}
          step={step}
          value={value}
          onChange={(e) => void onChange(Number(e.target.value))}
          style={{ flex: 1 }}
        />
        <span className="oc-mono oc-faint" style={{ width: 48, textAlign: "right", fontSize: "0.85em" }}>
          {Number.isInteger(value) ? value : value.toFixed(2)}
          {suffix}
        </span>
      </div>
    </div>
  );
}

// ----------------------------------------------------------------- providers

function ProvidersTab() {
  const providers = useChat((s) => s.providers);
  const refresh = useChat((s) => s.refreshProviders);
  const toast = useChat((s) => s.toast);
  const [reach, setReach] = useState<Record<string, boolean | null>>({});
  const [probing, setProbing] = useState<string | null>(null);
  const [editing, setEditing] = useState<CustomProvider | null>(null);

  const local = providers.filter((p) => p.local);
  const cloud = providers.filter((p) => !p.local);
  const custom = providers.filter((p) => p.custom);

  const probe = async (p: ResolvedProvider) => {
    setProbing(p.id);
    try {
      const models = await api.providerProbeModels(p.id);
      await Promise.all(models.map((m) => api.modelAdd(p.id, m).catch(() => undefined)));
      await refresh();
      toast("success", `${p.label} reported ${models.length} models`);
    } catch (e) {
      toast("warn", `${p.label}: ${e instanceof BackendError ? e.message : String(e)}`);
    } finally {
      setProbing(null);
    }
  };

  const ping = async (p: ResolvedProvider) => {
    setProbing(p.id);
    try {
      const ok = await api.providerPing(p.id);
      setReach((r) => ({ ...r, [p.id]: ok }));
    } catch {
      setReach((r) => ({ ...r, [p.id]: false }));
    } finally {
      setProbing(null);
    }
  };

  const group = (title: string, items: ResolvedProvider[]) =>
    items.length === 0 ? null : (
      <div style={{ marginBottom: 20 }}>
        <div className="oc-group-label" style={{ padding: "0 0 7px" }}>
          {title}
        </div>
        {items.map((p) => (
          <div
            key={p.id}
            className="oc-row"
            style={{
              gap: 10,
              padding: "10px 11px",
              marginBottom: 6,
              border: "1px solid var(--oc-border)",
              borderRadius: 9,
              background: "var(--oc-bg-elevated)",
            }}
          >
            <div style={{ flex: 1, minWidth: 0 }}>
              <div className="oc-row" style={{ gap: 7 }}>
                <strong style={{ fontSize: "0.92em" }}>{p.label}</strong>
                {p.local ? <Badge tone="success">local</Badge> : null}
                {p.needsKey ? (
                  p.hasKey ? (
                    <Badge tone="info">key set</Badge>
                  ) : (
                    <Badge tone="warn">no key</Badge>
                  )
                ) : null}
                {reach[p.id] === true ? <Badge tone="success">reachable</Badge> : null}
                {reach[p.id] === false ? <Badge tone="danger">unreachable</Badge> : null}
              </div>
              <div className="oc-faint oc-mono oc-truncate" style={{ fontSize: "0.76em", marginTop: 3 }}>
                {p.baseUrl}
              </div>
              <div className="oc-faint" style={{ fontSize: "0.76em", marginTop: 2 }}>
                {p.models.length} models listed
                {p.models.length === 0 ? " — add one below" : ""}
              </div>
            </div>

            <Button
              size="sm"
              variant="ghost"
              title="Check the endpoint"
              disabled={probing === p.id}
              onClick={() => void ping(p)}
            >
              <Wifi size={13} />
            </Button>
            <Button
              size="sm"
              variant="ghost"
              title="Ask the endpoint which models it serves"
              disabled={probing === p.id}
              onClick={() => void probe(p)}
            >
              {probing === p.id ? <span className="oc-spinner" /> : <RefreshCw size={13} />}
            </Button>
          </div>
        ))}
      </div>
    );

  return (
    <>
      <div
        className="oc-row"
        style={{ marginBottom: 14, gap: 8 }}
      >
        <p className="oc-field-hint" style={{ flex: 1, margin: 0 }}>
          Local providers need no key and never leave your machine. Cloud providers store their key
          in the app database on this machine only.
        </p>
        <Button size="sm" onClick={() => setEditing({ id: "", label: "", dialect: "openai", baseUrl: "http://127.0.0.1:8000/v1", local: true, apiKey: null, headers: [], models: [] })}>
          <Plus size={13} /> Add provider
        </Button>
      </div>

      {group("On this machine", local)}
      {group("Cloud", cloud)}
      {group("Added by you", custom)}

      <ProviderKeys providers={providers.filter((p) => !p.custom && p.needsKey)} />

      <AddModel />

      {editing ? (
        <CustomProviderDialog
          provider={editing}
          onClose={() => setEditing(null)}
          onSaved={async () => {
            setEditing(null);
            await refresh();
          }}
        />
      ) : null}
    </>
  );
}

function ProviderKeys({ providers }: { providers: ResolvedProvider[] }) {
  const refresh = useChat((s) => s.refreshProviders);
  const toast = useChat((s) => s.toast);
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [reveal, setReveal] = useState<string | null>(null);

  if (providers.length === 0) return null;

  return (
    <div style={{ marginBottom: 20 }}>
      <div className="oc-group-label" style={{ padding: "0 0 7px" }}>
        API keys
      </div>
      {providers.map((p) => (
        <div key={p.id} className="oc-row" style={{ gap: 8, marginBottom: 8 }}>
          <span style={{ width: 128, fontSize: "0.9em" }}>{p.label}</span>
          <input
            className="oc-input"
            type={reveal === p.id ? "text" : "password"}
            placeholder={p.hasKey ? "•••••••• stored" : p.keyUrl ? "Paste your key" : "Not required"}
            value={drafts[p.id] ?? ""}
            onChange={(e) => setDrafts((d) => ({ ...d, [p.id]: e.target.value }))}
          />
          <Button
            size="sm"
            variant="ghost"
            title="Show"
            onClick={() => setReveal(reveal === p.id ? null : p.id)}
          >
            {reveal === p.id ? <WifiOff size={13} /> : <Search size={13} />}
          </Button>
          <Button
            size="sm"
            variant="primary"
            disabled={!(drafts[p.id] ?? "").trim()}
            onClick={async () => {
              try {
                await api.providerSetKey(p.id, drafts[p.id]);
                setDrafts((d) => ({ ...d, [p.id]: "" }));
                await refresh();
                toast("success", `${p.label} key saved`);
              } catch (e) {
                toast("error", String(e));
              }
            }}
          >
            Save
          </Button>
        </div>
      ))}
    </div>
  );
}

function AddModel() {
  const providers = useChat((s) => s.providers);
  const refresh = useChat((s) => s.refreshProviders);
  const toast = useChat((s) => s.toast);
  const [provider, setProvider] = useState("");
  const [model, setModel] = useState("");
  const [busy, setBusy] = useState(false);

  // Defaults to the first provider so the control is usable without touching the
  // select, which is the step people were missing.
  useEffect(() => {
    if (!provider && providers.length > 0) setProvider(providers[0].id);
  }, [providers, provider]);

  const add = async () => {
    if (!provider || !model.trim()) return;
    setBusy(true);
    try {
      const id = model.trim();
      await api.modelAdd(provider, id);
      setModel("");
      await refresh();
      const label = providers.find((p) => p.id === provider)?.label ?? provider;
      toast("success", `Added ${id} to ${label}`);
    } catch (err) {
      toast("error", err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  const target = providers.find((p) => p.id === provider);

  return (
    <div style={{ marginBottom: 8 }}>
      <div className="oc-group-label" style={{ padding: "0 0 7px" }}>
        Add a model
      </div>
      <p className="oc-field-hint" style={{ marginTop: 0 }}>
        Any model id works, including ones released after this build. Type what your endpoint calls
        it. It is added to whichever provider is selected and appears in the composer straight away.
      </p>
      <div className="oc-row" style={{ gap: 8 }}>
        <select
          className="oc-select"
          style={{ width: 200 }}
          value={provider}
          onChange={(e) => setProvider(e.target.value)}
        >
          {providers.map((p) => (
            <option key={p.id} value={p.id}>
              {p.label}
            </option>
          ))}
        </select>
        <input
          className="oc-input"
          placeholder="model id, e.g. qwen3-coder:30b"
          value={model}
          onChange={(e) => setModel(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              void add();
            }
          }}
        />
        <Button
          variant="primary"
          size="sm"
          disabled={busy || !provider || !model.trim()}
          onClick={() => void add()}
        >
          {busy ? <span className="oc-spinner" /> : <Plus size={13} />} Add model
        </Button>
      </div>
      {target && target.models.length > 0 ? (
        <p className="oc-field-hint" style={{ marginTop: 7 }}>
          {target.label} lists {target.models.length}{" "}
          {target.models.length === 1 ? "model" : "models"}.
        </p>
      ) : null}
    </div>
  );
}

function CustomProviderDialog({
  provider,
  onClose,
  onSaved,
}: {
  provider: CustomProvider;
  onClose: () => void;
  onSaved: () => Promise<void>;
}) {
  const [draft, setDraft] = useState<CustomProvider>(provider);
  const [modelId, setModelId] = useState("");
  const [busy, setBusy] = useState(false);
  const toast = useChat((s) => s.toast);

  const set = <K extends keyof CustomProvider>(k: K, v: CustomProvider[K]) =>
    setDraft((d) => ({ ...d, [k]: v }));

  return (
    <Dialog
      open
      title={provider.id ? "Edit provider" : "Add provider"}
      onClose={onClose}
      footer={
        <>
          <Button variant="primary" disabled={busy} onClick={async () => {
            setBusy(true);
            try {
              await api.providerSaveCustom(draft);
              await onSaved();
            } catch (e) {
              toast("error", String(e));
            } finally {
              setBusy(false);
            }
          }}>
            Save provider
          </Button>
          <div className="oc-spacer" />
          {draft.id ? (
            <Button
              variant="danger"
              onClick={async () => {
                await api.providerDeleteCustom(draft.id);
                await onSaved();
              }}
            >
              <Trash2 size={12} /> Delete
            </Button>
          ) : null}
        </>
      }
    >
      <Field label="Name">
        <input className="oc-input" value={draft.label} onChange={(e) => set("label", e.target.value)} />
      </Field>
      <Field label="Base URL" hint="Must start with http:// or https://">
        <input
          className="oc-input oc-mono"
          value={draft.baseUrl}
          onChange={(e) => set("baseUrl", e.target.value)}
        />
      </Field>
      <Field label="Wire format">
        <select
          className="oc-select"
          value={draft.dialect}
          onChange={(e) => set("dialect", e.target.value as CustomProvider["dialect"])}
        >
          <option value="openai">OpenAI-compatible (/chat/completions)</option>
          <option value="anthropic">Anthropic Messages</option>
          <option value="gemini">Google Gemini</option>
        </select>
      </Field>
      <Toggle
        label="Runs on this machine"
        hint="Marks the endpoint as local: no key required and nothing is sent anywhere else."
        value={draft.local}
        onChange={(v) => set("local", v)}
      />
      {!draft.local ? (
        <Field label="API key" hint="Stored on this machine only.">
          <input
            className="oc-input"
            type="password"
            value={draft.apiKey ?? ""}
            onChange={(e) => set("apiKey", e.target.value)}
          />
        </Field>
      ) : null}

      <div className="oc-group-label" style={{ padding: "14px 0 7px" }}>
        Models
      </div>
      <div className="oc-row" style={{ gap: 8, marginBottom: 10 }}>
        <input
          className="oc-input"
          placeholder="model id"
          value={modelId}
          onChange={(e) => setModelId(e.target.value)}
        />
        <Button
          size="sm"
          disabled={!modelId.trim()}
          onClick={() => {
            const id = modelId.trim();
            set("models", [
              ...draft.models,
              {
                id,
                label: id,
                dialect: draft.dialect,
                contextWindow: null,
                maxOutput: null,
                supportsTools: true,
                supportsVision: false,
                reasoning: false,
                inputCost: null,
                outputCost: null,
              },
            ]);
            setModelId("");
          }}
        >
          <Plus size={12} /> Add
        </Button>
      </div>
      {draft.models.map((m, i) => (
        <div key={m.id} className="oc-row" style={{ gap: 8, padding: "5px 0" }}>
          <span className="oc-mono" style={{ flex: 1, fontSize: "0.87em" }}>
            {m.id}
          </span>
          <Button
            size="icon"
            variant="ghost"
            title="Remove"
            onClick={() => set("models", draft.models.filter((_, j) => j !== i))}
          >
            <Trash2 size={12} />
          </Button>
        </div>
      ))}
    </Dialog>
  );
}

// --------------------------------------------------------------- permissions

function PermissionsTab() {
  const settings = useChat((s) => s.settings);
  const save = useChat((s) => s.saveSettings);
  const projects = useChat((s) => s.projects);
  const toast = useChat((s) => s.toast);

  return (
    <>
      <Field label="Default permission mode">
        <select
          className="oc-select"
          value={settings.defaultPermissionMode}
          onChange={(e) => void save({ defaultPermissionMode: e.target.value as PermissionMode })}
        >
          {(Object.keys(PERMISSION_LABELS) as PermissionMode[]).map((m) => (
            <option key={m} value={m}>
              {PERMISSION_LABELS[m]}
            </option>
          ))}
        </select>
      </Field>
      <p className="oc-field-hint" style={{ marginTop: -6 }}>
        {PERMISSION_HINTS[settings.defaultPermissionMode]} Plan mode is always read-only, whatever
        this is set to.
      </p>

      <div className="oc-group-label" style={{ padding: "18px 0 7px" }}>
        Per project
      </div>
      {projects.length === 0 ? (
        <p className="oc-field-hint">No projects yet.</p>
      ) : (
        projects.map((p) => (
          <div key={p.id} className="oc-row" style={{ gap: 10, padding: "7px 0" }}>
            <span style={{ flex: 1, fontSize: "0.9em" }}>{p.name}</span>
            <select
              className="oc-select"
              style={{ width: 190 }}
              value={p.permissionMode}
              onChange={(e) =>
                void useChat
                  .getState()
                  .updateProject({ ...p, permissionMode: e.target.value as PermissionMode })
                  .catch(() => toast("error", "Could not save"))
              }
            >
              {(Object.keys(PERMISSION_LABELS) as PermissionMode[]).map((m) => (
                <option key={m} value={m}>
                  {PERMISSION_LABELS[m]}
                </option>
              ))}
            </select>
          </div>
        ))
      )}

      <div className="oc-group-label" style={{ padding: "18px 0 7px" }}>
        Rules
      </div>
      <p className="oc-field-hint" style={{ marginTop: 0 }}>
        Allow or deny specific tools, e.g. <code>bash:npm test</code> or <code>write:src/**</code>.
        A deny always wins. Rule editing lands with the permissions milestone.
      </p>
    </>
  );
}

// ----------------------------------------------------------------- shortcuts

const SHORTCUT_LABELS: Record<string, string> = {
  commandPalette: "Command palette",
  newChat: "New chat",
  toggleSidebar: "Toggle sidebar",
  toggleMode: "Toggle plan / build",
  search: "Search chats",
  settings: "Open settings",
  send: "Send message",
  stop: "Stop the run",
  newLine: "Insert newline",
  toggleTerminal: "Toggle terminal",
  toggleArtifacts: "Toggle artifacts",
};

function ShortcutsTab() {
  const settings = useChat((s) => s.settings);
  const save = useChat((s) => s.saveSettings);
  const [recording, setRecording] = useState<string | null>(null);

  const capture = (cmd: string, e: React.KeyboardEvent) => {
    e.preventDefault();
    const parts: string[] = [];
    if (e.ctrlKey || e.metaKey) parts.push("mod");
    if (e.shiftKey) parts.push("shift");
    if (e.altKey) parts.push("alt");
    const key = e.key.length === 1 ? e.key.toLowerCase() : e.key.toLowerCase();
    if (["control", "meta", "shift", "alt"].includes(key)) return;
    parts.push(key);
    void save({ keybinds: { ...settings.keybinds, [cmd]: parts.join("+") } });
    setRecording(null);
  };

  return (
    <>
      <p className="oc-field-hint" style={{ marginTop: 0 }}>
        Click a row, then press the keys you want. <kbd>mod</kbd> is Ctrl on Windows and Linux,
        Command on macOS.
      </p>
      {Object.entries(SHORTCUT_LABELS).map(([cmd, label]) => (
        <div key={cmd} className="oc-row" style={{ gap: 10, padding: "7px 0" }}>
          <span style={{ flex: 1, fontSize: "0.92em" }}>{label}</span>
          <button
            className="oc-btn"
            data-size="sm"
            onClick={() => setRecording(recording === cmd ? null : cmd)}
            onKeyDown={recording === cmd ? (e) => capture(cmd, e) : undefined}
            style={{ minWidth: 130, justifyContent: "center" }}
          >
            {recording === cmd ? (
              <span className="oc-muted">press keys…</span>
            ) : (
              <kbd>{settings.keybinds[cmd] ?? "—"}</kbd>
            )}
          </button>
        </div>
      ))}
    </>
  );
}

// --------------------------------------------------------------------- about

function AboutTab() {
  const [paths, setPaths] = useState<import("../lib/ipc").AppPaths | null>(null);
  const [storage, setStorage] = useState<{ databaseBytes: number; databasePath: string } | null>(
    null,
  );
  const [version, setVersion] = useState("");

  useEffect(() => {
    void api.appPaths().then(setPaths).catch(() => setPaths(null));
    void api.storageInfo().then(setStorage).catch(() => setStorage(null));
    void api.ping().then(setVersion).catch(() => setVersion("backend unreachable"));
  }, []);

  return (
    <>
      <p style={{ fontSize: "0.94em", marginTop: 0 }}>
        <strong>Octopus Chat</strong> — a local-first agent desktop app.
      </p>
      <p className="oc-field-hint">Backend: {version}</p>
      <p className="oc-field-hint">
        Conversations, settings and keys live only on this machine, in a single SQLite file.
      </p>

      <div className="oc-group-label" style={{ padding: "16px 0 7px" }}>
        Storage
      </div>
      {storage ? (
        <div className="oc-row" style={{ gap: 8, marginBottom: 6 }}>
          <span className="oc-faint oc-mono" style={{ flex: 1, fontSize: "0.78em" }}>
            {storage.databasePath}
          </span>
          <Badge>{(storage.databaseBytes / 1024).toFixed(0)} KB</Badge>
        </div>
      ) : null}
      {paths
        ? Object.entries(paths).map(([k, v]) => (
            <div key={k} className="oc-row" style={{ gap: 8, padding: "3px 0" }}>
              <span className="oc-faint" style={{ width: 96, fontSize: "0.82em" }}>
                {k}
              </span>
              <span className="oc-mono oc-truncate" style={{ flex: 1, fontSize: "0.78em" }}>
                {v}
              </span>
            </div>
          ))
        : null}
    </>
  );
}

// ----------------------------------------------------------- new project flow

export function NewProjectDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const createProject = useChat((s) => s.createProject);
  const settings = useChat((s) => s.settings);
  const [roots, setRoots] = useState<string[]>([]);
  const [typed, setTyped] = useState("");
  const [name, setName] = useState("");
  const [instructions, setInstructions] = useState("");
  const [busy, setBusy] = useState(false);
  const [picking, setPicking] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (open) {
      setRoots([]);
      setTyped("");
      setName("");
      setInstructions("");
      setError(null);
    }
  }, [open]);

  const suggestName = (path: string) => {
    setName((current) => {
      if (current.trim()) return current;
      const leaf = path.split(/[\\/]/).filter(Boolean).pop();
      return leaf ?? current;
    });
  };

  const addPaths = (paths: string[]) => {
    if (paths.length === 0) return;
    setRoots((prev) => [...prev, ...paths.filter((p) => !prev.includes(p))]);
    suggestName(paths[0]);
    setError(null);
  };

  const pick = async () => {
    setPicking(true);
    setError(null);
    try {
      const picked = await api.pickProjectDirs();
      if (picked && picked.length > 0) addPaths(picked);
    } catch (e) {
      // Surfaced in the dialog rather than as a toast: the user is looking at
      // this window, and a toast can expire before it is read.
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setPicking(false);
    }
  };

  // The picker is a convenience. A typed path always works, which means a
  // broken or unavailable native dialog can never block project creation.
  const addTyped = async () => {
    const candidate = typed.trim();
    if (!candidate) return;
    setError(null);
    try {
      const canonical = await api.validateProjectDir(candidate);
      addPaths([canonical]);
      setTyped("");
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const create = async () => {
    setBusy(true);
    setError(null);
    try {
      await createProject(name.trim(), roots);
      if (instructions.trim()) {
        // Stored on the project so the agent reads it every request.
        const projects = useChat.getState().projects;
        const created = projects[0];
        if (created) {
          await useChat
            .getState()
            .updateProject({ ...created, instructions: instructions.trim() });
        }
      }
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog
      open={open}
      title="New project"
      onClose={onClose}
      width={560}
      footer={
        <>
          <Button
            variant="primary"
            disabled={busy || roots.length === 0 || !name.trim()}
            onClick={() => void create()}
          >
            Create project
          </Button>
          <div className="oc-spacer" />
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
        </>
      }
    >
      {error ? (
        <div className="oc-inline-error" role="alert">
          {error}
        </div>
      ) : null}

      <Field label="Name">
        <input
          className="oc-input"
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="My project"
        />
      </Field>

      <Field
        label="Folders"
        hint="One or more. The agent may only read and write inside these. Browse for them, or paste a path."
      >
        <div className="oc-col" style={{ gap: 6 }}>
          {roots.map((r) => (
            <div key={r} className="oc-root-row">
              <FolderOpen size={13} className="oc-faint oc-flex-none" />
              <span className="oc-mono oc-truncate" style={{ flex: 1, fontSize: "0.82em" }}>
                {r}
              </span>
              <Button
                size="icon"
                variant="ghost"
                title="Remove"
                onClick={() => setRoots(roots.filter((x) => x !== r))}
              >
                <Trash2 size={12} />
              </Button>
            </div>
          ))}

          <div className="oc-root-picker">
            <input
              className="oc-input oc-mono"
              value={typed}
              onChange={(e) => setTyped(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  void addTyped();
                }
              }}
              placeholder="C:\path\to\folder"
              spellCheck={false}
              style={{ flex: 1, fontSize: "0.85em" }}
            />
            <Button size="sm" onClick={() => void addTyped()} disabled={!typed.trim()}>
              Add
            </Button>
            <Button size="sm" onClick={() => void pick()} disabled={picking}>
              {picking ? <span className="oc-spinner" /> : <FolderOpen size={13} />}
              {roots.length ? "Browse…" : "Browse…"}
            </Button>
          </div>
        </div>
      </Field>

      <Field
        label="Project instructions"
        hint={`Read on every request in this project. An AGENTS.md in the folder is picked up automatically. Ignored for now: ${settings.ignoreGlobs.slice(0, 4).join(", ")}.`}
      >
        <textarea
          className="oc-textarea"
          value={instructions}
          onChange={(e) => setInstructions(e.target.value)}
          placeholder="- Always run the tests before reporting done."
          style={{ minHeight: 110 }}
        />
      </Field>
    </Dialog>
  );
}
