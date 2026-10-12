import { memo, useCallback, useEffect, useRef } from "react";
import { ChevronDown, ChevronUp, Heart, MoreHorizontal, Play } from "lucide-react";
import { useActions, useAppState, type TrackMenuExtras } from "../context";
import type { Artist, Target, Track } from "../types";
import { openMenuFrom, useMenu } from "./Menu";
import { PlayingIndicator } from "./PlayingIndicator";
import { Thumbnail } from "./Thumbnail";

export type SortKey = "none" | "title" | "artist" | "album" | "duration";
export type SortDir = "asc" | "desc";

/** `playlist`: number, art, artist and album columns. `album`: number, no art. `plain`: art, no number. */
export type Variant = "playlist" | "album" | "plain";

// Column breakpoints are the list's own width (container queries), not the window's: the queue takes room too.
// The last column is fixed so the header, a separate grid, lines up with the rows.
const GRID: Record<Variant, string> = {
  playlist:
    "grid-cols-[2rem_2.75rem_minmax(0,1fr)_5.75rem] @2xl:grid-cols-[2rem_2.75rem_minmax(0,3fr)_minmax(0,2fr)_5.75rem] @4xl:grid-cols-[2rem_2.75rem_minmax(0,3fr)_minmax(0,2fr)_minmax(0,2fr)_5.75rem]",
  album: "grid-cols-[2rem_minmax(0,1fr)_5.75rem]",
  plain:
    "grid-cols-[2.75rem_minmax(0,1fr)_5.75rem] @2xl:grid-cols-[2.75rem_minmax(0,3fr)_minmax(0,2fr)_5.75rem]",
};

const stop = (e: React.SyntheticEvent) => e.stopPropagation();

/** Each artist as a link to their page where the API gave an id. */
export const ArtistLinks = memo(function ArtistLinks({ artists, className = "" }: { artists: Artist[]; className?: string }) {
  const { navigate } = useActions();
  return (
    <span className={`truncate ${className}`}>
      {artists.map((a, i) => (
        <span key={`${a.name}-${i}`}>
          {i > 0 && ", "}
          {a.id ? (
            <button
              className="link"
              onClick={(e) => {
                stop(e);
                navigate({ kind: "artist", id: a.id! });
              }}
              onDoubleClick={stop}
            >
              {a.name}
            </button>
          ) : (
            a.name
          )}
        </span>
      ))}
    </span>
  );
});

function AlbumLink({ track }: { track: Track }) {
  const { navigate } = useActions();
  const album = track.album;
  if (!album) return <span />;
  if (!album.id) return <span className="truncate">{album.name}</span>;
  return (
    <span className="truncate">
      <button
        className="link"
        onClick={(e) => {
          stop(e);
          navigate({ kind: "album", id: album.id! });
        }}
        onDoubleClick={stop}
      >
        {album.name}
      </button>
    </span>
  );
}

export function TrackHeader({
  sortKey,
  sortDir,
  onSort,
}: {
  sortKey: SortKey;
  sortDir: SortDir;
  onSort: (k: SortKey) => void;
}) {
  const col = (k: SortKey, label: string, extra = "") => (
    <button
      onClick={() => onSort(k)}
      className={`flex min-w-0 items-center gap-1 text-left text-label font-semibold tracking-wider uppercase ${
        sortKey === k ? "text-ink" : "text-ink-faint hover:text-ink-dim"
      } ${extra}`}
    >
      <span className="truncate">{label}</span>
      {sortKey === k && (sortDir === "asc" ? <ChevronUp size={11} /> : <ChevronDown size={11} />)}
    </button>
  );
  return (
    <div className={`grid ${GRID.playlist} items-center gap-3 border-b-[0.5px] border-hairline px-3 pb-2 select-none`}>
      <span />
      <span />
      {col("title", "Song")}
      {col("artist", "Artist", "hidden @2xl:flex")}
      {col("album", "Album", "hidden @4xl:flex")}
      {col("duration", "Time", "justify-end pr-6")}
    </div>
  );
}

type PlayState = "none" | "playing" | "paused";

