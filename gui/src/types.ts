export interface Artist {
  name: string;
  id: string | null;
}

export interface Album {
  name: string;
  id: string | null;
}

export interface Track {
  video_id: string | null;
  title: string | null;
  artists: Artist[];
  album: Album | null;
  duration: string | null;
  duration_seconds: number | null;
  thumbnail: string | null;
  set_video_id?: string | null;
}

export interface PlaylistView {
  playlist_id: string;
  title: string;
  count: number | null;
  loaded: boolean;
  failed: boolean;
  thumbnail: string | null;
}

export interface SearchResult {
  video_id: string;
  title: string;
  artist: string;
  album: string;
  duration: string;
  duration_seconds: number | null;
  kind: "Song" | "Video";
  video_type: string;
  thumbnail: string | null;
  artists: Artist[];
  album_id: string | null;
}

export interface PlaybackStateView {
  elapsed: number;
  total: number;
  paused: boolean;
  loading: boolean;
  error: string | null;
  track: string | null;
  playing: [number, number] | null;
  queue_position: number | null;
  queue_len: number;
  queue_revision: number;
  volume: number;
  effective_volume: number;
  muted: boolean;
  mode: string;
}

export interface PlayedTrack {
  track: Track;
  playlist_id: string | null;
  played_at: number;
}

export interface HistoryView {
  tracks: PlayedTrack[];
}

export interface LyricLineView {
  at: number;
  text: string;
}

export interface LyricsView {
  synced: boolean;
  lines: LyricLineView[];
  recordId: number;
  overridden: boolean;
}

export interface LyricsChoice {
  id: number;
  trackName: string;
  artistName: string;
  albumName: string;
  duration: number | null;
  synced: boolean;
  lineCount: number;
  timingMismatch: boolean;
}

export interface ConfigView {
  lyricsOffset: number;
  translateTo: string;
  aiAvailable: boolean;
}

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
  track: Track | null;
}

export interface AlbumPage {
  browse_id: string;
  title: string;
  kind: string;
  year: string | null;
  artists: Artist[];
  thumbnail: string | null;
  tracks: Track[];
}

export interface Card {
  kind: "Album" | "Artist";
  id: string;
  title: string;
  subtitle: string;
  thumbnail: string | null;
}

export interface ArtistPage {
  channel_id: string;
  name: string;
  thumbnail: string | null;
  description: string | null;
  songs: Track[];
  shelves: { title: string; cards: Card[] }[];
}

/** What a menu action is aimed at; mirrors `actions::Target`. */
export type Target =
  | { kind: "library"; playlist: number; song: number }
  | { kind: "track"; track: Track }
  | { kind: "history"; index: number };

export type TranslateMode = "off" | "free" | "ai";

export type Route =
  | { kind: "home" }
  | { kind: "playlist"; index: number }
  | { kind: "search"; query: string }
  | { kind: "album"; id: string }
  | { kind: "artist"; id: string };

/** The synthetic playlist search and radio tracks are filed under. */
export const SEARCH_PLAYLIST_ID = "__search__";
/** Liked Music, which is the like button rather than a playlist to add to. */
export const LIKED_PLAYLIST_ID = "LM";
