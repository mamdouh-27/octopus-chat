import type { Settings } from "../lib/types";

/** Used until the real settings load, so the first paint is never unstyled. */
export const DEFAULT_SETTINGS: Settings = {
  theme: "octopus-dark",
  appearance: null,
  fontFamily: "Inter",
  fontSize: 14,
  uiScale: 1,
  codeFontFamily: "JetBrains Mono",
  lineHeight: 1.6,
  reduceMotion: false,
  highContrast: false,
  keybinds: {
    commandPalette: "mod+k",
    newChat: "mod+n",
    toggleSidebar: "mod+b",
    toggleMode: "mod+shift+m",
    search: "mod+f",
    settings: "mod+,",
    send: "mod+enter",
    stop: "escape",
    newLine: "shift+enter",
    toggleTerminal: "mod+`",
    toggleArtifacts: "mod+shift+a",
  },
  defaultProvider: "ollama",
  defaultModel: "",
  defaultPermissionMode: "ask",
  sendOnEnter: true,
  autoTitle: true,
  streamMarkdown: true,
  showReasoning: true,
  showTokenCost: true,
  preventSleepWhileRunning: true,
  notifyOnComplete: true,
  notifyOnInputNeeded: true,
  localOnly: false,
  telemetry: false,
  ignoreGlobs: [
    "node_modules",
    "target",
    "dist",
    "build",
    ".git",
    ".venv",
    "__pycache__",
    ".next",
    ".cache",
  ],
  maxContextMessages: 200,
  compactThresholdPercent: 85,
};

export const UI_FONT_STACKS: Record<string, string> = {
  Inter: `'Inter', 'Segoe UI Variable Text', 'Segoe UI', system-ui, -apple-system, sans-serif`,
  "Segoe UI": `'Segoe UI Variable Text', 'Segoe UI', system-ui, sans-serif`,
  "IBM Plex Sans": `'IBM Plex Sans', 'Segoe UI', system-ui, sans-serif`,
  "JetBrains Sans": `'JetBrains Sans', 'Inter', system-ui, sans-serif`,
  System: `system-ui, -apple-system, 'Segoe UI', sans-serif`,
  "IBM Plex Mono": `'IBM Plex Mono', ui-monospace, monospace`,
};

export const CODE_FONT_STACKS: Record<string, string> = {
  "JetBrains Mono": `'JetBrains Mono', 'Cascadia Code', ui-monospace, 'SF Mono', Menlo, monospace`,
  "Cascadia Code": `'Cascadia Code', 'JetBrains Mono', ui-monospace, Consolas, monospace`,
  "IBM Plex Mono": `'IBM Plex Mono', ui-monospace, Consolas, monospace`,
  "SF Mono": `ui-monospace, 'SF Mono', Menlo, Consolas, monospace`,
  Consolas: `Consolas, 'Cascadia Code', ui-monospace, monospace`,
  "Fira Code": `'Fira Code', ui-monospace, Consolas, monospace`,
};

/**
 * Non-colour chrome the theme engine does not own: fonts, sizes and motion.
 * Kept separate from the token block so themes only ever describe colour.
 */
export function chromeCss(s: Settings): string {
  const ui = UI_FONT_STACKS[s.fontFamily] ?? UI_FONT_STACKS.System;
  const code = CODE_FONT_STACKS[s.codeFontFamily] ?? CODE_FONT_STACKS["JetBrains Mono"];
  return `:root {
  --oc-font-ui: ${ui};
  --oc-font-code: ${code};
  --oc-font-size: ${s.fontSize};
  --oc-line-height: ${s.lineHeight};
  --oc-ui-scale: ${s.uiScale};
  --oc-motion: ${s.reduceMotion ? "0" : "1"};
}`;
}