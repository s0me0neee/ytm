import { memo, useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { RotateCw, SearchX } from "lucide-react";
import { call } from "../api";
import { TrackList, TrackSkeleton } from "../components/TrackList";
import { errorText, resultTrack } from "../format";
import type { SearchResult, Target, Track } from "../types";

// Going back to a search shows it again at once rather than asking YouTube twice.
const cache = new Map<string, SearchResult[]>();

function Section({ title, tracks }: { title: string; tracks: Track[] }) {
  const targetOf = useCallback((i: number): Target => ({ kind: "track", track: tracks[i] }), [tracks]);
  const play = useCallback((i: number) => void call("play_tracks", { tracks, start: i }), [tracks]);
  if (tracks.length === 0) return null;
  return (
    <section className="mb-8">
      <h2 className="section-heading">{title}</h2>
      <TrackList tracks={tracks} variant="plain" onPlay={play} targetOf={targetOf} />
    </section>
  );
}

/** Results for one query, songs (album audio) apart from videos, since they are different things. */
export const SearchPage = memo(function SearchPage({ query }: { query: string }) {
  const [results, setResults] = useState<SearchResult[] | null>(() => cache.get(query) ?? null);
  const [error, setError] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    let live = true;
    setError(null);
    const hit = cache.get(query);
    if (hit && attempt === 0) {
      setResults(hit);
      return;
    }
    setResults(null);
    invoke<SearchResult[]>("search", { query })
      .then((found) => {
        cache.set(query, found);
        if (live) setResults(found);
      })
      .catch((e) => live && setError(errorText(e)));
    return () => {
      live = false;
    };
  }, [query, attempt]);

  const songs = useMemo(() => (results ?? []).filter((r) => r.kind === "Song").map(resultTrack), [results]);
  const videos = useMemo(() => (results ?? []).filter((r) => r.kind === "Video").map(resultTrack), [results]);

  return (
    <div className="@container select-none">
      <header className="px-3 pt-1 pb-6">
        <p className="text-label font-semibold tracking-wider text-ink-faint uppercase">Search</p>
        <h1 className="truncate text-display leading-tight font-bold tracking-tight text-ink">“{query}”</h1>
        {results && results.length > 0 && (
          <p className="mt-1 text-body text-ink-faint">
            {songs.length} songs · {videos.length} videos
          </p>
        )}
      </header>

      {error ? (
        <div className="flex flex-col items-center gap-3 py-16 text-center">
          <p className="text-heading text-ink-dim">Search didn't go through.</p>
          <p className="max-w-md text-meta text-ink-faint">{error}</p>
          <button onClick={() => setAttempt((n) => n + 1)} className="btn btn-secondary">
            <RotateCw size={13} /> Try again
          </button>
        </div>
      ) : results === null ? (
        <>
          <h2 className="section-heading">Songs</h2>
          <TrackSkeleton variant="plain" rows={6} />
        </>
      ) : results.length === 0 ? (
        <div className="flex flex-col items-center gap-2 py-16 text-center">
          <SearchX size={28} className="text-ink-ghost" />
          <p className="text-heading text-ink-dim">No results for “{query}”.</p>
          <p className="text-body text-ink-faint">Check the spelling, or try fewer words.</p>
        </div>
      ) : (
        <>
          <Section title="Songs" tracks={songs} />
          <Section title="Videos" tracks={videos} />
        </>
      )}
    </div>
  );
});
