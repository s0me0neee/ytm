import type { ReactNode } from "react";
import { MoreHorizontal, Play, Shuffle } from "lucide-react";
import { coverCandidates } from "../cover";
import { openMenuFrom, useMenu, type MenuEntry } from "./Menu";
import { Thumbnail } from "./Thumbnail";
import { Tip } from "./Tip";

interface PageHeaderProps {
  cover?: string | null;
  round?: boolean;
  eyebrow?: string;
  title: string;
  subtitle?: ReactNode;
  meta?: string;
  onPlay?: () => void;
  onShuffle?: () => void;
  menu?: () => MenuEntry[];
  children?: ReactNode;
}

/** A page's face: cover, title, what it holds, and what can be done with it. */
export function PageHeader({ cover, round, eyebrow, title, subtitle, meta, onPlay, onShuffle, menu, children }: PageHeaderProps) {
  const menus = useMenu();
  return (
    <header className="flex flex-wrap items-end gap-x-6 gap-y-4 px-3 pt-1 pb-6 select-none">
      {cover !== undefined && (
        <Thumbnail
          srcs={cover ? coverCandidates(cover, 400) : []}
          className={`h-36 w-36 flex-shrink-0 shadow-xl shadow-black/40 ${round ? "rounded-full" : "rounded-2xl"}`}
        />
      )}
      <div className="flex min-w-0 flex-1 flex-col gap-1 pb-0.5">
        {eyebrow && <p className="text-label font-semibold tracking-wider text-ink-faint uppercase">{eyebrow}</p>}
        <h1 className="line-clamp-2 text-display leading-tight font-bold tracking-tight text-ink">{title}</h1>
        {subtitle && <div className="truncate text-heading text-ink-dim">{subtitle}</div>}
        {meta && <p className="truncate text-body text-ink-faint">{meta}</p>}
        <div className="mt-3 flex flex-wrap items-center gap-2" onContextMenu={menu ? (e) => menus.show(e, menu()) : undefined}>
          {onPlay && (
            <button onClick={onPlay} className="btn btn-primary">
              <Play size={13} fill="currentColor" /> Play
            </button>
          )}
          {onShuffle && (
            <button onClick={onShuffle} className="btn btn-secondary">
              <Shuffle size={13} /> Shuffle
            </button>
          )}
          {menu && (
            <Tip label="More">
              <button onClick={(e) => openMenuFrom(e.currentTarget)} className="btn-icon">
                <MoreHorizontal size={16} />
              </button>
            </Tip>
          )}
          {children}
        </div>
      </div>
    </header>
  );
}
