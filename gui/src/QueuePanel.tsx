import { memo } from "react";
import { X } from "lucide-react";
import { Thumbnail } from "./Thumbnail";

export interface QueueEntry {
  qPos: number;
  playlist: number;
  song: number;
  title: string;
  artist: string;
  duration: string;
  thumbnail: string | null;
  videoId: string | null;
  current: boolean;
}

interface QueuePanelProps {
  entries: QueueEntry[];
  onJump: (qPos: number) => void;
  onRemove: (qPos: number) => void;
  onClear: () => void;
  onContextMenu: (e: React.MouseEvent, entry: QueueEntry) => void;
}

/** Apple Music's "Up Next", against our own queue.
 *
 * Entries are addressed by `qPos` -- their position in the queue -- because
 * that is what `jump_to` and `remove_from_queue` take. The rows are resolved
 * server-side (`get_queue`) rather than joined here, since the queue holds
 * only `(playlist, song)` pairs and the frontend has no copy of playlists it
 * hasn't opened. */
export const QueuePanel = memo(function QueuePanel({
  entries,
  onJump,
  onRemove,
  onClear,
  onContextMenu,
}: QueuePanelProps) {
  // Everything before the playing entry has already been heard. Apple shows
  // only what is still coming, which is what "Up Next" means.
  const current = entries.findIndex((e) => e.current);
  const upcoming = current === -1 ? entries : entries.slice(current + 1);

  return (
    <aside className="m-3 ml-0 flex w-72 flex-shrink-0 flex-col overflow-hidden rounded-2xl select-none glass">
      <div className="flex flex-shrink-0 items-center justify-between px-4 py-3">
        <p className="text-[11px] font-semibold tracking-wider text-ink-faint uppercase">Up Next</p>
        {entries.length > 0 && (
          <button onClick={onClear} className="text-[11px] text-accent transition-colors hover:text-accent-2">
            Clear
          </button>
        )}
      </div>

      <div className="thin-scrollbar min-h-0 flex-1 overflow-y-auto px-2 pb-2">
        {upcoming.length === 0 && (
          <p className="px-3 py-6 text-center text-[13px] text-ink-faint">No upcoming songs.</p>
        )}
        {upcoming.map((e) => (
          <div
            key={e.qPos}
            className="row group relative flex items-center rounded-xl transition-colors hover:bg-surface"
            onContextMenu={(ev) => onContextMenu(ev, e)}
          >
            <button
              onClick={() => onJump(e.qPos)}
              className="flex w-full min-w-0 items-center gap-2.5 py-1.5 pr-8 pl-2 text-left"
            >
              <Thumbnail srcs={[e.thumbnail]} className="h-9 w-9 flex-shrink-0 rounded object-cover" />
              <span className="min-w-0 flex-1">
                <p className="truncate text-[13px] text-ink">{e.title}</p>
                <p className="truncate text-xs text-ink-dim">{e.artist}</p>
              </span>
              <span className="flex-shrink-0 font-mono text-[11px] text-ink-ghost">{e.duration}</span>
            </button>
            <button
              onClick={() => onRemove(e.qPos)}
              aria-label="Remove from queue"
              className="absolute top-1/2 right-2 -translate-y-1/2 text-ink-ghost opacity-0 transition hover:text-accent group-hover:opacity-100"
            >
              <X size={13} />
            </button>
          </div>
        ))}
      </div>
    </aside>
  );
});
