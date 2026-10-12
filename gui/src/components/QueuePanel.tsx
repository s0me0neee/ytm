import { memo } from "react";
import { MoreHorizontal, X } from "lucide-react";
import { useActions } from "../context";
import type { QueueEntry } from "../types";
import { openMenuFrom, useMenu } from "./Menu";
import { Thumbnail } from "./Thumbnail";
import { Tip } from "./Tip";

interface QueuePanelProps {
  entries: QueueEntry[];
  onJump: (qPos: number) => void;
  onRemove: (qPos: number) => void;
  onClear: () => void;
}

/** Up Next: what is still to come, addressed by queue position. */
export const QueuePanel = memo(function QueuePanel({ entries, onJump, onRemove, onClear }: QueuePanelProps) {
  const { menus } = useActions();
  const menu = useMenu();
  const current = entries.findIndex((e) => e.current);
  const upcoming = current === -1 ? entries : entries.slice(current + 1);

  return (
    <aside aria-label="Up Next" className="m-3 ml-0 flex w-72 flex-shrink-0 flex-col overflow-hidden rounded-2xl select-none glass">
      <div className="flex flex-shrink-0 items-center justify-between px-4 py-3">
        <p className="text-label font-semibold tracking-wider text-ink-faint uppercase">Up Next</p>
        {entries.length > 0 && (
          <button onClick={onClear} className="text-label text-accent hover:text-accent-2">
            Clear
          </button>
        )}
      </div>

      <div className="thin-scrollbar min-h-0 flex-1 overflow-y-auto px-2 pb-2">
        {upcoming.length === 0 && (
          <div className="px-3 py-8 text-center">
            <p className="text-body text-ink-dim">Nothing queued.</p>
            <p className="mt-1 text-meta text-ink-faint">Right-click a song and choose Play Next.</p>
          </div>
        )}
        {upcoming.map((e) => (
          <div
            key={e.qPos}
            className="row track-row group relative flex items-center rounded-xl"
            onContextMenu={(ev) => {
              if (e.track) {
                menu.show(ev, menus.track(e.track, { kind: "library", playlist: e.playlist, song: e.song }, { queueEntry: e }));
              }
            }}
          >
            <button
              onClick={() => onJump(e.qPos)}
              className="flex w-full min-w-0 items-center gap-2.5 py-1.5 pr-14 pl-2 text-left"
            >
              <Thumbnail srcs={[e.thumbnail]} className="h-9 w-9 flex-shrink-0 rounded object-cover" />
              <span className="min-w-0 flex-1">
                <p className="truncate text-body text-ink">{e.title}</p>
                <p className="truncate text-meta text-ink-dim">{e.artist}</p>
              </span>
            </button>
            <span className="absolute top-1/2 right-2 flex -translate-y-1/2 items-center gap-1.5">
              <span className="font-mono text-label text-ink-ghost tabular-nums group-hover:hidden">{e.duration}</span>
              <Tip label="More">
                <button
                  onClick={(ev) => openMenuFrom(ev.currentTarget)}
                  className="hidden text-ink-dim group-hover:block hover:text-ink"
                >
                  <MoreHorizontal size={14} />
                </button>
              </Tip>
              <Tip label="Remove from queue">
                <button onClick={() => onRemove(e.qPos)} className="hidden text-ink-dim group-hover:block hover:text-accent">
                  <X size={14} />
                </button>
              </Tip>
            </span>
          </div>
        ))}
      </div>
    </aside>
  );
});
