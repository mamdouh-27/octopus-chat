import { createHighlighterCore, type HighlighterCore } from "shiki/core";
import { createJavaScriptRegexEngine } from "@shikijs/engine-javascript";
import type { ThemeRegistrationRaw } from "@shikijs/types";
import { useEffect, useMemo, useState } from "react";

/**
 * Syntax highlighting.
 *
 * Three decisions keep this light:
 *
 * 1. Fine-grained imports. Shiki's main entry bundles every language it knows
 *    (about 6 MB across hundreds of chunks). Importing each grammar from
 *    `@shikijs/langs/*` makes them separate chunks that are only downloaded when
 *    a transcript actually contains that language.
 *
 * 2. The JavaScript regex engine instead of the default Oniguruma WASM, which
 *    removes a ~600 KB wasm chunk. Some grammars that rely on advanced regex
 *    features degrade, so the engine runs in forgiving mode.
 *
 * 3. A hand-written theme whose colours are CSS custom properties. Code blocks
 *    then follow the Octopus theme the user picked instead of a colour scheme
 *    baked into the highlighter.
 */

const v = (token: string) => `var(--oc-${token})`;

/** Shiki theme that emits `var(--oc-*)` so themes stay in control. */
const OCTOPUS_THEME: ThemeRegistrationRaw = {
  name: "octopus",
  type: "dark",
  colors: {
    "editor.background": "transparent",
    "editor.foreground": v("text"),
  },
  settings: [
    { settings: { foreground: v("text") } },
    {
      scope: ["comment", "punctuation.definition.comment", "string.comment"],
      settings: { foreground: v("syntax-comment"), fontStyle: "italic" },
    },
    {
      scope: [
        "keyword", "storage", "storage.type", "keyword.control", "keyword.operator",
        "punctuation.separator",
      ],
      settings: { foreground: v("syntax-keyword") },
    },
    {
      scope: ["entity.name.function", "support.function", "meta.function-call", "variable.function"],
      settings: { foreground: v("syntax-function") },
    },
    {
      scope: ["variable", "variable.other", "variable.parameter", "meta.definition.variable.name"],
      settings: { foreground: v("syntax-variable") },
    },
    {
      scope: ["string", "string.quoted", "punctuation.definition.string"],
      settings: { foreground: v("syntax-string") },
    },
    {
      scope: ["constant.numeric", "constant.language", "constant.character", "constant.other"],
      settings: { foreground: v("syntax-number") },
    },
    {
      scope: [
        "entity.name.type", "support.type", "support.class", "entity.other.inherited-class",
        "entity.name.namespace",
      ],
      settings: { foreground: v("syntax-type") },
    },
    {
      scope: ["keyword.operator", "punctuation.separator.operator"],
      settings: { foreground: v("syntax-operator") },
    },
    {
      scope: ["punctuation", "meta.brace", "meta.bracket"],
      settings: { foreground: v("syntax-punctuation") },
    },
    {
      scope: ["entity.name.tag", "meta.tag"],
      settings: { foreground: v("syntax-tag") },
    },
    {
      scope: ["entity.other.attribute-name"],
      settings: { foreground: v("syntax-attribute") },
    },
    {
      scope: ["support.constant", "variable.language", "meta.object-literal.key"],
      settings: { foreground: v("syntax-property") },
    },
    {
      scope: ["markup.underline.link", "string.other.link"],
      settings: { foreground: v("syntax-tag") },
    },
    {
      scope: ["invalid", "invalid.illegal"],
      settings: { foreground: v("danger") },
    },
  ],
};

const LANGS = [
  "rust",
  "typescript",
  "javascript",
  "tsx",
  "jsx",
  "json",
  "bash",
  "powershell",
  "python",
  "go",
  "sql",
  "yaml",
  "toml",
  "html",
  "css",
  "diff",
  "markdown",
] as const;

type Lang = (typeof LANGS)[number];

