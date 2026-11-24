import { memo, useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Play, RotateCw, SearchX } from "lucide-react";
import { call } from "../api";
import { CardShelf } from "../components/CardShelf";
import { useMenu } from "../components/Menu";
import { PlayingIndicator } from "../components/PlayingIndicator";
import { SongGrid } from "../components/SongGrid";
import { Thumbnail } from "../components/Thumbnail";
import { useActions, useAppState } from "../context";
import { coverCandidates } from "../cover";
import { artistNames, errorText, resultTrack } from "../format";
import type { Card, LibraryHit, SearchPage as Results, Target, Track } from "../types";

// Going back to a search shows it again at once rather than asking YouTube twice.
const cache = new Map<string, Results>();
// Two rows of three: enough to hold the obvious answer without becoming a second results list.
const TOP = 6;

type Top = { kind: "page"; card: Card } | { kind: "song"; index: number };

/** The artist or album the query names (`top`, decided by the backend, which the TUI shares), then the best songs. */
function topResults(r: Results): Top[] {
  const out: Top[] = r.top.map((card) => ({ kind: "page", card }));
  r.songs.slice(0, TOP - out.length).forEach((_, index) => out.push({ kind: "song", index }));
  return out;
}

function TopTile({
  thumbnail,
  round,
  title,
  subtitle,
  playing,
  onOpen,
  onMenu,
}: {
  thumbnail: string | null;
  round: boolean;
  title: string;
  subtitle: string;
  /** Set for a song: whether it is the one playing, and paused. */
  playing?: { now: boolean; paused: boolean };
  onOpen: () => void;
  onMenu: (e: React.MouseEvent<HTMLElement>) => void;
}) {
  return (
    <button
      onClick={onOpen}
      onContextMenu={onMenu}
      className="group flex min-w-0 items-center gap-4 rounded-2xl bg-surface p-3 text-left hover:bg-surface-2"
    >
      <span className="relative flex-shrink-0">
        <Thumbnail
          srcs={thumbnail ? coverCandidates(thumbnail, 160) : []}
          className={`h-16 w-16 shadow-md shadow-black/30 ${round ? "rounded-full" : "rounded-lg"}`}
        />
        {playing && (
          <span
            className={`absolute inset-0 flex items-center justify-center rounded-lg bg-black/45 text-white ${
              playing.now ? "" : "opacity-0 group-hover:opacity-100"
            }`}
          >
            {playing.now ? <PlayingIndicator paused={playing.paused} /> : <Play size={18} fill="currentColor" />}
          </span>
        )}
      </span>
      <span className="min-w-0 flex-1">
        <p className={`truncate text-heading font-semibold ${playing?.now ? "text-accent" : "text-ink"}`}>{title}</p>
        <p className="truncate text-meta text-ink-dim">{subtitle}</p>
      </span>
    </button>
  );
}

const songLine = (t: Track) => [artistNames(t), t.album?.name].filter(Boolean).join(" · ");
const videoLine = (t: Track) => [artistNames(t), t.duration].filter(Boolean).join(" · ");