interface TrackRowProps {
  track: Track;
  index: number;
  variant: Variant;
  number: string | number;
  play: PlayState;
  liked: boolean;
  onPlay: (i: number) => void;
  onSelect: (el: HTMLElement) => void;
  onMenu: (e: React.MouseEvent<HTMLElement>, i: number) => void;
  onHover: (i: number) => void;
}

/** One row. Memoised on plain values, so a click, a like or a track change re-renders only the rows it touches. */
const TrackRow = memo(function TrackRow({
  track: t,
  index: i,
  variant,
  number,
  play,
  liked,
  onPlay,
  onSelect,
  onMenu,
  onHover,
}: TrackRowProps) {
  const { toggleLike } = useActions();
  const numbered = variant !== "plain";
  const art = variant !== "album";
  const isPlaying = play !== "none";
  const indicator = <PlayingIndicator paused={play === "paused"} />;
  const playButton = (className: string, content: React.ReactNode) => (
    <button
      className={className}
      aria-label={`Play ${t.title ?? "track"}`}
      onClick={(e) => {
        stop(e);
        onPlay(i);
      }}
      onDoubleClick={stop}
    >
      {content}
    </button>
  );

  return (
    <li
      tabIndex={0}
      className={`row track-row grid ${GRID[variant]} items-center gap-3 rounded-lg px-3 py-1.5`}
      onClick={(e) => onSelect(e.currentTarget)}
      onDoubleClick={() => onPlay(i)}
      onKeyDown={(e) => {
        if (e.key === "Enter") onPlay(i);
      }}
      onMouseEnter={() => onHover(i)}
      onContextMenu={(e) => {
        onSelect(e.currentTarget);
        onMenu(e, i);
      }}
    >
      {numbered && (
        <span className="relative flex h-full items-center justify-end">
          <span className="track-num flex items-center">
            {isPlaying ? indicator : <span className="font-mono text-meta text-ink-faint tabular-nums">{number}</span>}
          </span>
          {playButton("track-play absolute inset-y-0 right-0 flex items-center text-ink", <Play size={14} fill="currentColor" />)}
        </span>
      )}
      {art && (
        <span className="relative">
          <Thumbnail srcs={[t.thumbnail]} className="h-11 w-11 rounded-md object-cover" />
          {!numbered &&
            playButton(
              `track-play absolute inset-0 flex items-center justify-center rounded-md bg-black/45 text-white ${isPlaying ? "is-playing" : ""}`,
              isPlaying ? indicator : <Play size={16} fill="currentColor" />,
            )}
        </span>
      )}
      <span className="flex min-w-0 flex-col">
        <span className={`truncate text-body ${isPlaying ? "text-accent" : "text-ink"}`}>{t.title ?? "Untitled"}</span>
        <ArtistLinks artists={t.artists} className={`text-meta text-ink-dim ${variant === "album" ? "" : "@2xl:hidden"}`} />
      </span>
      {variant === "playlist" && (
        <span className="hidden min-w-0 text-body text-ink-dim @2xl:flex">
          <ArtistLinks artists={t.artists} />
        </span>
      )}
      {variant === "playlist" && (
        <span className="hidden min-w-0 text-body text-ink-dim @4xl:flex">
          <AlbumLink track={t} />
        </span>
      )}
      {variant === "plain" && (
        <span className="hidden min-w-0 text-body text-ink-dim @2xl:flex">
          <AlbumLink track={t} />
        </span>
      )}
      <span className="flex items-center justify-end gap-2">
        {t.video_id && (
          <button
            className={`track-heart ${liked ? "is-liked text-accent" : "text-ink-faint hover:text-ink"}`}
            aria-label={liked ? "Remove from Liked Music" : "Add to Liked Music"}
            aria-pressed={liked}
            onClick={(e) => {
              stop(e);
              toggleLike(t);
            }}
            onDoubleClick={stop}
          >
            <Heart size={14} fill={liked ? "currentColor" : "none"} />
          </button>
        )}
        <span className="w-10 text-right font-mono text-meta text-ink-faint tabular-nums">{t.duration ?? ""}</span>
        <button
          className="track-more text-ink-dim hover:text-ink"
          aria-label="More"
          onClick={(e) => {
            stop(e);
            openMenuFrom(e.currentTarget);
          }}
          onDoubleClick={stop}
        >
          <MoreHorizontal size={16} />
        </button>
      </span>
    </li>
  );
});

