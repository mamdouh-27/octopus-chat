import { DEFAULT_THEME, findTheme, THEMES } from "./registry";
import { TOKEN_NAMES, resolveToken, type Theme } from "./types";

/**
 * Applies a theme by writing CSS custom properties onto the document root.
 *
 * Nothing in React re-renders when the theme changes: the variables live on
 * `:root`, so the browser handles the repaint. This is what keeps switching
 * themes instant even with a long transcript on screen.
 */

const ROOT_STYLE_ID = "octopus-theme-vars";

export type Scheme = "dark" | "light";

function prefersDark(): boolean {
  return window.matchMedia?.("(prefers-color-scheme: dark)").matches ?? true;
}

export function currentScheme(): Scheme {
  const attr = document.documentElement.dataset.scheme;
  return attr === "light" ? "light" : "dark";
}

function styleElement(): HTMLStyleElement {
  let el = document.getElementById(ROOT_STYLE_ID) as HTMLStyleElement | null;
  if (!el) {
    el = document.createElement("style");
    el.id = ROOT_STYLE_ID;
    document.head.appendChild(el);
  }
  return el;
}

/**
 * Renders the token block. `color-scheme` is emitted alongside the colours so
 * native scrollbars, form controls and the caret match the theme.
 */
export function themeCss(theme: Theme, scheme: Scheme, highContrast: boolean): string {
  const lines: string[] = [":root {"];

  if (highContrast) {
    // Raising border and muted-text contrast is cheaper and more predictable
    // than shipping a second palette for every theme.
    lines.push(`--hc-boost: 1;`);
  } else {
    lines.push(`--hc-boost: 0;`);
  }

  for (const name of TOKEN_NAMES) {
    const value = resolveToken(theme.tokens[name], scheme);
    lines.push(`  --oc-${kebab(name)}: ${value};`);
  }

  lines.push(`  color-scheme: ${scheme};`);
  lines.push("}");
  return lines.join("\n");
}

function kebab(s: string): string {
  return s.replace(/[A-Z]/g, (m) => `-${m.toLowerCase()}`);
}

export interface ApplyOptions {
  /** `null` follows the operating system. */
  appearance?: string | null;
  highContrast?: boolean;
}

export function applyTheme(themeId: string, opts: ApplyOptions = {}): Theme {
  const theme = findTheme(themeId) ?? findTheme(DEFAULT_THEME) ?? THEMES[0];
  const scheme: Scheme =
    opts.appearance === "light" || opts.appearance === "dark"
      ? opts.appearance
      : theme.appearance;

  const root = document.documentElement;
  root.dataset.scheme = scheme;
  root.dataset.theme = theme.id;
  root.classList.toggle("oc-reduce-motion", false);
  styleElement().textContent = themeCss(theme, scheme, opts.highContrast ?? false);
  return theme;
}

/** Follows the OS setting while the user has not pinned an appearance. */
export function watchSystemAppearance(onChange: (dark: boolean) => void): () => void {
  const mq = window.matchMedia?.("(prefers-color-scheme: dark)");
  if (!mq) return () => {};
  const handler = (e: MediaQueryListEvent) => onChange(e.matches);
  mq.addEventListener("change", handler);
  return () => mq.removeEventListener("change", handler);
}

export function systemScheme(): Scheme {
  return prefersDark() ? "dark" : "light";
}

/** Themes matching the active scheme, for the picker. */
export function themesForScheme(scheme: Scheme): Theme[] {
  return THEMES.filter((t) => t.appearance === scheme);
}

/** Nearest theme of the requested scheme, used when switching appearance. */
export function themeForScheme(themeId: string, scheme: Scheme): string {
  const current = findTheme(themeId);
  if (current?.appearance === scheme) return themeId;
  const candidates = THEMES.filter((t) => t.appearance === scheme);
  const preferred = candidates.find((t) => t.id.startsWith("octopus-"));
  return preferred?.id ?? candidates[0]?.id ?? themeId;
}