/** Resolved lazily so a grammar is only fetched when it appears. */
const LOADERS: Record<Lang, () => Promise<unknown>> = {
  rust: () => import("@shikijs/langs/rust"),
  typescript: () => import("@shikijs/langs/typescript"),
  javascript: () => import("@shikijs/langs/javascript"),
  tsx: () => import("@shikijs/langs/tsx"),
  jsx: () => import("@shikijs/langs/jsx"),
  json: () => import("@shikijs/langs/json"),
  bash: () => import("@shikijs/langs/bash"),
  powershell: () => import("@shikijs/langs/powershell"),
  python: () => import("@shikijs/langs/python"),
  go: () => import("@shikijs/langs/go"),
  sql: () => import("@shikijs/langs/sql"),
  yaml: () => import("@shikijs/langs/yaml"),
  toml: () => import("@shikijs/langs/toml"),
  html: () => import("@shikijs/langs/html"),
  css: () => import("@shikijs/langs/css"),
  diff: () => import("@shikijs/langs/diff"),
  markdown: () => import("@shikijs/langs/markdown"),
};

const ALIASES: Record<string, Lang> = {
  rs: "rust",
  ts: "typescript",
  js: "javascript",
  sh: "bash",
  shell: "bash",
  zsh: "bash",
  console: "bash",
  ps1: "powershell",
  py: "python",
  yml: "yaml",
  htm: "html",
  md: "markdown",
  patch: "diff",
};

export function normalizeLang(lang: string): Lang | null {
  const key = lang.trim().toLowerCase();
  const resolved = ALIASES[key] ?? key;
  return (LANGS as readonly string[]).includes(resolved) ? (resolved as Lang) : null;
}

let engine: Promise<HighlighterCore> | null = null;
const loaded = new Set<Lang>();

function ensureEngine(): Promise<HighlighterCore> {
  engine ??= createHighlighterCore({
    themes: [OCTOPUS_THEME],
    langs: [],
    engine: createJavaScriptRegexEngine({ forgiving: true }),
  });
  return engine;
}

async function ensureLang(lang: Lang): Promise<HighlighterCore | null> {
  try {
    const hl = await ensureEngine();
    if (!loaded.has(lang)) {
      const mod = (await LOADERS[lang]()) as { default: never };
      hl.loadLanguage(mod.default);
      loaded.add(lang);
    }
    return hl;
  } catch {
    // A missing grammar must never blank the transcript.
    return null;
  }
}

/** Result cache keyed by language and content, shared across the transcript. */
const cache = new Map<string, string>();
const CACHE_LIMIT = 400;

function cacheKey(lang: Lang, code: string): string {
  return `${lang}:${hash(code)}`;
}

function remember(key: string, html: string): void {
  if (cache.size >= CACHE_LIMIT) {
    const oldest = cache.keys().next().value;
    if (oldest !== undefined) cache.delete(oldest);
  }
  cache.set(key, html);
}

/**
 * Highlighted HTML, or an empty string while the grammar loads. Callers fall
 * back to plain text, so the first paint is never empty.
 */
export function useHighlight(
  code: string,
  lang: string,
): { html: string; ready: boolean } {
  const resolved = useMemo(() => normalizeLang(lang), [lang]);
  const key = resolved && code ? cacheKey(resolved, code) : "";
  const [html, setHtml] = useState(() => (key ? (cache.get(key) ?? "") : ""));
  const [ready, setReady] = useState(() => !key);

  useEffect(() => {
    if (!resolved || !code) {
      setHtml("");
      setReady(true);
      return;
    }
    const k = cacheKey(resolved, code);
    const hit = cache.get(k);
    if (hit !== undefined) {
      setHtml(hit);
      setReady(true);
      return;
    }
    let cancelled = false;
    setReady(false);
    void (async () => {
      const hl = await ensureLang(resolved);
      if (cancelled || !hl) {
        if (!cancelled) setReady(true);
        return;
      }
      try {
        const out = hl.codeToHtml(code, { lang: resolved, theme: "octopus" });
        if (cancelled) return;
        remember(k, out);
        setHtml(out);
      } catch {
        /* grammar cannot handle this input; plain text is the right fallback */
      } finally {
        if (!cancelled) setReady(true);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [key, code, resolved]);

  return { html, ready };
}

/** Stable, cheap string hash for cache keys. */
function hash(s: string): number {
  let h = 2166136261;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return h >>> 0;
}

export const SUPPORTED_LANGS = LANGS;