interface TrackListProps {
  tracks: Track[];
  variant: Variant;
  onPlay: (i: number) => void;
  targetOf: (i: number) => Target;
  extrasOf?: (i: number) => TrackMenuExtras;
  /** Where numbering starts, for a list that continues another. */
  offset?: number;
  /** What the number column shows, where it isn't the position (a sorted playlist). */
  numbers?: number[];
}

export const TrackList = memo(function TrackList({ tracks, variant, onPlay, targetOf, extrasOf, offset = 0, numbers }: TrackListProps) {
  const { currentTrackId, paused, liked } = useAppState();
  const { menus, prefetch } = useActions();
  const menu = useMenu();
  // Selection is a DOM attribute rather than state: moving it touches two elements, not a list re-render.
  const selected = useRef<HTMLElement | null>(null);

  // The latest per-list callbacks, read by handlers that stay stable so the rows' memo holds.
  const latest = useRef({ tracks, targetOf, extrasOf });
  latest.current = { tracks, targetOf, extrasOf };

  useEffect(() => {
    selected.current?.removeAttribute("data-selected");
    selected.current = null;
  }, [tracks]);

  const onSelect = useCallback((el: HTMLElement) => {
    if (selected.current === el) return;
    selected.current?.removeAttribute("data-selected");
    el.setAttribute("data-selected", "");
    selected.current = el;
  }, []);
  const onMenu = useCallback(
    (e: React.MouseEvent<HTMLElement>, i: number) => {
      const { tracks, targetOf, extrasOf } = latest.current;
      menu.show(e, menus.track(tracks[i], targetOf(i), extrasOf?.(i)));
    },
    [menu, menus],
  );
  const onHover = useCallback((i: number) => prefetch(latest.current.targetOf(i)), [prefetch]);

  return (
    <ul className="@container select-none" role="list">
      {tracks.map((t, i) => {
        const isPlaying = Boolean(t.video_id) && t.video_id === currentTrackId;
        return (
          <TrackRow
            key={`${t.video_id ?? "row"}-${i}`}
            track={t}
            index={i}
            variant={variant}
            number={numbers?.[i] ?? i + 1 + offset}
            play={isPlaying ? (paused ? "paused" : "playing") : "none"}
            liked={Boolean(t.video_id && liked.has(t.video_id))}
            onPlay={onPlay}
            onSelect={onSelect}
            onMenu={onMenu}
            onHover={onHover}
          />
        );
      })}
    </ul>
  );
});

/** Placeholder rows while a list loads, in the same grid so nothing jumps when it lands. */
export function TrackSkeleton({ rows = 8, variant = "playlist" }: { rows?: number; variant?: Variant }) {
  return (
    <ul className="@container" aria-busy aria-label="Loading">
      {Array.from({ length: rows }, (_, i) => (
        <li key={i} className={`grid ${GRID[variant]} items-center gap-3 px-3 py-1.5`} style={{ opacity: 1 - i * 0.1 }}>
          {variant !== "plain" && <span className="ml-auto h-3 w-4 rounded bg-surface-2" />}
          {variant !== "album" && <span className="h-11 w-11 rounded-md bg-surface-2" />}
          <span className="flex flex-col gap-1.5">
            <span className="h-3 w-2/5 rounded bg-surface-2" />
            <span className="h-2.5 w-1/4 rounded bg-surface" />
          </span>
          {variant === "playlist" && <span className="hidden h-3 w-1/3 rounded bg-surface @2xl:block" />}
          {variant === "playlist" && <span className="hidden h-3 w-1/3 rounded bg-surface @4xl:block" />}
          {variant === "plain" && <span className="hidden h-3 w-1/3 rounded bg-surface @2xl:block" />}
          <span className="h-3 w-10 rounded bg-surface" />
        </li>
      ))}
    </ul>
  );
}
