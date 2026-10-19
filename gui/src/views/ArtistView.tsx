import { memo, useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Radio, RotateCw } from "lucide-react";
import { call } from "../api";
import { CardShelf } from "../components/CardShelf";
import { PageHeader } from "../components/PageHeader";
import { SongGrid } from "../components/SongGrid";
import { TrackSkeleton } from "../components/TrackList";
import { useActions } from "../context";
import { artistNames, errorText } from "../format";
import type { ArtistPage, Target, Track } from "../types";

const cache = new Map<string, ArtistPage>();

// The artist is the page's own name, so the album is what tells one top song from the next.
const albumOrArtist = (t: Track) => t.album?.name ?? artistNames(t);

export const ArtistView = memo(function ArtistView({ id }: { id: string }) {
  const { menus } = useActions();
  const [artist, setArtist] = useState<ArtistPage | null>(() => cache.get(id) ?? null);
  const [error, setError] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);
  const [aboutOpen, setAboutOpen] = useState(false);

  useEffect(() => {
    let live = true;
    setError(null);
    setAboutOpen(false);
    const hit = cache.get(id);
    setArtist(hit ?? null);
    if (hit && attempt === 0) return;
    invoke<ArtistPage>("get_artist", { channelId: id })
      .then((a) => {
        cache.set(id, a);
        if (live) setArtist(a);
      })
      .catch((e) => live && setError(errorText(e)));
    return () => {
      live = false;
    };
  }, [id, attempt]);

  const songs = artist?.songs ?? [];
  const targetOf = useCallback((i: number): Target => ({ kind: "track", track: songs[i] }), [songs]);
  const play = useCallback((i: number) => void call("play_tracks", { tracks: songs, start: i }), [songs]);
  const shuffle = useCallback(async () => {
    await call("set_mode", { mode: "shuffle" });
    play(Math.floor(Math.random() * songs.length));
  }, [play, songs.length]);

  if (error) {
    return (
      <div className="flex flex-col items-center gap-3 py-24 text-center">
        <p className="text-heading text-ink-dim">Couldn't open this artist.</p>
        <p className="max-w-md text-meta text-ink-faint">{error}</p>
        <button onClick={() => setAttempt((n) => n + 1)} className="btn btn-secondary">
          <RotateCw size={13} /> Try again
        </button>
      </div>
    );
  }

  if (!artist) {
    return (
      <div aria-busy>
        <header className="flex items-end gap-6 px-3 pt-1 pb-6">
          <span className="h-36 w-36 rounded-full bg-surface-2" />
          <span className="flex flex-col gap-3 pb-2">
            <span className="h-2.5 w-16 rounded bg-surface" />
            <span className="h-7 w-56 rounded bg-surface-2" />
          </span>
        </header>
        <TrackSkeleton variant="plain" rows={5} />
      </div>
    );
  }

  return (
    <div className="@container">
      <PageHeader
        cover={artist.thumbnail}
        round
        eyebrow="Artist"
        title={artist.name}
        onPlay={songs.length > 0 ? () => play(0) : undefined}
        onShuffle={songs.length > 1 ? () => void shuffle() : undefined}
        menu={() => menus.artist(id)}
      >
        {songs[0] && (
          <button
            onClick={() => void call("radio_target", { target: { kind: "track", track: songs[0] } })}
            className="btn btn-secondary"
          >
            <Radio size={13} /> Radio
          </button>
        )}
      </PageHeader>

      <SongGrid title="Top songs" tracks={songs} subtitle={albumOrArtist} onPlay={play} targetOf={targetOf} />

      {artist.shelves.map((s) => (
        <CardShelf key={s.title} title={s.title} cards={s.cards} />
      ))}

      {artist.description && (
        <section className="mb-8 px-3">
          <h2 className="section-heading px-0">About</h2>
          <p className={`max-w-3xl text-body leading-relaxed whitespace-pre-line text-ink-dim ${aboutOpen ? "" : "line-clamp-4"}`}>
            {artist.description}
          </p>
          <button onClick={() => setAboutOpen((v) => !v)} className="mt-1 text-meta text-ink hover:underline">
            {aboutOpen ? "Less" : "More"}
          </button>
        </section>
      )}
    </div>
  );
});
