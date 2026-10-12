import type { SearchResult, Track } from "./types";

export function artistNames(t: Track): string {
  return t.artists.map((a) => a.name).join(", ");
}

/** A position inside one track: `3:07`. */
export function formatTime(secs: number): string {
  if (!Number.isFinite(secs) || secs < 0) return "0:00";
  const m = Math.floor(secs / 60);
  const s = Math.floor(secs % 60);
  return `${m}:${s.toString().padStart(2, "0")}`;
}

/** A list's total length: "3 hr 6 min", where seconds would be noise. */
export function formatTotal(secs: number): string {
  const mins = Math.round(secs / 60);
  if (mins < 60) return `${mins} min`;
  const h = Math.floor(mins / 60);
  const m = mins % 60;
  return m === 0 ? `${h} hr` : `${h} hr ${m} min`;
}

export function songCount(n: number): string {
  return `${n} song${n === 1 ? "" : "s"}`;
}

export function greeting(): string {
  const h = new Date().getHours();
  if (h < 5) return "Good night";
  if (h < 12) return "Good morning";
  if (h < 18) return "Good afternoon";
  return "Good evening";
}

/** A search hit as the `Track` every list and menu speaks. */
export function resultTrack(r: SearchResult): Track {
  return {
    video_id: r.video_id,
    title: r.title,
    artists: r.artists.length > 0 ? r.artists : [{ name: r.artist, id: null }],
    album: r.album ? { name: r.album, id: r.album_id } : null,
    duration: r.duration || null,
    duration_seconds: r.duration_seconds,
    thumbnail: r.thumbnail,
  };
}

/** A failed command's message, without the transport's wrapping. */
export function errorText(e: unknown): string {
  const s = e instanceof Error ? e.message : String(e);
  return s.replace(/^Error:\s*/, "");
}
