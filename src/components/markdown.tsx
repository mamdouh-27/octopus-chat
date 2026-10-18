import { memo, useMemo } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { useHighlight } from "../lib/highlight";
import { CopyButton } from "./diff";

/**
 * Markdown renderer.
 *
 * `react-markdown` never enables raw HTML, so model output cannot inject markup;
 * that is the reason this is a component tree rather than
 * `dangerouslySetInnerHTML`.
 *
 * `dir="auto"` lets the browser pick the paragraph direction from the first
 * strong character, which is what makes mixed Arabic/Latin replies read
 * correctly without the whole app having to be mirrored. Code is exempt: source
 * is always left to right, and letting it flip produces mangled punctuation.
 */
export const Markdown = memo(function Markdown({ text }: { text: string }) {
  return (
    <div className="oc-md" dir="auto">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          a: ({ href, children }) => (
            <a href={href} target="_blank" rel="noreferrer noopener">
              {children}
            </a>
          ),
          pre: ({ children }) => <>{children}</>,
          code: ({ className, children }) => {
            const raw = String(children ?? "");
            const lang = /language-(\w+)/.exec(className ?? "")?.[1] ?? "";
            // Inline code has no language class and no trailing newline.
            if (!lang && !raw.includes("\n")) {
              return (
                <code dir="ltr" className="oc-inline-code">
                  {children}
                </code>
              );
            }
            return <CodeBlock code={raw.replace(/\n$/, "")} lang={lang} />;
          },
          table: ({ children }) => (
            <div style={{ overflowX: "auto" }}>
              <table>{children}</table>
            </div>
          ),
        }}
      >
        {text}
      </ReactMarkdown>
    </div>
  );
});

function CodeBlock({ code, lang }: { code: string; lang: string }) {
  const { html, ready } = useHighlight(code, lang);
  return (
    <div className="oc-code" dir="ltr">
      <div className="oc-code-head">
        <span>{lang || "text"}</span>
        <div className="oc-code-actions">
          <CopyButton text={code} />
        </div>
      </div>
      {ready && html ? (
        <pre dangerouslySetInnerHTML={{ __html: html }} />
      ) : (
        <pre>
          <code>{code}</code>
        </pre>
      )}
    </div>
  );
}

/** Fenced-block renderer for values the agent did not produce as chat text. */
export const RawBlock = memo(function RawBlock({
  text,
  lang = "",
  maxHeight,
}: {
  text: string;
  lang?: string;
  maxHeight?: number;
}) {
  const { html, ready } = useHighlight(text, lang);
  const body = useMemo(
    () =>
      ready && html ? (
        <pre dangerouslySetInnerHTML={{ __html: html }} />
      ) : (
        <pre>
          <code>{text}</code>
        </pre>
      ),
    [ready, html, text],
  );
  return (
    <div className="oc-code" dir="ltr" style={maxHeight ? { maxHeight, overflow: "auto" } : undefined}>
      <div className="oc-code-head">
        <span>{lang || "text"}</span>
        <div className="oc-code-actions">
          <CopyButton text={text} />
        </div>
      </div>
      {body}
    </div>
  );
});