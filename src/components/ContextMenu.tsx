import { useEffect, useRef } from "react";

export interface ContextMenuItem {
  /** Stable id for keyed rendering. */
  id: string;
  label: string;
  /** Optional emoji / icon glyph shown to the left of the label. */
  icon?: string;
  /** When true, render as a disabled greyed-out entry that doesn't fire onClick. */
  disabled?: boolean;
  /** When true, render in danger color (red). */
  danger?: boolean;
  /** When true, render a thin separator BEFORE this item. */
  separator?: boolean;
  onClick?: () => void;
}

interface ContextMenuProps {
  /** Position in viewport coordinates (clientX/clientY). */
  x: number;
  y: number;
  items: ContextMenuItem[];
  onClose: () => void;
}

/**
 * Lightweight floating context menu. Auto-flips when too close to viewport
 * edge, dismisses on outside click / Escape / window blur. Position is
 * passed in viewport coordinates so the parent doesn't need to worry about
 * scroll offset.
 */
export function ContextMenu({ x, y, items, onClose }: ContextMenuProps) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    // Defer registration by 1 tick so the same mouse event that opened the
    // menu doesn't immediately close it.
    const t = setTimeout(() => {
      const onDoc = (e: MouseEvent) => {
        if (!ref.current) return;
        if (!ref.current.contains(e.target as Node)) onClose();
      };
      const onKey = (e: KeyboardEvent) => {
        if (e.key === "Escape") onClose();
      };
      document.addEventListener("mousedown", onDoc);
      document.addEventListener("keydown", onKey);
      window.addEventListener("blur", onClose);
      return () => {
        document.removeEventListener("mousedown", onDoc);
        document.removeEventListener("keydown", onKey);
        window.removeEventListener("blur", onClose);
      };
    }, 0);
    return () => clearTimeout(t);
  }, [onClose]);

  // Flip horizontally / vertically if the menu would clip the viewport.
  const style: React.CSSProperties = (() => {
    if (typeof window === "undefined") return { left: x, top: y };
    const W = 240;
    const H = items.length * 32 + 8;
    const left = Math.min(x, window.innerWidth - W - 8);
    const top = Math.min(y, window.innerHeight - H - 8);
    return { position: "fixed", left, top, minWidth: W };
  })();

  return (
    <div
      ref={ref}
      role="menu"
      style={style}
      className="z-[60] rounded-md border border-border bg-surface shadow-lg py-1 text-sm"
    >
      {items.map((it, i) => (
        <div key={it.id}>
          {it.separator && i > 0 && <div className="my-1 border-t border-border" />}
          <button
            type="button"
            disabled={it.disabled}
            onClick={() => {
              if (it.disabled) return;
              it.onClick?.();
              onClose();
            }}
            className={`w-full flex items-center gap-2 px-3 py-1.5 text-left ${
              it.disabled
                ? "opacity-50 cursor-not-allowed"
                : it.danger
                ? "text-danger hover:bg-danger/10"
                : "text-fg hover:bg-surface-2"
            }`}
          >
            {it.icon && <span className="w-4 text-center">{it.icon}</span>}
            <span className="flex-1">{it.label}</span>
          </button>
        </div>
      ))}
    </div>
  );
}
