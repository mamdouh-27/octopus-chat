import { useEffect, useLayoutEffect, useMemo, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { ChevronLeft, ChevronRight, X } from "lucide-react";

export function cx(...parts: (string | false | null | undefined)[]): string {
  return parts.filter(Boolean).join(" ");
}

export function Button({
  children,
  onClick,
  variant = "default",
  size = "default",
  disabled,
  title,
  type = "button",
}: {
  children: ReactNode;
  onClick?: () => void;
  variant?: "default" | "primary" | "ghost" | "danger";
  size?: "default" | "sm" | "icon";
  disabled?: boolean;
  title?: string;
  type?: "button" | "submit";
}) {
  return (
    <button
      type={type}
      className="oc-btn"
      data-variant={variant}
      data-size={size}
      onClick={onClick}
      disabled={disabled}
      title={title}
      aria-label={size === "icon" ? title : undefined}
    >
      {children}
    </button>
  );
}

export function Badge({
  children,
  tone = "default",
}: {
  children: ReactNode;
  tone?: "default" | "primary" | "success" | "warn" | "danger" | "info";
}) {
  return (
    <span className="oc-badge" data-tone={tone}>
      {children}
    </span>
  );
}

export function Switch({
  checked,
  onChange,
  label,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  label?: string;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      className="oc-switch"
      onClick={() => onChange(!checked)}
    />
  );
}

export function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: ReactNode;
  children: ReactNode;
}) {
  return (
    <label className="oc-field">
      <span className="oc-field-label">{label}</span>
      {children}
      {hint ? <span className="oc-field-hint">{hint}</span> : null}
    </label>
  );
}

export function EmptyState({
  icon,
  title,
  hint,
  action,
}: {
  icon?: ReactNode;
  title: string;
  hint?: string;
  action?: ReactNode;
}) {
  return (
    <div className="oc-empty">
      {icon}
      <div className="oc-empty-title">{title}</div>
      {hint ? <div className="oc-empty-hint">{hint}</div> : null}
      {action}
    </div>
  );
}

export function Spinner() {
  return <span className="oc-spinner" aria-hidden />;
}

/**
 * Full-size image viewer.
 *
 * Attached images are otherwise only ever seen as 72px thumbnails, which makes
 * it impossible to check a screenshot you just pasted before sending it. Escape
 * or a click on the backdrop closes; the arrows and Home/End step through the
 * set so a multi-image message can be reviewed in one pass.
 */
export function ImageLightbox({
  images,
  index,
  onIndex,
  onClose,
}: {
  images: { id: string; src: string; label: string }[];
  index: number;
  onIndex: (next: number) => void;
  onClose: () => void;
}) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        onClose();
      } else if (e.key === "ArrowRight" || e.key === "ArrowDown") {
        e.preventDefault();
        onIndex((index + 1) % images.length);
      } else if (e.key === "ArrowLeft" || e.key === "ArrowUp") {
        e.preventDefault();
        onIndex((index - 1 + images.length) % images.length);
      } else if (e.key === "Home") {
        e.preventDefault();
        onIndex(0);
      } else if (e.key === "End") {
        e.preventDefault();
        onIndex(images.length - 1);
      }
    };
    // Capture, so the viewer wins over anything the chat has bound.
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [index, images.length, onIndex, onClose]);

  if (images.length === 0) return null;
  const current = images[Math.min(Math.max(index, 0), images.length - 1)];

  return (
    <div
      className="oc-lightbox"
      role="dialog"
      aria-modal="true"
      aria-label={current.label}
      onMouseDown={onClose}
    >
      <div className="oc-lightbox-bar" onMouseDown={(e) => e.stopPropagation()}>
        <span className="oc-truncate oc-faint" title={current.label}>
          {current.label}
        </span>
        <div className="oc-spacer" />
        {images.length > 1 ? (
          <span className="oc-faint">
            {index + 1} / {images.length}
          </span>
        ) : null}
        <button
          className="oc-lightbox-btn"
          onClick={onClose}
          aria-label="Close preview"
          title="Close (Esc)"
        >
          <X size={16} />
        </button>
      </div>

      <div className="oc-lightbox-stage" onMouseDown={(e) => e.stopPropagation()}>
        {images.length > 1 ? (
          <button
            className="oc-lightbox-nav"
            data-side="prev"
            onClick={() => onIndex((index - 1 + images.length) % images.length)}
            aria-label="Previous image"
          >
            <ChevronLeft size={20} />
          </button>
        ) : null}

        <img className="oc-lightbox-img" src={current.src} alt={current.label} />

        {images.length > 1 ? (
          <button
            className="oc-lightbox-nav"
            data-side="next"
            onClick={() => onIndex((index + 1) % images.length)}
            aria-label="Next image"
          >
            <ChevronRight size={20} />
          </button>
        ) : null}
      </div>
    </div>
  );
}

