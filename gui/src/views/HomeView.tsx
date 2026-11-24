import { memo, useCallback, useMemo } from "react";
import { Play } from "lucide-react";
import { call, send } from "../api";
import { CardActions } from "../components/CardActions";
import { useMenu } from "../components/Menu";
import { Shelf } from "../components/Shelf";
import { Thumbnail } from "../components/Thumbnail";
import { TrackList } from "../components/TrackList";
import { useActions, useAppState } from "../context";
import { coverCandidates } from "../cover";
import { artistNames, greeting } from "../format";
import { SEARCH_PLAYLIST_ID, type HistoryView, type Target } from "../types";

// A dozen covers is a shelf; a hundred would be a wall, so the rest stay rows.
const HOME_SHELF = 12;
// More than this is the sidebar's job.
const HOME_TILES = 8;

/** The start page: playlists as pictures, then what has been played lately. */
export const HomeView = memo(function HomeView({
  history,
  onPlayHistory,
}: {
  history: HistoryView | null;
  onPlayHistory: (index: number) => void;
}) {
  const { playlists, currentTrackId, paused } = useAppState();
  const { navigate, menus } = useActions();
  const menu = useMenu();
  const tracks = history?.tracks ?? [];
  const shelf = tracks.slice(0, HOME_SHELF);
  const rest = useMemo(() => tracks.slice(HOME_SHELF).map((p) => p.track), [tracks]);
  const tiles = useMemo(
    () =>
      playlists
        .map((p, i) => ({ p, i }))
        .filter(({ p }) => p.playlist_id !== SEARCH_PLAYLIST_ID && !p.failed)
        .slice(0, HOME_TILES),
    [playlists],
  );
  const historyTarget = (index: number): Target => ({ kind: "history", index });
  const restTarget = useCallback((i: number): Target => ({ kind: "history", index: i + HOME_SHELF }), []);
  const playRest = useCallback((i: number) => onPlayHistory(i + HOME_SHELF), [onPlayHistory]);

  return (
    <div className="select-none">
      <h1 className="px-3 pt-1 pb-5 text-title font-bold tracking-tight text-ink">{greeting()}</h1>

      {tiles.length > 0 && (
        <div className="mb-8 grid grid-cols-[repeat(auto-fill,minmax(14rem,1fr))] gap-2 px-3">
          {tiles.map(({ p, i }) => (
            <div key={p.playlist_id} className="group relative min-w-0" onContextMenu={(e) => menu.show(e, menus.playlist(i))}>
              <button
                onClick={() => navigate({ kind: "playlist", index: i })}
                className="flex w-full min-w-0 items-center gap-3 overflow-hidden rounded-xl bg-surface pr-12 text-left group-hover:bg-surface-2"
              >
                <Thumbnail srcs={[p.thumbnail]} className="h-12 w-12 flex-shrink-0" />
                <span className="min-w-0 flex-1 truncate text-body font-medium text-ink">{p.title}</span>
              </button>
              {p.loaded && (p.count ?? 0) > 0 && (
                <button
                  onClick={() => void call("play_target", { target: { kind: "library", playlist: i, song: 0 } })}
                  aria-label={`Play ${p.title}`}
                  className="absolute top-1/2 right-2 flex h-8 w-8 -translate-y-1/2 items-center justify-center rounded-full bg-accent text-white opacity-0 group-hover:opacity-100 hover:bg-accent-2 focus-visible:opacity-100"
                >
                  <Play size={13} fill="currentColor" />
                </button>
              )}
            </div>
          ))}
        </div>
      )}

      {history && tracks.length === 0 && (
        <div className="flex flex-col items-center gap-2 py-16 text-center">
          <p className="text-heading text-ink-dim">Nothing played yet.</p>
          <p className="text-body text-ink-faint">Pick a playlist or search for a song — what you play shows up here.</p>
        </div>
      )}

      {shelf.length > 0 && (
        <Shelf title="Recently played" rowClassName="flex gap-2 px-1 pb-6">
          {shelf.map((p, i) => {
            const t = p.track;
            const isPlaying = Boolean(t.video_id) && t.video_id === currentTrackId;
            return (
              <div
                key={`${t.video_id ?? "card"}-${i}`}
                className="group relative w-44 flex-shrink-0"
                onContextMenu={(e) => menu.show(e, menus.track(t, historyTarget(i)))}
              >
                <button onClick={() => onPlayHistory(i)} className="card w-full rounded-2xl p-2 text-left group-hover:bg-surface">
                  <Thumbnail
                    srcs={t.thumbnail ? coverCandidates(t.thumbnail, 320) : []}
                    className="mb-2 aspect-square w-full rounded-xl"
                  />
                  <p className={`truncate text-body ${isPlaying ? "text-accent" : "text-ink"}`}>{t.title ?? "Untitled"}</p>
                  <p className="truncate text-meta text-ink-dim">{artistNames(t)}</p>
                </button>
                <CardActions
                  label={t.title ?? "track"}
                  onPlay={() => (isPlaying ? send("play_pause") : onPlayHistory(i))}
                  playing={isPlaying ? { paused } : null}
                />
              </div>
            );
          })}
        </Shelf>
      )}

      {rest.length > 0 && (
        <section className="@container">
          <h2 className="section-heading">Earlier</h2>
          <TrackList tracks={rest} variant="playlist" offset={HOME_SHELF} onPlay={playRest} targetOf={restTarget} />
        </section>
      )}
    </div>
  );
});
