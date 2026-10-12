import { memo, useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Filter, RotateCw, X } from "lucide-react";
import { call } from "../api";
import { nativeMenu } from "../components/Menu";
import { PageHeader } from "../components/PageHeader";
import { TrackHeader, TrackList, TrackSkeleton, type SortDir, type SortKey } from "../components/TrackList";
import { useActions, useAppState } from "../context";
import { artistNames, formatTotal, songCount } from "../format";
import { LIKED_PLAYLIST_ID, type Target, type Track } from "../types";

export const FILTER_INPUT_ID = "playlist-filter";

export const PlaylistPage = memo(function PlaylistPage({ index, onRetry }: { index: number; onRetry: (i: number) => void }) {
  const { playlists } = useAppState();
  const { menus } = useActions();
  const playlist = playlists[index];
  const [songs, setSongs] = useState<Track[] | null>(null);
  const [filter, setFilter] = useState("");
  const [sortKey, setSortKey] = useState<SortKey>("none");
  const [sortDir, setSortDir] = useState<SortDir>("asc");

  // Rows are positions into this playlist, so an answer for a different one must never land.
  useEffect(() => {
    let live = true;
    setSongs(null);
    setFilter("");
    setSortKey("none");
    const load = () =>
      invoke<Track[]>("get_songs", { index }).then((s) => {
        if (live) setSongs(s);
      });
    void load();
    const unlisten = listen<number>("library-song-batch", (e) => {
      if (e.payload === index) void load();
    });
    return () => {
      live = false;
      void unlisten.then((f) => f());
    };
  }, [index]);

  // Filter and sort build one map from shown position to playlist position; every index goes through it.
  const order = useMemo(() => {
    const all = songs ?? [];
    const q = filter.trim().toLowerCase();
    const map: number[] = [];
    all.forEach((t, i) => {
      if (!q || (t.title ?? "").toLowerCase().includes(q) || artistNames(t).toLowerCase().includes(q)) map.push(i);
    });
    if (sortKey !== "none") {
      const dir = sortDir === "asc" ? 1 : -1;
      const key = (t: Track): string | number => {
        switch (sortKey) {
          case "title":
            return t.title ?? "";
          case "artist":
            return artistNames(t);
          case "album":
            return t.album?.name ?? "";
          default:
            return t.duration_seconds ?? 0;
        }
      };
      map.sort((a, b) => {
        const ka = key(all[a]);
        const kb = key(all[b]);
        if (typeof ka === "number" && typeof kb === "number") return (ka - kb) * dir;
        return String(ka).localeCompare(String(kb), undefined, { numeric: true }) * dir;
      });
    }
    return map;
  }, [songs, filter, sortKey, sortDir]);

  const shown = useMemo(() => order.map((i) => (songs ?? [])[i]), [order, songs]);
  // A sorted or filtered row keeps its number in the playlist.
  const numbers = useMemo(() => (sortKey === "none" && !filter ? undefined : order.map((i) => i + 1)), [order, sortKey, filter]);
  const totalSecs = useMemo(() => (songs ?? []).reduce((n, t) => n + (t.duration_seconds ?? 0), 0), [songs]);

  const targetOf = useCallback((i: number): Target => ({ kind: "library", playlist: index, song: order[i] ?? i }), [index, order]);
  const play = useCallback((i: number) => void call("play_target", { target: targetOf(i) }), [targetOf]);
  const removable = playlist?.playlist_id !== LIKED_PLAYLIST_ID;
  const extrasOf = useCallback(
    (i: number) =>
      removable
        ? {
            removeFromPlaylist: () =>
              void call("remove_from_playlist", { playlist: index, song: order[i] ?? i }, `Removed from ${playlist?.title ?? "playlist"}`),
          }
        : {},
    [removable, index, order, playlist?.title],
  );

  const onSort = useCallback(
    (k: SortKey) => {
      if (sortKey !== k) {
        setSortKey(k);
        setSortDir("asc");
      } else if (sortDir === "asc") {
        setSortDir("desc");
      } else {
        setSortKey("none");
      }
    },
    [sortKey, sortDir],
  );

  // Mode first: `build_queue` reads it as it builds, and a random start keeps every shuffle from opening the same way.
  const shuffle = useCallback(async () => {
    if (shown.length === 0) return;
    await call("set_mode", { mode: "shuffle" });
    play(Math.floor(Math.random() * shown.length));
  }, [shown.length, play]);

  if (!playlist) return null;
  const loading = songs === null || (!playlist.loaded && !playlist.failed && songs.length === 0);
  const meta = filter
    ? `${shown.length} matching`
    : songs && songs.length > 0
      ? `${songCount(songs.length)} · ${formatTotal(totalSecs)}`
      : playlist.count != null
        ? songCount(playlist.count)
        : undefined;

  return (
    <div className="@container">
      <PageHeader
        cover={playlist.thumbnail}
        eyebrow="Playlist"
        title={playlist.title}
        meta={meta}
        onPlay={shown.length > 0 ? () => play(0) : undefined}
        onShuffle={shown.length > 0 ? () => void shuffle() : undefined}
        menu={() => menus.playlist(index)}
      >
        {songs && songs.length > 0 && (
          <div className="ml-auto flex w-56 min-w-0 items-center gap-1.5 rounded-full bg-surface px-3 py-1.5">
            <Filter size={12} className="flex-shrink-0 text-ink-faint" />
            <input
              id={FILTER_INPUT_ID}
              value={filter}
              onChange={(e) => setFilter(e.target.value)}
              onKeyDown={(e) => e.key === "Escape" && setFilter("")}
              onContextMenu={nativeMenu}
              placeholder="Filter (/)"
              aria-label="Filter this playlist"
              className="w-full bg-transparent text-meta outline-none placeholder:text-ink-faint"
            />
            {filter && (
              <button onClick={() => setFilter("")} className="text-ink-faint hover:text-ink" aria-label="Clear filter">
                <X size={12} />
              </button>
            )}
          </div>
        )}
      </PageHeader>

      {playlist.failed && (songs?.length ?? 0) === 0 ? (
        <div className="flex flex-col items-center gap-3 py-16 text-center">
          <p className="text-heading text-ink-dim">Couldn't load this playlist.</p>
          <button onClick={() => onRetry(index)} className="btn btn-secondary">
            <RotateCw size={13} /> Try again
          </button>
        </div>
      ) : loading ? (
        <>
          <TrackHeader sortKey="none" sortDir="asc" onSort={() => {}} />
          <TrackSkeleton />
        </>
      ) : songs!.length === 0 ? (
        <p className="py-16 text-center text-heading text-ink-dim">This playlist is empty.</p>
      ) : (
        <>
          <TrackHeader sortKey={sortKey} sortDir={sortDir} onSort={onSort} />
          <TrackList
            tracks={shown}
            variant="playlist"
            onPlay={play}
            targetOf={targetOf}
            extrasOf={extrasOf}
            numbers={numbers}
          />
          {shown.length === 0 && filter && <p className="py-8 text-center text-body text-ink-dim">Nothing matches “{filter}”.</p>}
        </>
      )}
    </div>
  );
});
