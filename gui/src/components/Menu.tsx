import { createContext, useCallback, useContext, useMemo, useRef, useState, type ReactNode } from "react";
import * as CM from "@radix-ui/react-context-menu";
import { ChevronRight } from "lucide-react";

export type MenuEntry =
  | {
      label: string;
      icon?: ReactNode;
      onSelect?: () => void;
      disabled?: boolean;
      danger?: boolean;
      items?: MenuEntry[];
    }
  | { separator: true }
  | { heading: string };

export const SEPARATOR: MenuEntry = { separator: true };

interface MenuApi {
  /** Call from a row's `onContextMenu`; the window's one menu then opens with these. */
  show: (e: React.MouseEvent<HTMLElement>, items: MenuEntry[]) => void;
}

const MenuContext = createContext<MenuApi>({ show: () => {} });

export function useMenu(): MenuApi {
  return useContext(MenuContext);
}

/** Opens the menu of the row containing `button`, as a right-click on it would. */
export function openMenuFrom(button: HTMLElement): void {
  const r = button.getBoundingClientRect();
  button.dispatchEvent(
    new MouseEvent("contextmenu", { bubbles: true, cancelable: true, button: 2, clientX: r.left, clientY: r.bottom + 4 }),
  );
}

/** Lets a text field keep the platform's own cut/copy/paste menu. */
export function nativeMenu(e: React.MouseEvent): void {
  e.stopPropagation();
}

function Entries({ items }: { items: MenuEntry[] }) {
  return (
    <>
      {items.map((item, i) => {
        if ("separator" in item) return <CM.Separator key={i} className="menu-sep" />;
        if ("heading" in item) {
          return (
            <CM.Label key={i} className="menu-heading">
              {item.heading}
            </CM.Label>
          );
        }
        if (item.items) {
          return (
            <CM.Sub key={i}>
              <CM.SubTrigger className="menu-item" disabled={item.disabled}>
                <span className="min-w-0 flex-1 truncate">{item.label}</span>
                <ChevronRight size={14} className="menu-icon" />
              </CM.SubTrigger>
              <CM.Portal>
                <CM.SubContent className="menu menu-scroll" sideOffset={-2} alignOffset={-1} collisionPadding={8}>
                  {item.items.length === 0 ? (
                    <CM.Label className="menu-heading">Nothing here</CM.Label>
                  ) : (
                    <Entries items={item.items} />
                  )}
                </CM.SubContent>
              </CM.Portal>
            </CM.Sub>
          );
        }
        return (
          <CM.Item
            key={i}
            className={`menu-item ${item.danger ? "is-danger" : ""}`}
            disabled={item.disabled}
            onSelect={item.onSelect}
          >
            <span className="min-w-0 flex-1 truncate">{item.label}</span>
            {item.icon && <span className="menu-icon">{item.icon}</span>}
          </CM.Item>
        );
      })}
    </>
  );
}

export function MenuProvider({ children }: { children: ReactNode }) {
  const [items, setItems] = useState<MenuEntry[]>([]);
  // Set by a row in the same event, so a right-click on nothing opens nothing.
  const armed = useRef(false);
  const marked = useRef<HTMLElement | null>(null);

  const show = useCallback((e: React.MouseEvent<HTMLElement>, next: MenuEntry[]) => {
    armed.current = true;
    setItems(next);
    marked.current?.removeAttribute("data-menu-open");
    marked.current = e.currentTarget;
    e.currentTarget.setAttribute("data-menu-open", "");
  }, []);
  const api = useMemo(() => ({ show }), [show]);

  return (
    <MenuContext.Provider value={api}>
      <CM.Root
        onOpenChange={(open) => {
          if (open) return;
          marked.current?.removeAttribute("data-menu-open");
          marked.current = null;
        }}
      >
        <CM.Trigger
          asChild
          onContextMenu={(e) => {
            if (!armed.current) e.preventDefault();
            armed.current = false;
          }}
        >
          <div className="contents">{children}</div>
        </CM.Trigger>
        <CM.Portal>
          <CM.Content className="menu" collisionPadding={8}>
            <Entries items={items} />
          </CM.Content>
        </CM.Portal>
      </CM.Root>
    </MenuContext.Provider>
  );
}
