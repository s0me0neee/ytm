import { memo } from "react";
import { MoreHorizontal, Play } from "lucide-react";
import { useActions, useAppState } from "../context";
import type { Target, Track } from "../types";
import { openMenuFrom, useMenu } from "./Menu";
import { PlayingIndicator } from "./PlayingIndicator";
import { Shelf } from "./Shelf";
import { Thumbnail } from "./Thumbnail";

const ROWS = 4;

// Columns fill the row exactly at each width, so paging by whole columns never leaves one cut in half.
const COLUMNS =
  "auto-cols-[100%] @xl:auto-cols-[calc((100%-1rem)/2)] @3xl:auto-cols-[calc((100%-2rem)/3)] @5xl:auto-cols-[calc((100%-3rem)/4)]";

/** Songs as Apple's compact grid: four rows deep, paged sideways, where a list would push the page down. */
export const SongGrid = memo(function SongGrid({
  title,
  tracks,
  subtitle,
  onPlay,
  targetOf,
}: {
  title: string;
  tracks: Track[];
  subtitle: (t: Track, i: number) => string;
  onPlay: (i: number) => void;
  targetOf: (i: number) => Target;
}) {
  const { currentTrackId, paused } = useAppState();
  const { menus } = useActions();
  const menu = useMenu();
  if (tracks.length === 0) return null;

  return (
    <Shelf title={title} className="mb-8" rowClassName={`grid grid-flow-col grid-rows-4 gap-x-4 px-3 ${COLUMNS}`}>
      {tracks.map((t, i) => {
        const isPlaying = Boolean(t.video_id) && t.video_id === currentTrackId;
        return (
          <div
            key={`${t.video_id ?? "song"}-${i}`}
            tabIndex={0}
            onDoubleClick={() => onPlay(i)}
            onKeyDown={(e) => e.key === "Enter" && onPlay(i)}
            onContextMenu={(e) => menu.show(e, menus.track(t, targetOf(i)))}
            className={`track-row flex min-w-0 items-center gap-3 rounded-lg py-1.5 pr-1 ${
              i % ROWS === 0 ? "" : "border-t-[0.5px] border-hairline"
            }`}
          >
            <span className="relative flex-shrink-0">
              <Thumbnail srcs={[t.thumbnail]} className="h-10 w-10 rounded-md object-cover" />
              <button
                onClick={() => onPlay(i)}
                onDoubleClick={(e) => e.stopPropagation()}
                aria-label={`Play ${t.title ?? "track"}`}
                className={`track-play absolute inset-0 flex items-center justify-center rounded-md bg-black/45 text-white ${
                  isPlaying ? "is-playing" : ""
                }`}
              >
                {isPlaying ? <PlayingIndicator paused={paused} /> : <Play size={14} fill="currentColor" />}
              </button>
            </span>
            <span className="flex min-w-0 flex-1 flex-col">
              <span className={`truncate text-body ${isPlaying ? "text-accent" : "text-ink"}`}>{t.title ?? "Untitled"}</span>
              <span className="truncate text-meta text-ink-dim">{subtitle(t, i)}</span>
            </span>
            <button
              onClick={(e) => openMenuFrom(e.currentTarget)}
              onDoubleClick={(e) => e.stopPropagation()}
              aria-label="More"
              className="flex-shrink-0 text-ink-dim hover:text-ink"
            >
              <MoreHorizontal size={16} />
            </button>
          </div>
        );
      })}
    </Shelf>
  );
});
