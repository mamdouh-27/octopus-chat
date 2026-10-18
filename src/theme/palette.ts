import type { Theme, ThemeTokens } from "./types";

/**
 * A compact colour palette. `buildTokens` expands it into the full semantic
 * token set.
 *
 * Derived colours are computed as *solid hex values* rather than `color-mix()`
 * expressions. Mixing produces a wash of the same hue, which reads as a gradient
 * and makes hover and selected states nearly invisible; stepping lightness in
 * OKLCH keeps the hue and gives genuinely distinct, flat colours.
 */
export interface SyntaxPalette {
  comment: string;
  keyword: string;
  fn: string;
  variable: string;
  string: string;
  number: string;
  type: string;
  operator: string;
  punctuation: string;
  tag: string;
  attribute: string;
  constant: string;
  property: string;
}

export interface Palette {
  id: string;
  name: string;
  author: string;
  appearance: "dark" | "light";
  /** Accent used for the running border and focus glow. */
  primary: string;
  accent: string;
  bg: string;
  panel: string;
  elevated: string;
  overlay: string;
  input: string;
  text: string;
  textMuted: string;
  textFaint: string;
  success: string;
  warning: string;
  danger: string;
  info: string;
  border: string;
  borderStrong: string;
  /** Background wash applied while a session is in Plan mode. */
  planTint: string;
  syntax: SyntaxPalette;
}

// ------------------------------------------------------------------ colour

interface Oklch {
  l: number;
  c: number;
  h: number;
}

function clamp(v: number, lo: number, hi: number): number {
  return v < lo ? lo : v > hi ? hi : v;
}

function parseHex(hex: string): { r: number; g: number; b: number } {
  let h = hex.trim().replace("#", "");
  if (h.length === 3) h = h.split("").map((c) => c + c).join("");
  const n = parseInt(h, 16);
  return { r: (n >> 16) & 255, g: (n >> 8) & 255, b: n & 255 };
}

