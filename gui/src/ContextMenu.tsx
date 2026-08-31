import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { ReactNode } from "react";

export interface MenuItem {
  label: string;
  icon?: ReactNode;
  onSelect: () => void;
  /** Draws a separator above this item. */
  separatorBefore?: boolean;
  disabled?: boolean;
  /** Renders in the accent colour -- destructive or otherwise notable. */
  danger?: boolean;
  /** A submenu instead of an action. `onSelect` is ignored when set. */
  items?: MenuItem[];
}

export interface MenuState {
  x: number;
  y: number;
  items: MenuItem[];
}

const MENU_WIDTH = 208;
const EDGE_GAP = 8;

/** Right-click menu, positioned at the pointer and flipped back inside the
 * window when it would overflow.
 *
 * Hand-rolled rather than pulled from a component library for the same reason
 * `tui/src/kitty.rs` hand-writes its base64: it is a small, well-understood
 * piece of behaviour, and a dependency here would be a whole design system's
 * worth of styling to override before it matched anything else on screen. */
export function ContextMenu({ state, onClose }: { state: MenuState; onClose: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ x: state.x, y: state.y });
  const [openSub, setOpenSub] = useState<number | null>(null);

  // Measured after mount rather than guessed: the height depends on how many
  // items this particular menu has, and a menu opened near the bottom edge has
  // to be flipped up by exactly that.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const { width, height } = el.getBoundingClientRect();
    const x = Math.min(state.x, window.innerWidth - width - EDGE_GAP);
    const y = Math.min(state.y, window.innerHeight - height - EDGE_GAP);
    setPos({ x: Math.max(EDGE_GAP, x), y: Math.max(EDGE_GAP, y) });
  }, [state]);

  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") onClose();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  function run(item: MenuItem) {
    if (item.disabled || item.items) return;
    item.onSelect();
    onClose();
  }

  return (
    // A full-window backdrop, so any click outside dismisses and no click
    // reaches whatever is underneath. `onContextMenu` is caught too, or a
    // second right-click would open the page's own menu behind this one.
    <div
      className="fixed inset-0 z-50"
      onClick={onClose}
      onContextMenu={(e) => {
        e.preventDefault();
        onClose();
      }}
    >
      <div
        ref={ref}
        style={{ left: pos.x, top: pos.y, width: MENU_WIDTH }}
        className="absolute overflow-visible rounded-xl py-1 glass-heavy"
        onClick={(e) => e.stopPropagation()}
      >
        {state.items.map((item, i) => (
          <div key={i} className="relative">
            {item.separatorBefore && <div className="my-1 h-px bg-hairline" />}
            <button
              disabled={item.disabled}
              onClick={() => run(item)}
              onMouseEnter={() => setOpenSub(item.items ? i : null)}
              className={`flex w-full items-center gap-2 px-3 py-1.5 text-left text-[13px] transition-colors ${
                item.disabled
                  ? "cursor-default text-ink-ghost"
                  : item.danger
                    ? "text-accent hover:bg-surface-2"
                    : "text-ink hover:bg-surface-2"
              }`}
            >
              <span className="min-w-0 flex-1 truncate">{item.label}</span>
              {item.items ? (
                <span className="flex-shrink-0 text-ink-faint">›</span>
              ) : (
                item.icon && <span className="flex-shrink-0 text-ink-faint">{item.icon}</span>
              )}
            </button>

            {item.items && openSub === i && (
              <div
                style={{ width: MENU_WIDTH }}
                // Flips to the left when the parent is already near the right
                // edge, which is where a submenu would otherwise vanish.
                className={`absolute top-0 z-10 max-h-72 overflow-y-auto rounded-xl py-1 thin-scrollbar glass-heavy ${
                  pos.x + MENU_WIDTH * 2 + EDGE_GAP > window.innerWidth ? "right-full mr-1" : "left-full ml-1"
                }`}
              >
                {item.items.length === 0 && (
                  <p className="px-3 py-1.5 text-[13px] text-ink-faint">Nothing here</p>
                )}
                {item.items.map((sub, j) => (
                  <button
                    key={j}
                    onClick={() => run(sub)}
                    className="flex w-full items-center px-3 py-1.5 text-left text-[13px] text-ink transition-colors hover:bg-surface-2"
                  >
                    <span className="min-w-0 flex-1 truncate">{sub.label}</span>
                  </button>
                ))}
              </div>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}