/**
 * A floating panel anchored to an element and kept inside the window.
 *
 * The composer cannot be positioned by CSS alone: on an empty chat it sits in
 * the middle of the pane, so a menu that always opens upward has no room and
 * spills off the top of the window, and one aligned to a left edge near the
 * right of the composer spills off the side. So the panel is measured after
 * mount, opens on whichever side has room, and is clamped to the viewport.
 *
 * Dismissal is on an outside pointer press rather than `onMouseLeave`, because
 * moving the cursor from the panel back onto the button that opened it would
 * otherwise close it and leave the button unusable.
 */
export function Popover({
  anchor,
  onClose,
  wide,
  children,
}: {
  anchor: HTMLElement | null;
  onClose: () => void;
  wide?: boolean;
  children: ReactNode;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [style, setStyle] = useState<CSSProperties>({ visibility: "hidden" });
  const placed = useRef<CSSProperties>({});

  useLayoutEffect(() => {
    const place = () => {
      const el = ref.current;
      if (!el || !anchor) return;

      const box = el.getBoundingClientRect();
      const host = anchor.getBoundingClientRect();
      const gap = 6;
      const margin = 10;

      const roomAbove = Math.max(0, host.top - gap - margin);
      const roomBelow = Math.max(0, window.innerHeight - host.bottom - gap - margin);
      // Whichever side has more room wins; ties go above, which is where a menu
      // is expected when it hangs off a toolbar at the bottom of the window.
      const above = roomAbove >= box.height || roomAbove >= roomBelow;

      // The panel is capped to the side it opens on, and the position is derived
      // from the capped height so the two can never disagree.
      const cap = Math.max(160, above ? roomAbove : roomBelow);
      const height = Math.min(box.height, cap);

      const top = above
        ? Math.max(margin, host.top - height - gap)
        : Math.min(window.innerHeight - height - margin, host.bottom + gap);

      const left = Math.max(
        margin,
        Math.min(host.left, window.innerWidth - box.width - margin),
      );

      const next: CSSProperties = {
        position: "fixed",
        top: Math.round(top),
        left: Math.round(left),
        maxHeight: Math.round(cap),
        visibility: "visible",
      };

      // Skipping an unchanged update matters here: the size observer below calls
      // this again on every tick, and re-rendering each time would never settle.
      const prev = placed.current;
      if (
        prev.top === next.top &&
        prev.left === next.left &&
        prev.maxHeight === next.maxHeight
      ) {
        return;
      }
      placed.current = next;
      setStyle(next);
    };

    place();

    // The content changes size after mount: provider lists arrive asynchronously
    // and the model list grows as they load. Re-placing on size change keeps the
    // panel attached to the composer instead of where it was when it was empty.
    const el = ref.current;
    const observer = el && typeof ResizeObserver !== "undefined" ? new ResizeObserver(place) : null;
    if (el && observer) observer.observe(el);

    window.addEventListener("resize", place);
    return () => {
      observer?.disconnect();
      window.removeEventListener("resize", place);
    };
  }, [anchor]);

  useEffect(() => {
    const onDown = (e: PointerEvent) => {
      const el = ref.current;
      if (el && e.target instanceof Node && el.contains(e.target)) return;
      if (anchor && e.target instanceof Node && anchor.contains(e.target)) return;
      onClose();
    };
    // Deferred so the press that opened the panel does not immediately close it.
    const id = window.setTimeout(() => document.addEventListener("pointerdown", onDown), 0);
    return () => {
      window.clearTimeout(id);
      document.removeEventListener("pointerdown", onDown);
    };
  }, [anchor, onClose]);

  return (
    <div
      ref={ref}
      className={cx("oc-popmenu", wide && "oc-popmenu-wide")}
      role="menu"
      style={style}
    >
      {children}
    </div>
  );
}

export function Dialog({
  open,
  title,
  onClose,
  children,
  footer,
  width,
}: {
  open: boolean;
  title: string;
  onClose: () => void;
  children: ReactNode;
  footer?: ReactNode;
  width?: number;
}) {
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [open, onClose]);

  if (!open) return null;
  return (
    <div className="oc-scrim" onMouseDown={onClose}>
      <div
        className="oc-dialog"
        style={width ? { maxWidth: width } : undefined}
        onMouseDown={(e) => e.stopPropagation()}
        role="dialog"
        aria-modal="true"
        aria-label={title}
      >
        <div className="oc-dialog-head">
          <div className="oc-dialog-title">{title}</div>
          <div className="oc-spacer" />
          <Button size="icon" variant="ghost" onClick={onClose} title="Close">
            <X size={15} />
          </Button>
        </div>
        <div className="oc-dialog-body oc-scroll">{children}</div>
        {footer ? <div className="oc-dialog-foot">{footer}</div> : null}
      </div>
    </div>
  );
}