function toOklch(hex: string): Oklch {
  const { r, g, b } = parseHex(hex);
  const lin = (v: number) => {
    const s = v / 255;
    return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  const R = lin(r);
  const G = lin(g);
  const B = lin(b);

  const l = Math.cbrt(0.4122214708 * R + 0.5363325363 * G + 0.0514459929 * B);
  const m = Math.cbrt(0.2119034982 * R + 0.6806995451 * G + 0.1073969566 * B);
  const s = Math.cbrt(0.0883024619 * R + 0.2817188376 * G + 0.6299787005 * B);

  const L = 0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s;
  const A = 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s;
  const Bb = 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s;

  const c = Math.sqrt(A * A + Bb * Bb);
  let h = (Math.atan2(Bb, A) * 180) / Math.PI;
  if (h < 0) h += 360;
  return { l: L, c, h };
}

function fromOklch({ l, c, h }: Oklch): string {
  const hr = (h * Math.PI) / 180;
  const A = c * Math.cos(hr);
  const B = c * Math.sin(hr);

  const l_ = l + 0.3963377774 * A + 0.2158037573 * B;
  const m_ = l - 0.1055613458 * A - 0.0638541728 * B;
  const s_ = l - 0.0894841775 * A - 1.291485548 * B;

  const L = l_ * l_ * l_;
  const M = m_ * m_ * m_;
  const S = s_ * s_ * s_;

  const r = 4.0767416621 * L - 3.3077115913 * M + 0.2309699292 * S;
  const g = -1.2684380046 * L + 2.6097574011 * M - 0.3413193965 * S;
  const b = -0.0041960863 * L - 0.7034186147 * M + 1.707614701 * S;

  const enc = (v: number) => {
    const c2 = v <= 0.0031308 ? v * 12.92 : 1.055 * v ** (1 / 2.4) - 0.055;
    return clamp(Math.round(c2 * 255), 0, 255);
  };
  return `#${[enc(r), enc(g), enc(b)].map((v) => v.toString(16).padStart(2, "0")).join("")}`;
}

/** Moves a colour's lightness by `delta`, keeping hue and chroma. */
export function shade(hex: string, delta: number): string {
  const c = toOklch(hex);
  return fromOklch({ l: clamp(c.l + delta, 0, 1), c: c.c, h: c.h });
}

/** Blends two colours in OKLCH and returns a flat hex value. */
export function blend(a: string, b: string, weight: number): string {
  const ca = toOklch(a);
  const cb = toOklch(b);
  const t = clamp(weight, 0, 1);
  return fromOklch({
    l: ca.l + (cb.l - ca.l) * t,
    c: ca.c + (cb.c - ca.c) * t,
    // Take the shorter arc so hues do not travel the long way round.
    h: ca.h + ((((cb.h - ca.h) % 360) + 540) % 360 - 180) * t,
  });
}

/**
 * A solid tint of `base` toward `tint`: `amount` is how far to move.
 * Used instead of `color-mix` so the result is a real colour, not an alpha wash.
 */
export function tint(base: string, tintColor: string, amount: number): string {
  return blend(base, tintColor, amount);
}

/** Fully opaque variant of a colour, for cases that need alpha-free output. */
export function opaque(hex: string, over: string): string {
  return blend(over, hex, 1);
}

export function buildTokens(p: Palette): ThemeTokens {
  const s = p.syntax;
  const dark = p.appearance === "dark";

  // Interaction steps need to be visible without a gradient: on dark surfaces
  // they step lighter, on light surfaces darker.
  const step = dark ? 0.045 : -0.035;
  const stepStrong = dark ? 0.085 : -0.065;

  return {
    bg: p.bg,
    bgPanel: p.panel,
    bgElevated: p.elevated,
    bgOverlay: p.overlay,
    bgInput: p.input,

    bgHover: shade(p.panel, step),
    bgActive: shade(p.panel, stepStrong),
    bgSelected: tint(p.panel, p.primary, dark ? 0.2 : 0.14),

    text: p.text,
    textMuted: p.textMuted,
    textFaint: p.textFaint,
    textInverse: p.bg,

    primary: p.primary,
    primaryHover: shade(p.primary, dark ? -0.05 : -0.04),
    primarySoft: tint(p.panel, p.primary, dark ? 0.16 : 0.12),
    accent: p.accent,

    success: p.success,
    warning: p.warning,
    danger: p.danger,
    info: p.info,

    border: p.border,
    borderStrong: p.borderStrong,
    borderSubtle: shade(p.border, dark ? -0.02 : 0.03),
    borderFocus: p.primary,

    diffAdded: p.success,
    diffAddedBg: tint(p.bg, p.success, dark ? 0.16 : 0.13),
    diffRemoved: p.danger,
    diffRemovedBg: tint(p.bg, p.danger, dark ? 0.16 : 0.13),
    diffContext: p.textFaint,
    diffHunk: p.info,
    diffHighlight: tint(p.bg, p.primary, dark ? 0.2 : 0.14),

    markdownHeading: shade(p.primary, dark ? 0.04 : -0.06),
    markdownLink: p.info,
    markdownCode: shade(p.accent, dark ? 0.06 : -0.1),
    markdownCodeBg: shade(p.bg, step),
    markdownQuote: p.textFaint,
    markdownEmph: p.accent,
    markdownStrong: p.text,
    markdownList: p.primary,
    markdownRule: p.border,

    syntaxComment: s.comment,
    syntaxKeyword: s.keyword,
    syntaxFunction: s.fn,
    syntaxVariable: s.variable,
    syntaxString: s.string,
    syntaxNumber: s.number,
    syntaxType: s.type,
    syntaxOperator: s.operator,
    syntaxPunctuation: s.punctuation,
    syntaxTag: s.tag,
    syntaxAttribute: s.attribute,
    syntaxConstant: s.constant,
    syntaxProperty: s.property,

    planTint: tint(p.bg, p.primary, dark ? 0.07 : 0.05),
  };
}

export function buildTheme(p: Palette): Theme {
  return {
    id: p.id,
    name: p.name,
    author: p.author,
    appearance: p.appearance,
    tokens: buildTokens(p),
    swatch: [p.primary, p.accent],
  };
}