/** Results for one query, laid out by kind as Apple's are: the likely answer first, then each kind in turn. */
export const SearchPage = memo(function SearchPage({ query }: { query: string }) {
  const { playlists, currentTrackId, paused } = useAppState();
  const { navigate, menus } = useActions();
  const menu = useMenu();
  const [results, setResults] = useState<Results | null>(() => cache.get(query) ?? null);
  const [error, setError] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);
  const [hits, setHits] = useState<LibraryHit[]>([]);

  useEffect(() => {
    let live = true;
    setError(null);
    const hit = cache.get(query);
    if (hit && attempt === 0) {
      setResults(hit);
      return;
    }
    setResults(null);
    invoke<Results>("search", { query })
      .then((found) => {
        cache.set(query, found);
        if (live) setResults(found);
      })
      .catch((e) => live && setError(errorText(e)));
    return () => {
      live = false;
    };
  }, [query, attempt]);

  // Local and instant, so it is asked again as playlists finish loading rather than cached.
  useEffect(() => {
    let live = true;
    invoke<LibraryHit[]>("search_library", { query })
      .then((h) => live && setHits(h))
      .catch(() => live && setHits([]));
    return () => {
      live = false;
    };
  }, [query, playlists]);

  const songs = useMemo(() => (results?.songs ?? []).map(resultTrack), [results]);
  const videos = useMemo(() => (results?.videos ?? []).map(resultTrack), [results]);
  const top = useMemo(() => (results ? topResults(results) : []), [results]);
  const mine = useMemo(() => hits.map((h) => h.track), [hits]);

  const songTarget = useCallback((i: number): Target => ({ kind: "track", track: songs[i] }), [songs]);
  const playSong = useCallback((i: number) => void call("play_tracks", { tracks: songs, start: i }), [songs]);
  const videoTarget = useCallback((i: number): Target => ({ kind: "track", track: videos[i] }), [videos]);
  const playVideo = useCallback((i: number) => void call("play_tracks", { tracks: videos, start: i }), [videos]);
  const mineTarget = useCallback(
    (i: number): Target => ({ kind: "library", playlist: hits[i]?.playlist ?? 0, song: hits[i]?.song ?? 0 }),
    [hits],
  );
  const playMine = useCallback((i: number) => void call("play_target", { target: mineTarget(i) }), [mineTarget]);
  // Where it lives says which copy plays, and that it is already yours.
  const mineLine = useCallback(
    (t: Track, i: number) => [artistNames(t), playlists[hits[i]?.playlist ?? -1]?.title].filter(Boolean).join(" · "),
    [hits, playlists],
  );

  const nothing =
    results !== null &&
    hits.length === 0 &&
    [results.songs, results.videos, results.albums, results.artists].every((l) => l.length === 0);

  return (
    <div className="@container select-none">
      <header className="px-3 pt-1 pb-6">
        <p className="text-label font-semibold tracking-wider text-ink-faint uppercase">Search</p>
        <h1 className="truncate text-display leading-tight font-bold tracking-tight text-ink">“{query}”</h1>
      </header>

      {error ? (
        <div className="flex flex-col items-center gap-3 py-16 text-center">
          <p className="text-heading text-ink-dim">Search didn't go through.</p>
          <p className="max-w-md text-meta text-ink-faint">{error}</p>
          <button onClick={() => setAttempt((n) => n + 1)} className="btn btn-secondary">
            <RotateCw size={13} /> Try again
          </button>
        </div>
      ) : nothing ? (
        <div className="flex flex-col items-center gap-2 py-16 text-center">
          <SearchX size={28} className="text-ink-ghost" />
          <p className="text-heading text-ink-dim">No results for “{query}”.</p>
          <p className="text-body text-ink-faint">Check the spelling, or try fewer words.</p>
        </div>
      ) : (
        <>
          {results === null ? (
            <section className="mb-8" aria-busy aria-label="Loading">
              <h2 className="section-heading">Top Results</h2>
              <div className="grid grid-cols-1 gap-2 px-3 @xl:grid-cols-2 @4xl:grid-cols-3">
                {Array.from({ length: TOP }, (_, i) => (
                  <div key={i} className="flex items-center gap-4 rounded-2xl bg-surface p-3" style={{ opacity: 1 - i * 0.12 }}>
                    <span className="h-16 w-16 rounded-lg bg-surface-2" />
                    <span className="flex flex-col gap-2">
                      <span className="h-3 w-32 rounded bg-surface-2" />
                      <span className="h-2.5 w-20 rounded bg-surface-2" />
                    </span>
                  </div>
                ))}
              </div>
            </section>
          ) : (
            top.length > 0 && (
              <section className="mb-8">
                <h2 className="section-heading">Top Results</h2>
                <div className="grid grid-cols-1 gap-2 px-3 @xl:grid-cols-2 @4xl:grid-cols-3">
                  {top.map((t) => {
                    if (t.kind === "page") {
                      const { card } = t;
                      const artist = card.kind === "Artist";
                      return (
                        <TopTile
                          key={card.id}
                          thumbnail={card.thumbnail}
                          round={artist}
                          title={card.title}
                          subtitle={artist ? "Artist" : card.subtitle}
                          onOpen={() => navigate(artist ? { kind: "artist", id: card.id } : { kind: "album", id: card.id })}
                          onMenu={(e) => menu.show(e, artist ? menus.artist(card.id) : menus.album(card.id))}
                        />
                      );
                    }
                    const song = songs[t.index];
                    return (
                      <TopTile
                        key={`${song.video_id}-${t.index}`}
                        thumbnail={song.thumbnail}
                        round={false}
                        title={song.title ?? "Untitled"}
                        subtitle={`Song · ${artistNames(song)}`}
                        playing={{ now: Boolean(song.video_id) && song.video_id === currentTrackId, paused }}
                        onOpen={() => playSong(t.index)}
                        onMenu={(e) => menu.show(e, menus.track(song, songTarget(t.index)))}
                      />
                    );
                  })}
                </div>
              </section>
            )
          )}

          <SongGrid title="In Your Library" tracks={mine} subtitle={mineLine} onPlay={playMine} targetOf={mineTarget} />

          {results && (
            <>
              {results.artists.length > 0 && <CardShelf title="Artists" cards={results.artists} />}
              {results.albums.length > 0 && <CardShelf title="Albums" cards={results.albums} />}
              <SongGrid title="Songs" tracks={songs} subtitle={songLine} onPlay={playSong} targetOf={songTarget} />
              <SongGrid title="Videos" tracks={videos} subtitle={videoLine} onPlay={playVideo} targetOf={videoTarget} />
            </>
          )}
        </>
      )}
    </div>
  );
});
