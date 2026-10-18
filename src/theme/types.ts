/**
 * Theme contract.
 *
 * Every theme supplies the same token set. A value is either a hex string, an
 * `oklch()` string, or `{ dark, light }` so one theme can serve both schemes.
 * The engine resolves the pair and writes CSS custom properties onto
 * `:root`, which is what makes swapping a theme a single style recalculation
 * with no React re-render.
 */

export type ColorValue = string | { dark: string; light: string };

export interface ThemeTokens {
  // Surfaces, from furthest back to closest.
  bg: ColorValue;
  bgPanel: ColorValue;
  bgElevated: ColorValue;
  bgOverlay: ColorValue;
  bgInput: ColorValue;
  bgHover: ColorValue;
  bgActive: ColorValue;
  bgSelected: ColorValue;

  // Text.
  text: ColorValue;
  textMuted: ColorValue;
  textFaint: ColorValue;
  textInverse: ColorValue;

  // Brand and status.
  primary: ColorValue;
  primaryHover: ColorValue;
  primarySoft: ColorValue;
  accent: ColorValue;
  success: ColorValue;
  warning: ColorValue;
  danger: ColorValue;
  info: ColorValue;

  // Lines.
  border: ColorValue;
  borderStrong: ColorValue;
  borderSubtle: ColorValue;
  borderFocus: ColorValue;

  // Diff.
  diffAdded: ColorValue;
  diffAddedBg: ColorValue;
  diffRemoved: ColorValue;
  diffRemovedBg: ColorValue;
  diffContext: ColorValue;
  diffHunk: ColorValue;
  diffHighlight: ColorValue;

  // Markdown.
  markdownHeading: ColorValue;
  markdownLink: ColorValue;
  markdownCode: ColorValue;
  markdownCodeBg: ColorValue;
  markdownQuote: ColorValue;
  markdownEmph: ColorValue;
  markdownStrong: ColorValue;
  markdownList: ColorValue;
  markdownRule: ColorValue;

  // Syntax, used by the highlighter.
  syntaxComment: ColorValue;
  syntaxKeyword: ColorValue;
  syntaxFunction: ColorValue;
  syntaxVariable: ColorValue;
  syntaxString: ColorValue;
  syntaxNumber: ColorValue;
  syntaxType: ColorValue;
  syntaxOperator: ColorValue;
  syntaxPunctuation: ColorValue;
  syntaxTag: ColorValue;
  syntaxAttribute: ColorValue;
  syntaxConstant: ColorValue;
  syntaxProperty: ColorValue;

  // Plan mode tints the whole surface so the state is unmissable.
  planTint: ColorValue;
}

export interface Theme {
  id: string;
  name: string;
  author: string;
  /** `dark` and `light` entries both need a complete token set. */
  appearance: "dark" | "light";
  tokens: ThemeTokens;
  /** Two colours used for the theme swatch in the picker. */
  swatch: [string, string];
}

/** Every key a theme must define, derived from the type so it cannot drift. */
export const TOKEN_NAMES = [
  "bg", "bgPanel", "bgElevated", "bgOverlay", "bgInput", "bgHover", "bgActive",
  "bgSelected", "text", "textMuted", "textFaint", "textInverse", "primary",
  "primaryHover", "primarySoft", "accent", "success", "warning", "danger",
  "info", "border", "borderStrong", "borderSubtle", "borderFocus", "diffAdded",
  "diffAddedBg", "diffRemoved", "diffRemovedBg", "diffContext", "diffHunk",
  "diffHighlight", "markdownHeading", "markdownLink", "markdownCode",
  "markdownCodeBg", "markdownQuote", "markdownEmph", "markdownStrong",
  "markdownList", "markdownRule", "syntaxComment", "syntaxKeyword",
  "syntaxFunction", "syntaxVariable", "syntaxString", "syntaxNumber",
  "syntaxType", "syntaxOperator", "syntaxPunctuation", "syntaxTag",
  "syntaxAttribute", "syntaxConstant", "syntaxProperty", "planTint",
] as const satisfies readonly (keyof ThemeTokens)[];

export type TokenName = (typeof TOKEN_NAMES)[number];

export function resolveToken(value: ColorValue, scheme: "dark" | "light"): string {
  if (typeof value === "string") return value;
  return value[scheme];
}