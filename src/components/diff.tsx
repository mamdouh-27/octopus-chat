/** Renders a unified diff, or falls back to plain monospace text. */
import { useMemo, useState } from "react";
import { ChevronRight, Copy, Check } from "lucide-react";
import { cx } from "./ui";

interface Line {
  kind: "add" | "del" | "ctx" | "hunk" | "meta";
  text: string;
  oldNo?: number;
  newNo?: number;
}

export function DiffView({ diff }: { diff: string }) {
  const lines = useMemo(() => parseDiff(diff), [diff]);
  return (
    <div className="oc-diff">
      {lines.map((l, i) => (
        <div key={i} className="oc-diff-line" data-sign={sign(l.kind)}>
          {l.kind === "hunk" ? (
            <div className="oc-diff-hunk oc-truncate">{l.text}</div>
          ) : (
            <>
              <span className="oc-diff-num">{l.oldNo ?? ""}</span>
              <span className="oc-diff-num">{l.newNo ?? ""}</span>
              <span className="oc-diff-text">{l.text}</span>
            </>
          )}
        </div>
      ))}
    </div>
  );
}

function sign(kind: Line["kind"]): string {
  if (kind === "add") return "+";
  if (kind === "del") return "-";
  return " ";
}

function parseDiff(diff: string): Line[] {
  const out: Line[] = [];
  let oldNo = 0;
  let newNo = 0;
  for (const raw of diff.split("\n")) {
    if (raw.startsWith("--- ") || raw.startsWith("+++ ")) {
      out.push({ kind: "meta", text: raw });
      continue;
    }
    if (raw.startsWith("@@")) {
      const m = /@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@/.exec(raw);
      if (m) {
        // A zero-length side means the change sits after this line.
        oldNo = m[2] === "0" ? Number(m[1]) : Number(m[1]);
        newNo = m[4] === "0" ? Number(m[3]) : Number(m[3]);
      }
      out.push({ kind: "hunk", text: raw });
      continue;
    }
    const c = raw[0];
    const body = raw.slice(1);
    if (c === "+") {
      out.push({ kind: "add", text: body, newNo: newNo++ });
    } else if (c === "-") {
      out.push({ kind: "del", text: body, oldNo: oldNo++ });
    } else {
      out.push({ kind: "ctx", text: body, oldNo: oldNo++, newNo: newNo++ });
    }
  }
  return out;
}

/** Collapsible block used for tool calls and reasoning. */
export function Collapsible({
  label,
  icon,
  children,
  defaultOpen = false,
  tone,
}: {
  label: React.ReactNode;
  icon?: React.ReactNode;
  children: React.ReactNode;
  defaultOpen?: boolean;
  tone?: string;
}) {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <div className="oc-tool" data-state={tone}>
      <button className="oc-tool-head" onClick={() => setOpen((v) => !v)}>
        <ChevronRight
          size={13}
          style={{
            transform: open ? "rotate(90deg)" : "none",
            transition: "transform 120ms ease",
            flex: "0 0 auto",
          }}
        />
        {icon}
        <span className="oc-tool-summary">{label}</span>
      </button>
      {open ? children : null}
    </div>
  );
}

export function CopyButton({ text, label = "Copy" }: { text: string; label?: string }) {
  const [done, setDone] = useState(false);
  return (
    <button
      className="oc-btn"
      data-size="sm"
      data-variant="ghost"
      title={label}
      onClick={async () => {
        try {
          await navigator.clipboard.writeText(text);
          setDone(true);
          setTimeout(() => setDone(false), 1400);
        } catch {
          /* clipboard may be unavailable; nothing useful to say here */
        }
      }}
    >
      {done ? <Check size={12} /> : <Copy size={12} />}
    </button>
  );
}

export function Spinner2({ className }: { className?: string }) {
  return <span className={cx("oc-spinner", className)} />;
}