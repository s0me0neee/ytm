import { memo, useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { RotateCw } from "lucide-react";
import { call } from "../api";
import { PageHeader } from "../components/PageHeader";
import { ArtistLinks, TrackList, TrackSkeleton } from "../components/TrackList";
import { useActions } from "../context";
import { errorText, formatTotal, songCount } from "../format";
import type { AlbumPage, Target } from "../types";

const cache = new Map<string, AlbumPage>();

/** Fetches an album page once per session. */
export async function loadAlbum(id: string): Promise<AlbumPage> {
  const hit = cache.get(id);
  if (hit) return hit;
  const album = await invoke<AlbumPage>("get_album", { browseId: id });
  cache.set(id, album);
  return album;
}

export const AlbumView = memo(function AlbumView({ id }: { id: string }) {
  const { menus } = useActions();
  const [album, setAlbum] = useState<AlbumPage | null>(() => cache.get(id) ?? null);
  const [error, setError] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    let live = true;
    setError(null);
    setAlbum(cache.get(id) ?? null);
    loadAlbum(id)
      .then((a) => live && setAlbum(a))
      .catch((e) => live && setError(errorText(e)));
    return () => {
      live = false;
    };
  }, [id, attempt]);

  const tracks = album?.tracks ?? [];
  const totalSecs = useMemo(() => tracks.reduce((n, t) => n + (t.duration_seconds ?? 0), 0), [tracks]);
  const targetOf = useCallback((i: number): Target => ({ kind: "track", track: tracks[i] }), [tracks]);
  const play = useCallback((i: number) => void call("play_tracks", { tracks, start: i }), [tracks]);
  const shuffle = useCallback(async () => {
    await call("set_mode", { mode: "shuffle" });
    play(Math.floor(Math.random() * tracks.length));
  }, [play, tracks.length]);

  if (error) {
    return (
      <div className="flex flex-col items-center gap-3 py-24 text-center">
        <p className="text-heading text-ink-dim">Couldn't open this album.</p>
        <p className="max-w-md text-meta text-ink-faint">{error}</p>
        <button onClick={() => setAttempt((n) => n + 1)} className="btn btn-secondary">
          <RotateCw size={13} /> Try again
        </button>
      </div>
    );
  }

  return (
    <div className="@container">
      {album ? (
        <PageHeader
          cover={album.thumbnail}
          eyebrow={[album.kind, album.year].filter(Boolean).join(" · ")}
          title={album.title}
          subtitle={<ArtistLinks artists={album.artists} className="block text-ink-dim" />}
          meta={tracks.length > 0 ? `${songCount(tracks.length)} · ${formatTotal(totalSecs)}` : undefined}
          onPlay={tracks.length > 0 ? () => play(0) : undefined}
          onShuffle={tracks.length > 1 ? () => void shuffle() : undefined}
          menu={() => menus.album(id)}
        />
      ) : (
        <header className="flex items-end gap-6 px-3 pt-1 pb-6" aria-busy>
          <span className="h-36 w-36 rounded-2xl bg-surface-2" />
          <span className="flex flex-col gap-3 pb-2">
            <span className="h-2.5 w-20 rounded bg-surface" />
            <span className="h-6 w-64 rounded bg-surface-2" />
            <span className="h-3 w-32 rounded bg-surface" />
          </span>
        </header>
      )}
      {album ? (
        <TrackList tracks={tracks} variant="album" onPlay={play} targetOf={targetOf} />
      ) : (
        <TrackSkeleton variant="album" />
      )}
    </div>
  );
});