export interface MenuItem {
  label: string;
  icon?: ReactNode;
  shortcut?: string;
  danger?: boolean;
  disabled?: boolean;
  onSelect?: () => void;
}

export type MenuEntry =
  | MenuItem
  | { separator: true }
  | { header: string };

export function Menu({
  items,
  onClose,
  anchorRef,
}: {
  items: MenuEntry[];
  onClose: () => void;
  anchorRef?: React.RefObject<HTMLElement | null>;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [active, setActive] = useState(0);

  const selectable = useMemo(
    () => items.filter((i): i is MenuItem => !("separator" in i) && !("header" in i)),
    [items],
  );

  // Measured after mount so the real size is available, then flipped to stay
  // inside the viewport.
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    const anchor = anchorRef?.current;
    const anchorRect = anchor?.getBoundingClientRect();
    let left = anchorRect ? anchorRect.left : r.left;
    let top = anchorRect ? anchorRect.bottom + 6 : r.bottom + 6;
    left = Math.max(10, Math.min(left, window.innerWidth - r.width - 10));
    if (top + r.height > window.innerHeight - 10) {
      top = Math.max(10, window.innerHeight - r.height - 10);
    }
    el.style.left = `${left}px`;
    el.style.top = `${top}px`;
    el.focus();
  }, [anchorRef]);

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (selectable.length === 0) return;
      setActive((a) => (a + (e.key === "ArrowDown" ? 1 : -1) + selectable.length) % selectable.length);
    } else if (e.key === "Enter") {
      e.preventDefault();
      const item = selectable[active];
      if (item?.onSelect) {
        item.onSelect();
        onClose();
      }
    } else if (e.key === "Escape") {
      e.preventDefault();
      onClose();
    }
  };

  let selIndex = -1;
  return (
    <div
      ref={ref}
      className="oc-menu"
      role="menu"
      tabIndex={-1}
      onKeyDown={onKeyDown}
      style={{ position: "fixed" }}
    >
      {items.map((item, i) => {
        if ("separator" in item) return <div key={i} className="oc-menu-sep" />;
        if ("header" in item) {
          return (
            <div key={i} className="oc-menu-label">
              {item.header}
            </div>
          );
        }
        selIndex += 1;
        const index = selIndex;
        return (
          <button
            key={i}
            role="menuitem"
            className="oc-menu-item"
            data-active={index === active}
            data-danger={item.danger || undefined}
            disabled={item.disabled}
            onMouseEnter={() => setActive(index)}
            onClick={() => {
              item.onSelect?.();
              onClose();
            }}
          >
            {item.icon}
            <span style={{ flex: 1 }}>{item.label}</span>
            {item.shortcut ? <kbd>{item.shortcut}</kbd> : null}
          </button>
        );
      })}
    </div>
  );
}
