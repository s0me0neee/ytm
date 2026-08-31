import { memo, useCallback, useEffect, useMemo, useRef, useState, type RefObject } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { AnimatePresence, motion } from "motion/react";
import {
  Play,
  Pause,
  SkipBack,
  SkipForward,
  Volume2,
  VolumeX,
  ChevronDown,
  ChevronUp,
  Heart,
  Search,
  Repeat,
  Repeat1,
  Shuffle,
  MessageSquareText,
  RotateCw,
  Languages,
  ListMusic,
  Filter,
  Home,
  X,
} from "lucide-react";
import { Slider } from "./Slider";
import { Thumbnail } from "./Thumbnail";
import { ContextMenu } from "./ContextMenu";
import type { MenuItem, MenuState } from "./ContextMenu";
import { QueuePanel } from "./QueuePanel";
import type { QueueEntry } from "./QueuePanel";
import { bestCoverUrl, coverCandidates } from "./cover";
import "./App.css";

interface PlaylistView {
  playlist_id: string;
  title: string;
  count: number | null;
  loaded: boolean;
  failed: boolean;
}

interface Artist {
  name: string;
}

interface Album {
  name: string;
  id: string | null;
}

interface Track {
  video_id: string | null;
  title: string | null;
  artists: Artist[];
  album: Album | null;
  duration: string | null;
  duration_seconds: number | null;
  thumbnail: string | null;
}

interface SearchResult {
  video_id: string;
  title: string;
  artist: string;
  album: string;
  duration: string;
  duration_seconds: number | null;
  kind: "Song" | "Video";
  video_type: string;
  thumbnail: string | null;
}

interface PlaybackStateView {
  elapsed: number;
  total: number;
  paused: boolean;
  loading: boolean;
  error: string | null;
  track: string | null;
  /** The queue entry playing, not the queue -- see `PlaybackStateView`. */
  playing: [number, number] | null;
  queue_position: number | null;
  queue_len: number;
  queue_revision: number;
  volume: number;
  effective_volume: number;
  muted: boolean;
  mode: string;
}

/** One row of the home page's "Recently played".
 *
 * Carries the whole `Track` rather than a reference to one: a `TrackRef` is a
 * position and means nothing across a restart, and a song played from search
 * belongs to no playlist that will exist next time. So a row draws with no
 * library loaded at all -- which is what the home page needs, being the first
 * thing on screen while playlists are still arriving. */
interface PlayedTrack {
  track: Track;
  /** Where it was played from, when that was a real playlist. */
  playlist_id: string | null;
  played_at: number;
}

interface HistoryView {
  tracks: PlayedTrack[];
}

interface LyricLineView {
  at: number;
  text: string;
}

interface LyricsView {
  synced: boolean;
  lines: LyricLineView[];
  /** The lrclib record the words came from -- what a translation is keyed on. */
  recordId: number;
  overridden: boolean;
}

/** One row of the `c` picker. */
interface LyricsChoice {
  id: number;
  trackName: string;
  artistName: string;
  albumName: string;
  duration: number | null;
  synced: boolean;
  lineCount: number;
  timingMismatch: boolean;
}

/** The parts of `config.toml` the frontend acts on. */
interface ConfigView {
  lyricsOffset: number;
  translateTo: string;
  aiAvailable: boolean;
}

/** Which translator is showing, if any. `i` and `I` in the TUI. */
type TranslateMode = "off" | "free" | "ai";

/** Track-list sort. `none` is the playlist's own order, which is meaningful
 * (it is the order the user or the service put it in) and so is the default
 * and the state a third click on a column returns to. */
type SortKey = "none" | "title" | "artist" | "album" | "duration";
type SortDir = "asc" | "desc";

/** The grid the header and every row share, so the columns line up without
 * either measuring the other. Album is dropped first on a narrow window, then
 * artist -- title and time are the two that always earn their place. */
const TRACK_GRID =
  "grid grid-cols-[1.25rem_2.75rem_minmax(0,3fr)_3.5rem] md:grid-cols-[1.25rem_2.75rem_minmax(0,3fr)_minmax(0,2fr)_3.5rem] lg:grid-cols-[1.25rem_2.75rem_minmax(0,3fr)_minmax(0,2fr)_minmax(0,2fr)_3.5rem] items-center gap-3";

/** macOS is the only platform where `titleBarStyle: "Overlay"` applies, so it
 * is the only one whose header has to leave room for the traffic lights. */
const IS_MAC = navigator.userAgent.includes("Mac OS X");

/* Where the traffic lights sit, and therefore what the headers have to clear.
 * `trafficLightPosition` in tauri.conf.json places the group's top-left at
 * (20, 21); the group is 52px wide (three 12px buttons, 20px apart) and 12px
 * tall, so it occupies x 20..72 and is centred on y 27.
 *
 * 27 is not a free choice: the header is `pt-3` over a ~30px search field, so
 * the field's centre line is at 12 + 15 = 27 and the lights have to meet it.
 * Re-derive it if either the padding or the field's height changes. */
const TRAFFIC_LIGHT_SPAN = "pl-20"; // 80px, just past the lights' right edge

/* A header whose contents sit on that same centre line, whatever height they
 * happen to be. The lyrics view's control is a 32px round button rather than
 * the ~30px search field, so padding it by the same `pt-3` put it a pixel
 * below the lights -- visible, since the two sit side by side with nothing
 * between them. 54px of height with `items-center` centres on 27 by
 * construction instead, so the button can be resized without re-deriving. */
const TRAFFIC_LIGHT_AXIS = "h-[54px]";

/* The gap above the search field. The sidebar and the track column below it
 * are both `m-3`/`mt-3`, so using the same 12px here is what makes the space
 * over the field and the space under it equal. */
const HEADER_PAD_TOP = "pt-3";

function artistNames(t: Track): string {
  return t.artists.map((a) => a.name).join(", ");
}

function formatTime(secs: number): string {
  if (!Number.isFinite(secs) || secs < 0) return "0:00";
  const m = Math.floor(secs / 60);
  const s = Math.floor(secs % 60);
  return `${m}:${s.toString().padStart(2, "0")}`;
}

/** Which line is playing at `elapsed`, or -1 before the first timestamp.
 *
 * Binary search, matching `lrclib::lrc::active_index`. This is asked once per
 * playback tick against a sheet that can run to a couple of hundred lines, and
 * the lines are sorted, so there is no reason to walk them. */
function activeLyricIndex(lines: LyricLineView[], elapsed: number): number {
  let lo = 0;
  let hi = lines.length - 1;
  let idx = -1;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (lines[mid].at <= elapsed) {
      idx = mid;
      lo = mid + 1;
    } else {
      hi = mid - 1;
    }
  }
  return idx;
}

function modeIcon(mode: string) {
  const m = mode.toLowerCase();
  if (m.includes("shuffle")) return <Shuffle size={16} />;
  if (m.includes("repeat one") || m === "one") return <Repeat1 size={16} />;
  return <Repeat size={16} />;
}

/** Three bars that bounce while playing and settle flat when paused --
 * replaces the track number for whichever row is currently loaded.
 *
 * The bounce is a CSS animation on `transform: scaleY`, and both halves of
 * that matter. It animated `height` from JS before (framer-motion, `repeat:
 * Infinity`), and height is a layout property: three bars x every frame x for
 * as long as a song plays meant the main thread ran a layout and paint pass
 * continuously, which is what made hovering anything feel like it was
 * catching. `scaleY` is composited on the GPU and touches neither, and as a
 * CSS animation it keeps running without the main thread at all. */
const PlayingIndicator = memo(function PlayingIndicator({ paused }: { paused: boolean }) {
  return (
    <span className="flex h-3.5 w-4 items-end justify-center gap-0.5" aria-hidden>
      {[0, 1, 2].map((i) => (
        <span
          key={i}
          className={`h-3 w-0.5 rounded-full bg-accent ${paused ? "eq-bar-idle" : "eq-bar"}`}
          style={{ animationDuration: `${0.9 + i * 0.2}s`, animationDelay: `${i * 0.12}s` }}
        />
      ))}
    </span>
  );
});

interface SearchResultsListProps {
  results: SearchResult[];
  onPlayResult: (r: SearchResult) => void;
  onContextMenu: (e: React.MouseEvent, r: SearchResult) => void;
}

/** Memoized so typing in the search box or a playback tick elsewhere on the
 * page doesn't re-render every result row.
 *
 * Rows are plain `<li>`s. They used to be `motion.li`s with a staggered entry
 * animation, which meant one animation driver and one layout pass per row --
 * a few hundred of them the moment a result set or a large playlist landed,
 * all competing with click handling for the main thread. `TrackList` below
 * dropped the same thing for the same reason. */
const SearchResultsList = memo(function SearchResultsList({
  results,
  onPlayResult,
  onContextMenu,
}: SearchResultsListProps) {
  return (
    <>
      {results.map((r, i) => (
        <li
          key={`${r.video_id}-${i}`}
          className="row group relative flex items-center rounded-xl transition-colors hover:bg-surface"
          onContextMenu={(e) => onContextMenu(e, r)}
        >
          {/* The whole row is the play target. It used to be only the title
              block, so a click on the artwork, the duration or the padding
              between them did nothing -- which reads as the first click of a
              pair having been swallowed. */}
          <button
            className="flex w-full min-w-0 items-center gap-3 py-2 pr-11 pl-3 text-left"
            onClick={() => onPlayResult(r)}
          >
            <Thumbnail srcs={[r.thumbnail]} className="h-11 w-11 flex-shrink-0 rounded-md object-cover" />
            <span className="min-w-0 flex-1">
              <p className="truncate text-[13px] text-ink">{r.title}</p>
              <p className="truncate text-xs text-ink-dim">{r.artist}</p>
            </span>
            <span className="hidden flex-shrink-0 font-mono text-xs text-ink-faint sm:inline">{r.duration}</span>
          </button>
          <button
            onClick={() => invoke("like_track", { videoId: r.video_id })}
            className="absolute top-1/2 right-3 -translate-y-1/2 text-ink-faint opacity-0 transition hover:text-accent group-hover:opacity-100"
            aria-label="Like"
          >
            <Heart size={16} />
          </button>
        </li>
      ))}
    </>
  );
});

interface TrackHeaderProps {
  sortKey: SortKey;
  sortDir: SortDir;
  onSort: (k: SortKey) => void;
}

/** The column header. Clicking a column sorts by it, clicking it again
 * reverses, and a third click returns to the playlist's own order. */
function TrackHeader({ sortKey, sortDir, onSort }: TrackHeaderProps) {
  const col = (k: SortKey, label: string, extra = "") => (
    <button
      onClick={() => onSort(k)}
      className={`flex min-w-0 items-center gap-1 text-left text-[11px] font-semibold tracking-wider uppercase transition-colors ${
        sortKey === k ? "text-ink" : "text-ink-faint hover:text-ink-dim"
      } ${extra}`}
    >
      <span className="truncate">{label}</span>
      {sortKey === k && (sortDir === "asc" ? <ChevronUp size={11} /> : <ChevronDown size={11} />)}
    </button>
  );

  return (
    <div className={`${TRACK_GRID} border-b-[0.5px] border-hairline px-3 pb-2 select-none`}>
      <span />
      <span />
      {col("title", "Song")}
      {col("artist", "Artist", "hidden md:flex")}
      {col("album", "Album", "hidden lg:flex")}
      {col("duration", "Time", "justify-end")}
    </div>
  );
}

interface TrackListProps {
  songs: Track[];
  currentTrackId: string | null | undefined;
  paused: boolean;
  onPlaySong: (i: number) => void;
  onHoverSong: (i: number) => void;
  onContextMenu: (e: React.MouseEvent, i: number) => void;
}

/** Memoized against the playback ticker: `paused` and `currentTrackId` only
 * change on real track/transport events, not on every ~250ms elapsed tick,
 * so this list of (possibly hundreds of) rows stays untouched while a song
 * plays instead of re-rendering four times a second. */
const TrackList = memo(function TrackList({
  songs,
  currentTrackId,
  paused,
  onPlaySong,
  onHoverSong,
  onContextMenu,
}: TrackListProps) {
  return (
    <>
      {songs.map((t, i) => {
        const isPlaying = Boolean(t.video_id) && t.video_id === currentTrackId;
        return (
          <li key={`${t.video_id ?? "row"}-${i}`} className="row">
            <button
              className={`${TRACK_GRID} w-full rounded-xl px-3 py-1.5 text-left transition-colors hover:bg-surface ${
                isPlaying ? "bg-surface" : ""
              }`}
              onClick={() => onPlaySong(i)}
              onMouseEnter={() => onHoverSong(i)}
              onContextMenu={(e) => onContextMenu(e, i)}
            >
              <span className="flex items-center justify-end">
                {isPlaying ? (
                  <PlayingIndicator paused={paused} />
                ) : (
                  <span className="font-mono text-xs text-ink-faint">{i + 1}</span>
                )}
              </span>
              <Thumbnail srcs={[t.thumbnail]} className="h-11 w-11 rounded-md object-cover" />
              <span className={`truncate text-[13px] ${isPlaying ? "text-accent" : "text-ink"}`}>
                {t.title ?? "Untitled"}
              </span>
              {/* Below `md` the artist folds under the title instead of being
                  lost, since it is the one column a song is unidentifiable
                  without. Album just goes. */}
              <span className="hidden truncate text-[13px] text-ink-dim md:block">{artistNames(t)}</span>
              <span className="hidden truncate text-[13px] text-ink-dim lg:block">{t.album?.name ?? ""}</span>
              <span className="text-right font-mono text-xs text-ink-faint">{t.duration ?? ""}</span>
            </button>
          </li>
        );
      })}
    </>
  );
});

interface HomeViewProps {
  history: HistoryView | null;
  currentTrackId: string | null | undefined;
  paused: boolean;
  onPlayTrack: (index: number) => void;
}

/** The home page: what has been played lately, and nothing else.
 *
 * This is what the main pane shows when no playlist is selected, which is the
 * state the app starts in -- so it fills a space that was previously an empty
 * "Tracks" heading, and it does it without waiting for the library, since
 * `history.json` is read at startup and every row carries its own metadata.
 *
 * Memoized for the same reason as `TrackList`: it is a child of `LibraryView`,
 * whose parent re-renders on the ~250ms playback tick. `history` changes once
 * per song and the rest only on a real action. */
const HomeView = memo(function HomeView({ history, currentTrackId, paused, onPlayTrack }: HomeViewProps) {
  const tracks = history?.tracks ?? [];

  if (history && tracks.length === 0) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-2 text-center select-none">
        <p className="text-[15px] text-ink-dim">Nothing played yet.</p>
        <p className="text-[13px] text-ink-faint">Pick a playlist on the left to get started.</p>
      </div>
    );
  }

  return (
    <div className="select-none">
      {tracks.length > 0 && (
        <>
          <p className="px-3 pb-2 text-[11px] font-semibold tracking-wider text-ink-faint uppercase">
            Recently played
          </p>
          <ul>
            {tracks.map((p, i) => {
              const t = p.track;
              const isPlaying = Boolean(t.video_id) && t.video_id === currentTrackId;
              return (
                <li key={`${t.video_id ?? "row"}-${i}`} className="row">
                  <button
                    className={`${TRACK_GRID} w-full rounded-xl px-3 py-1.5 text-left transition-colors hover:bg-surface ${
                      isPlaying ? "bg-surface" : ""
                    }`}
                    onClick={() => onPlayTrack(i)}
                  >
                    <span className="flex items-center justify-end">
                      {isPlaying ? (
                        <PlayingIndicator paused={paused} />
                      ) : (
                        <span className="font-mono text-xs text-ink-faint">{i + 1}</span>
                      )}
                    </span>
                    <Thumbnail srcs={[t.thumbnail]} className="h-11 w-11 rounded-md object-cover" />
                    <span className={`truncate text-[13px] ${isPlaying ? "text-accent" : "text-ink"}`}>
                      {t.title ?? "Untitled"}
                    </span>
                    <span className="hidden truncate text-[13px] text-ink-dim md:block">{artistNames(t)}</span>
                    <span className="hidden truncate text-[13px] text-ink-dim lg:block">{t.album?.name ?? ""}</span>
                    <span className="text-right font-mono text-xs text-ink-faint">{t.duration ?? ""}</span>
                  </button>
                </li>
              );
            })}
          </ul>
        </>
      )}
    </div>
  );
});

interface PlaylistNavProps {
  playlists: PlaylistView[];
  selected: number | null;
  onSelect: (i: number | null) => void;
  onRetry: (i: number) => void;
  onContextMenu: (e: React.MouseEvent, i: number) => void;
}

/** Memoized against the playback ticker for the same reason as `TrackList`:
 * this lived inline in `LibraryView`, which re-renders on every ~250ms
 * `playback` tick (needed for the footer scrubber). The `layoutId` sliding
 * highlight below triggers Framer Motion's shared-layout remeasurement on
 * every render it appears in, so left inline it was remeasuring 4x/second
 * regardless of whether the selected playlist changed -- competing with
 * click handling for the main thread. Pulling it out with props that only
 * change on a real selection change stops that. */
const PlaylistNav = memo(function PlaylistNav({
  playlists,
  selected,
  onSelect,
  onRetry,
  onContextMenu,
}: PlaylistNavProps) {
  return (
    <nav className="thin-scrollbar m-3 w-60 flex-shrink-0 overflow-y-auto rounded-2xl p-2 select-none glass">
      {/* `null` is Home, which is also where the app starts -- so this is a
          way *back* to a page the user was already on rather than a new
          destination, and it sits above the list for that reason. It shares
          the `layoutId` highlight with the playlists below, so the selection
          slides between them as one control. */}
      <div className="relative">
        {selected === null && (
          <motion.div
            layoutId="playlist-active"
            transition={{ duration: 0.2, ease: [0.22, 1, 0.36, 1] }}
            className="pointer-events-none absolute inset-0 rounded-xl bg-surface-2"
          />
        )}
        <button
          onClick={() => onSelect(null)}
          className={`relative flex w-full items-center gap-2 rounded-xl px-3 py-2 text-left text-[13px] transition-colors ${
            selected === null ? "text-ink" : "text-ink-dim hover:bg-surface hover:text-ink"
          }`}
        >
          <Home size={14} className="flex-shrink-0" />
          <span className="min-w-0 flex-1 truncate">Home</span>
        </button>
      </div>
      <p className="px-3 pt-3 pb-2 text-[11px] font-semibold tracking-wider text-ink-faint uppercase">Playlists</p>
      {playlists.length === 0 && <p className="px-3 text-sm text-ink-dim">Loading…</p>}
      <ul>
        {playlists.map((p, i) => (
          <li key={p.playlist_id} className="relative" onContextMenu={(e) => onContextMenu(e, i)}>
            {selected === i && (
              <motion.div
                layoutId="playlist-active"
                transition={{ duration: 0.2, ease: [0.22, 1, 0.36, 1] }}
                className="pointer-events-none absolute inset-0 rounded-xl bg-surface-2"
              />
            )}
            <button
              onClick={() => onSelect(i)}
              className={`relative flex w-full items-center gap-2 rounded-xl px-3 py-2 text-left text-[13px] transition-colors ${
                selected === i ? "text-ink" : "text-ink-dim hover:bg-surface hover:text-ink"
              }`}
            >
              <span className="min-w-0 flex-1 truncate">{p.title}</span>
              {p.count !== null && !p.failed && (
                <span className="font-mono text-[11px] text-ink-ghost">{p.count}</span>
              )}
              {!p.loaded && !p.failed && <span className="text-ink-ghost">…</span>}
            </button>
            {/* A failed fetch leaves the playlist *unloaded* rather than
                loaded-and-empty, so the only useful thing to offer is another
                go at it -- the TUI's `r`. */}
            {p.failed && (
              <button
                onClick={() => onRetry(i)}
                title="Couldn't load this playlist — try again"
                className="absolute top-1/2 right-2 flex -translate-y-1/2 items-center gap-1 rounded-md px-1.5 py-0.5 text-[11px] text-accent transition-colors hover:bg-surface-2"
              >
                <RotateCw size={11} /> Retry
              </button>
            )}
          </li>
        ))}
      </ul>
    </nav>
  );
});

const SEEK_STEP = 5;
const VOLUME_STEP = 5;

function App() {
  const [signedIn, setSignedIn] = useState<boolean | null>(null);
  const [browsers, setBrowsers] = useState<string[]>([]);
  const [browser, setBrowser] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [playlists, setPlaylists] = useState<PlaylistView[]>([]);
  const [selected, setSelected] = useState<number | null>(null);
  const [songs, setSongs] = useState<Track[]>([]);
  const [playback, setPlayback] = useState<PlaybackStateView | null>(null);
  const [currentTrack, setCurrentTrack] = useState<Track | null>(null);
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<SearchResult[] | null>(null);
  const [view, setView] = useState<"library" | "now-playing">("library");
  const [lyrics, setLyrics] = useState<LyricsView | null>(null);
  const [lyricsError, setLyricsError] = useState(false);
  const [lyricsLoading, setLyricsLoading] = useState(false);
  const [config, setConfig] = useState<ConfigView | null>(null);
  const [filter, setFilter] = useState("");
  const [translateMode, setTranslateMode] = useState<TranslateMode>("off");
  const [translation, setTranslation] = useState<string[] | null>(null);
  const [translateBusy, setTranslateBusy] = useState(false);
  const [choices, setChoices] = useState<LyricsChoice[] | null>(null);
  const [pickerOpen, setPickerOpen] = useState(false);
  const [sortKey, setSortKey] = useState<SortKey>("none");
  const [sortDir, setSortDir] = useState<SortDir>("asc");
  const [queue, setQueue] = useState<QueueEntry[]>([]);
  const [showQueue, setShowQueue] = useState(false);
  const [menu, setMenu] = useState<MenuState | null>(null);
  const [history, setHistory] = useState<HistoryView | null>(null);

  const playbackRef = useRef<PlaybackStateView | null>(null);
  useEffect(() => {
    playbackRef.current = playback;
  }, [playback]);

  const currentTrackRef = useRef<Track | null>(null);
  useEffect(() => {
    currentTrackRef.current = currentTrack;
  }, [currentTrack]);

  function refreshPlaylists() {
    invoke<PlaylistView[]>("get_playlists").then(setPlaylists);
  }

  // `library-song-batch` fires once per playlist, and they land in a burst
  // while the library loads -- each one previously cost an IPC round trip and
  // a fresh array that invalidated the sidebar's memo. Coalescing them means
  // one refresh per burst instead of one per playlist.
  const refreshTimer = useRef<number | undefined>(undefined);
  function scheduleRefresh() {
    window.clearTimeout(refreshTimer.current);
    refreshTimer.current = window.setTimeout(refreshPlaylists, 150);
  }

  useEffect(() => {
    invoke<boolean>("auth_status").then((ok) => {
      setSignedIn(ok);
      if (ok) refreshPlaylists();
    });
    invoke<string[]>("list_browsers").then((list) => {
      setBrowsers(list);
      setBrowser(list[0] ?? "");
    });
    invoke<PlaybackStateView | null>("playback_state").then(setPlayback);
    invoke<ConfigView>("get_config").then(setConfig);
    // Read from `history.json` at startup, so the home page has content before
    // a single playlist has arrived.
    invoke<HistoryView>("get_history").then(setHistory);

    // bootstrap() on the Rust side can finish (and emit library-loaded)
    // before this webview has loaded far enough to register the listener
    // below -- events aren't replayed for late subscribers, so this poll is
    // the fallback that guarantees the initial fetch is eventually seen.
    //
    // It stops the moment the library arrives by either route, rather than
    // running its full 15s regardless: once there are playlists the fallback
    // has done its job, and every further tick is an IPC round trip plus a
    // rebuilt array that invalidates the sidebar's memo for no change.
    const poll = setInterval(() => {
      invoke<PlaylistView[]>("get_playlists").then((list) => {
        setPlaylists(list);
        if (list.length > 0) clearInterval(poll);
      });
    }, 1000);
    const stopPolling = setTimeout(() => clearInterval(poll), 15000);

    const unlistenLoaded = listen("library-loaded", () => {
      clearInterval(poll);
      refreshPlaylists();
    });
    const unlistenBatch = listen("library-song-batch", scheduleRefresh);
    const unlistenError = listen<string>("bootstrap-error", (e) => setError(e.payload));
    const unlistenPlayback = listen<PlaybackStateView>("playback-state", (e) => setPlayback(e.payload));
    // Once per song, not per tick -- the backend only emits this when a
    // different video actually starts. See `history::observe`.
    const unlistenHistory = listen("history-changed", () => {
      invoke<HistoryView>("get_history").then(setHistory);
    });
    return () => {
      clearInterval(poll);
      clearTimeout(stopPolling);
      window.clearTimeout(refreshTimer.current);
      unlistenLoaded.then((f) => f());
      unlistenBatch.then((f) => f());
      unlistenError.then((f) => f());
      unlistenPlayback.then((f) => f());
      unlistenHistory.then((f) => f());
    };
  }, []);

  // Standard media shortcuts. Registered once (via a ref for the latest
  // playback snapshot) rather than re-bound on every progress tick.
  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      const tag = (e.target as HTMLElement | null)?.tagName;
      if (tag === "INPUT" || tag === "SELECT" || tag === "TEXTAREA") return;

      const current = playbackRef.current;
      switch (e.key) {
        case " ":
          e.preventDefault();
          invoke("play_pause");
          break;
        case "ArrowLeft":
          e.preventDefault();
          invoke("seek", { deltaSecs: -SEEK_STEP });
          break;
        case "ArrowRight":
          e.preventDefault();
          invoke("seek", { deltaSecs: SEEK_STEP });
          break;
        case "ArrowUp":
          e.preventDefault();
          if (current) invoke("set_volume", { volume: Math.min(100, current.volume + VOLUME_STEP) });
          break;
        case "ArrowDown":
          e.preventDefault();
          if (current) invoke("set_volume", { volume: Math.max(0, current.volume - VOLUME_STEP) });
          break;
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  /* Re-fetch the current track's metadata only when what is playing actually
     changes, not on every progress tick.

     Keyed on the queue entry *and* the video mpv has open, because either can
     move without the other. `track` alone -- what this used to be -- is the
     video mpv was handed, and a queue restored from disk has never been handed
     to it: the player bar read "Nothing playing" over a queue the app had just
     restored, until something was pressed. `playing` alone is not enough
     either, being a `(playlist, song)` *position*: a playlist refetched after
     an edit can put a different track at the same numbers. */
  const playingKey =
    playback && (playback.playing || playback.track)
      ? `${playback.track ?? ""}@${playback.playing?.join(":") ?? ""}`
      : null;
  useEffect(() => {
    if (playingKey === null) {
      setCurrentTrack(null);
      return;
    }
    /* Guarded for the reason `lyricsRequest` and the translation effect below
       are: pressing `next` twice quickly leaves two of these in flight, and
       whichever answers last wins regardless of which was asked last. This one
       corrects itself on the following track change, which is why it went
       unnoticed -- the `get_songs` one below does not. */
    let cancelled = false;
    invoke<Track | null>("current_track").then((t) => {
      if (!cancelled) setCurrentTrack(t);
    });
    return () => {
      cancelled = true;
    };
  }, [playingKey]);

  /* The rows on screen must belong to the playlist that is selected, and two
     things used to be able to break that. The list was left showing the
     previous playlist's tracks for the length of the round trip; and two quick
     clicks could resolve out of order, leaving `songs` and `selected`
     disagreeing for good. Either way `playSong` then sends the *new* playlist
     with an index into the *old* list -- the wrong song plays, with the right
     row highlighted, which is exactly the silent failure the note above
     `songIndexMap` warns about.

     So: clear first, and ignore an answer that is no longer the one being
     waited for. Clearing costs an empty list for a frame of local IPC, which
     is the honest thing to show when we do not yet know what is in there. */
  useEffect(() => {
    if (selected === null) return;
    setFilter("");
    setSongs([]);
    let cancelled = false;
    invoke<Track[]>("get_songs", { index: selected }).then((s) => {
      if (!cancelled) setSongs(s);
    });
    return () => {
      cancelled = true;
    };
  }, [selected]);

  /* Rows are addressed by their position in the *playlist*, not in the view,
     and filtering and sorting both change that mapping -- so the two are
     built together, from the same pass, and every index the list hands back
     goes through `toPlaylistIndex` before it reaches the player. Getting this
     wrong is silent: the wrong song plays, with the right one's row
     highlighted. */
  const songIndexMap = useMemo(() => {
    const q = filter.trim().toLowerCase();
    const map: number[] = [];
    songs.forEach((t, i) => {
      if (!q || (t.title ?? "").toLowerCase().includes(q) || artistNames(t).toLowerCase().includes(q)) {
        map.push(i);
      }
    });
    if (sortKey !== "none") {
      const dir = sortDir === "asc" ? 1 : -1;
      const key = (t: Track) => {
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
        const ka = key(songs[a]);
        const kb = key(songs[b]);
        if (typeof ka === "number" && typeof kb === "number") return (ka - kb) * dir;
        // `localeCompare` rather than `<`, so accented and CJK titles sort the
        // way the user's locale says rather than by code point.
        return String(ka).localeCompare(String(kb), undefined, { numeric: true }) * dir;
      });
    }
    // No filter and no sort means the identity map, which callers can skip.
    return !q && sortKey === "none" ? null : map;
  }, [songs, filter, sortKey, sortDir]);

  /* The TUI's `/` and the column sort, applied together. Memoised for the
     reason `filtered_songs` is there: this is asked for on every render of a
     component a playback tick can wake. */
  const filteredSongs = useMemo(
    () => (songIndexMap ? songIndexMap.map((i) => songs[i]) : songs),
    [songs, songIndexMap],
  );

  const toPlaylistIndex = useCallback(
    (shown: number) => (songIndexMap ? (songIndexMap[shown] ?? shown) : shown),
    [songIndexMap],
  );

  /** A new column sorts ascending; the same column again reverses, and once
   * more returns to the playlist's own order -- which is a real order (the
   * one the user or the service chose), not just "unsorted". */
  const onSort = useCallback(
    (k: SortKey) => {
      if (sortKey !== k) {
        setSortKey(k);
        setSortDir("asc");
      } else if (sortDir === "asc") {
        setSortDir("desc");
      } else {
        setSortKey("none");
        setSortDir("asc");
      }
    },
    [sortKey, sortDir],
  );

  /** Where the playing track sits, which every lyrics call needs. */
  const playingRef = useCallback((): [number, number] | null => {
    const p = playbackRef.current;
    return p?.playing ?? null;
  }, []);

  /** Guards against a slow lookup landing after a newer one -- a retry fired
   * while the previous request is still out, or a track change mid-flight. */
  const lyricsRequest = useRef(0);
  /** The same, for the record picker's own lookup. */
  const choicesRequest = useRef(0);

  const loadLyrics = useCallback(() => {
    const ref = playingRef();
    const seq = ++lyricsRequest.current;
    if (!ref) {
      setLyrics(null);
      setLyricsError(false);
      setLyricsLoading(false);
      return;
    }
    const [playlist, song] = ref;
    setLyricsError(false);
    setLyricsLoading(true);
    invoke<LyricsView | null>("get_lyrics", { playlist, song })
      .then((l) => {
        if (seq !== lyricsRequest.current) return;
        setLyrics(l);
        setLyricsError(l === null);
      })
      .catch(() => {
        if (seq !== lyricsRequest.current) return;
        setLyrics(null);
        setLyricsError(true);
      })
      .finally(() => {
        if (seq === lyricsRequest.current) setLyricsLoading(false);
      });
  }, [playingRef]);

  // The Now Playing view always wants lyrics for whatever's on; fetched once
  // per song, not gated behind a separate toggle. A new song also drops the
  // translation on screen -- it belongs to the previous record's words.
  useEffect(() => {
    setTranslation(null);
    setChoices(null);
    setPickerOpen(false);
    loadLyrics();
  }, [playback?.track]);

  // Fetches whenever the translator or the record on screen changes -- which
  // covers picking a different record with `c`, since a translation belongs to
  // the words rather than to the track.
  useEffect(() => {
    if (translateMode === "off" || !lyrics || lyrics.lines.length === 0) {
      setTranslation(null);
      return;
    }
    let cancelled = false;
    setTranslateBusy(true);
    invoke<{ lines: string[] }>("translate_lyrics", {
      recordId: lyrics.recordId,
      lines: lyrics.lines.map((l) => l.text),
      useAi: translateMode === "ai",
      force: false,
    })
      .then((t) => {
        if (!cancelled) setTranslation(t.lines);
      })
      .catch((e) => {
        if (!cancelled) {
          setTranslation(null);
          setError(String(e));
        }
      })
      .finally(() => {
        if (!cancelled) setTranslateBusy(false);
      });
    return () => {
      cancelled = true;
    };
  }, [translateMode, lyrics?.recordId, lyrics?.lines.length]);

  const openPicker = useCallback(() => {
    const ref = playingRef();
    if (!ref) return;
    const [playlist, song] = ref;
    setPickerOpen(true);
    setChoices(null);
    // Same guard, one level down: the picker can be closed and reopened on a
    // different track while a lookup is out, and the ladder behind this is the
    // slowest call in the app.
    const seq = ++choicesRequest.current;
    invoke<LyricsChoice[]>("get_lyrics_choices", { playlist, song, onScreen: lyrics?.recordId ?? null })
      .then((c) => {
        if (seq === choicesRequest.current) setChoices(c);
      })
      .catch(() => {
        if (seq === choicesRequest.current) setChoices([]);
      });
  }, [playingRef, lyrics?.recordId]);

  const pickRecord = useCallback(
    (recordId: number) => {
      const ref = playingRef();
      if (!ref) return;
      const [playlist, song] = ref;
      setPickerOpen(false);
      invoke<LyricsView | null>("choose_lyrics", { playlist, song, recordId })
        .then((l) => {
          setLyrics(l);
          setLyricsError(l === null);
        })
        .catch((e) => setError(String(e)));
    },
    [playingRef],
  );

  const refreshQueue = useCallback(() => {
    invoke<QueueEntry[]>("get_queue").then(setQueue).catch(() => {});
  }, []);

  // The queue changes on transport events, not on the elapsed tick, so it is
  // re-read when the playing track or the queue's own length changes rather
  // than four times a second.
  useEffect(() => {
    refreshQueue();
    // `queue_revision` rather than the length: shuffling reorders the queue
    // without changing how long it is, and the panel has to follow that.
  }, [playback?.track, playback?.queue_revision, playback?.queue_position, refreshQueue]);

  const queueAction = useCallback(
    (cmd: string, args: Record<string, unknown>) => {
      invoke(cmd, args)
        .then(refreshQueue)
        .catch((e) => setError(String(e)));
    },
    [refreshQueue],
  );

  /* Stable identities, and not for tidiness: these three go to
     `memo(LibraryView)`, and a memo is only as good as its least stable prop.
     Written inline at the call site they were rebuilt on every render of this
     component -- which happens four times a second, because that is how often
     the playback clock arrives -- so the memo compared unequal every time and
     re-rendered the playlist column, the whole track list and the queue to
     move a progress bar none of them display. Every other prop down there is
     a value, a `useState` setter, or already wrapped; these were the only
     three holding the door open. */
  const onJumpQueue = useCallback((qPos: number) => queueAction("jump_to", { qPos }), [queueAction]);
  const onRemoveQueue = useCallback(
    (qPos: number) => queueAction("remove_from_queue", { qPos }),
    [queueAction],
  );
  const onClearQueue = useCallback(() => queueAction("clear_queue", {}), [queueAction]);

  const retryPlaylist = useCallback((index: number) => {
    invoke("refetch_playlist", { index })
      .then(refreshPlaylists)
      .catch((e) => setError(String(e)));
  }, []);

  const prefetchSong = useCallback(
    (shown: number) => {
      if (selected === null) return;
      invoke("prefetch", { playlist: selected, song: toPlaylistIndex(shown) }).catch(() => {});
    },
    [selected, toPlaylistIndex],
  );

  // Declared above the context menus, which all build entries that call them.
  const playSong = useCallback(
    (shown: number) => {
      if (selected === null) return;
      invoke("play", { playlist: selected, song: toPlaylistIndex(shown) }).catch((e) => setError(String(e)));
    },
    [selected, toPlaylistIndex],
  );

  const playResult = useCallback((result: SearchResult) => {
    invoke("play_search_result", { result }).catch((e) => setError(String(e)));
  }, []);

  /** The "Add to Playlist" submenu, shared by every menu that offers it.
   *
   * Liked Music is left out on purpose: its id is literally `LM` and it is the
   * like button rather than a playlist items can be added to, so it gets its
   * own "Like" entry instead. */
  const addToPlaylistItems = useCallback(
    (videoId: string | null): MenuItem[] =>
      playlists
        .filter((p) => p.playlist_id !== "LM")
        .map((p) => ({
          label: p.title,
          onSelect: () => {
            if (!videoId) return;
            invoke("add_to_playlist", { playlistId: p.playlist_id, videoId })
              .then(refreshPlaylists)
              .catch((e) => setError(String(e)));
          },
        })),
    [playlists],
  );

  /** Menu for a track in the library list. */
  const trackMenu = useCallback(
    (e: React.MouseEvent, shown: number) => {
      e.preventDefault();
      if (selected === null) return;
      const song = toPlaylistIndex(shown);
      const track = songs[song];
      const videoId = track?.video_id ?? null;
      setMenu({
        x: e.clientX,
        y: e.clientY,
        items: [
          { label: "Play", onSelect: () => playSong(shown) },
          { label: "Play Next", onSelect: () => queueAction("play_next", { playlist: selected, song }) },
          { label: "Play Last", onSelect: () => queueAction("append_to_queue", { playlist: selected, song }) },
          { label: "Add to Playlist", separatorBefore: true, items: addToPlaylistItems(videoId), onSelect: () => {} },
          {
            label: "Like",
            disabled: !videoId,
            onSelect: () => {
              invoke("like_track", { videoId }).catch((err) => setError(String(err)));
            },
          },
        ],
      });
    },
    [selected, songs, toPlaylistIndex, playSong, queueAction, addToPlaylistItems],
  );

  /** Menu for a search result. Same shape, but a hit has to be filed into the
   * library before it has a `(playlist, song)` pair the queue can hold -- so
   * the queue entries go through `place_search_result` first. */
  const resultMenu = useCallback(
    (e: React.MouseEvent, r: SearchResult) => {
      e.preventDefault();
      setMenu({
        x: e.clientX,
        y: e.clientY,
        items: [
          { label: "Play", onSelect: () => playResult(r) },
          {
            label: "Play Next",
            onSelect: () => queueAction("queue_search_result", { result: r, next: true }),
          },
          {
            label: "Play Last",
            onSelect: () => queueAction("queue_search_result", { result: r, next: false }),
          },
          {
            label: "Add to Playlist",
            separatorBefore: true,
            items: addToPlaylistItems(r.video_id),
            onSelect: () => {},
          },
          {
            label: "Like",
            onSelect: () => {
              invoke("like_track", { videoId: r.video_id }).catch((err) => setError(String(err)));
            },
          },
        ],
      });
    },
    [playResult, queueAction, addToPlaylistItems],
  );

  /** Menu for a queue entry. Addressed by `qPos`, and so a different set --
   * you cannot "add to queue" something already in it. */
  const queueMenu = useCallback(
    (e: React.MouseEvent, entry: QueueEntry) => {
      e.preventDefault();
      setMenu({
        x: e.clientX,
        y: e.clientY,
        items: [
          { label: "Play", onSelect: () => queueAction("jump_to", { qPos: entry.qPos }) },
          {
            label: "Add to Playlist",
            items: addToPlaylistItems(entry.videoId),
            onSelect: () => {},
          },
          {
            label: "Remove from Queue",
            separatorBefore: true,
            danger: true,
            onSelect: () => queueAction("remove_from_queue", { qPos: entry.qPos }),
          },
        ],
      });
    },
    [queueAction, addToPlaylistItems],
  );

  /** Menu for a playlist in the sidebar. */
  const playlistMenu = useCallback(
    (e: React.MouseEvent, index: number) => {
      e.preventDefault();
      const p = playlists[index];
      if (!p) return;
      setMenu({
        x: e.clientX,
        y: e.clientY,
        items: [
          { label: "Open", onSelect: () => setSelected(index) },
          {
            label: p.failed ? "Try loading again" : "Refresh",
            onSelect: () => retryPlaylist(index),
          },
        ],
      });
    },
    [playlists, retryPlaylist],
  );

  const handleSignIn = useCallback(async () => {
    setBusy(true);
    setError("");
    try {
      await invoke("sign_in", { browser });
      setSignedIn(true);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }, [browser]);

  const runSearch = useCallback(
    async (e: React.FormEvent) => {
      e.preventDefault();
      if (!query.trim()) {
        setResults(null);
        return;
      }
      try {
        setResults(await invoke<SearchResult[]>("search", { query }));
      } catch (e) {
        setError(String(e));
      }
    },
    [query],
  );

  const onOpenNowPlaying = useCallback(() => {
    if (currentTrackRef.current) setView("now-playing");
  }, []);

  /* Both `useCallback` for the reason the queue handlers above are: `HomeView`
     is memoized, and an inline arrow would defeat that on every 250ms tick. */
  const onPlayHistory = useCallback((index: number) => {
    invoke("play_history_track", { index }).catch((e) => setError(String(e)));
  }, []);


  return (
    <div className="fixed inset-0 overflow-hidden bg-bg text-ink">
      <LibraryView
        visible={view === "library"}
        signedIn={signedIn}
        browsers={browsers}
        browser={browser}
        setBrowser={setBrowser}
        busy={busy}
        error={error}
        playlists={playlists}
        selected={selected}
        setSelected={setSelected}
        songs={filteredSongs}
        history={history}
        onPlayHistory={onPlayHistory}
        filter={filter}
        setFilter={setFilter}
        query={query}
        setQuery={setQuery}
        results={results}
        setResults={setResults}
        currentTrack={currentTrack}
        paused={playback?.paused ?? true}
        onSignIn={handleSignIn}
        onSearch={runSearch}
        onPlaySong={playSong}
        onHoverSong={prefetchSong}
        onPlayResult={playResult}
        onRetryPlaylist={retryPlaylist}
        sortKey={sortKey}
        sortDir={sortDir}
        onSort={onSort}
        queue={queue}
        showQueue={showQueue}
        onJumpQueue={onJumpQueue}
        onRemoveQueue={onRemoveQueue}
        onClearQueue={onClearQueue}
        onTrackMenu={trackMenu}
        onResultMenu={resultMenu}
        onQueueMenu={queueMenu}
        onPlaylistMenu={playlistMenu}
      />

      {playback && view === "library" && (
        <PlayerBar
          playback={playback}
          currentTrack={currentTrack}
          onOpenNowPlaying={onOpenNowPlaying}
          navOpen={!results}
          queueOpen={showQueue}
          onToggleQueue={() => setShowQueue((v) => !v)}
        />
      )}

      {menu && <ContextMenu state={menu} onClose={() => setMenu(null)} />}

      <AnimatePresence>
        {view === "now-playing" && playback && (
          <NowPlayingView
            playback={playback}
            currentTrack={currentTrack}
            lyrics={lyrics}
            lyricsError={lyricsError}
            lyricsLoading={lyricsLoading}
            config={config}
            translation={translation}
            translateMode={translateMode}
            translateBusy={translateBusy}
            choices={choices}
            pickerOpen={pickerOpen}
            onClose={() => setView("library")}
            onSetTranslateMode={setTranslateMode}
            onRetryLyrics={loadLyrics}
            onOpenPicker={openPicker}
            onClosePicker={() => setPickerOpen(false)}
            onPickRecord={pickRecord}
          />
        )}
      </AnimatePresence>
    </div>
  );
}

interface LibraryViewProps {
  visible: boolean;
  signedIn: boolean | null;
  browsers: string[];
  browser: string;
  setBrowser: (b: string) => void;
  busy: boolean;
  error: string;
  playlists: PlaylistView[];
  selected: number | null;
  setSelected: (i: number | null) => void;
  songs: Track[];
  /** `null` while `history.json` is still being read -- distinct from a read
      that came back empty, which is what the home page's empty state means. */
  history: HistoryView | null;
  onPlayHistory: (index: number) => void;
  filter: string;
  setFilter: (f: string) => void;
  query: string;
  setQuery: (q: string) => void;
  results: SearchResult[] | null;
  setResults: (r: SearchResult[] | null) => void;
  currentTrack: Track | null;
  paused: boolean;
  onSignIn: () => void;
  onSearch: (e: React.FormEvent) => void;
  onPlaySong: (i: number) => void;
  onHoverSong: (i: number) => void;
  onPlayResult: (r: SearchResult) => void;
  onRetryPlaylist: (i: number) => void;
  sortKey: SortKey;
  sortDir: SortDir;
  onSort: (k: SortKey) => void;
  queue: QueueEntry[];
  showQueue: boolean;
  onJumpQueue: (qPos: number) => void;
  onRemoveQueue: (qPos: number) => void;
  onClearQueue: () => void;
  onTrackMenu: (e: React.MouseEvent, i: number) => void;
  onResultMenu: (e: React.MouseEvent, r: SearchResult) => void;
  onQueueMenu: (e: React.MouseEvent, entry: QueueEntry) => void;
  onPlaylistMenu: (e: React.MouseEvent, i: number) => void;
}

/** Memoized, and every prop it takes changes only on a real user action --
 * never on the 250ms playback tick, which is why the player bar below is a
 * sibling rather than a child. Left as a child, the whole library (header,
 * sidebar, track list container) reconciled four times a second for the sake
 * of a moving scrubber, which is most of where the input lag came from. */
const LibraryView = memo(function LibraryView(props: LibraryViewProps) {
  const {
    visible,
    signedIn,
    browsers,
    browser,
    setBrowser,
    busy,
    error,
    playlists,
    selected,
    setSelected,
    songs,
    history,
    onPlayHistory,
    filter,
    setFilter,
    query,
    setQuery,
    results,
    setResults,
    currentTrack,
    paused,
    onSignIn,
    onSearch,
    onPlaySong,
    onHoverSong,
    onPlayResult,
    onRetryPlaylist,
    sortKey,
    sortDir,
    onSort,
    queue,
    showQueue,
    onJumpQueue,
    onRemoveQueue,
    onClearQueue,
    onTrackMenu,
    onResultMenu,
    onQueueMenu,
    onPlaylistMenu,
  } = props;

  return (
    <div className={`flex h-full flex-col ${visible ? "" : "invisible"}`}>
      {/* The window's own title bar is hidden (`titleBarStyle: "Overlay"`), so
          this header *is* the title bar: it has to be draggable, and only the
          bare element carries `data-tauri-drag-region` -- children stay
          clickable. It is sized by its content (`pt-3`, no fixed height)
          rather than given one: at a fixed `h-14` the field centred inside it
          left ~13px of dead header underneath, which stacked with the
          sidebar's own 12px margin into a gap twice the one above. */}
      <header
        data-tauri-drag-region
        className={`flex flex-shrink-0 items-center justify-center px-3 ${HEADER_PAD_TOP}`}
      >
        {signedIn === true && (
          <form onSubmit={onSearch} className="flex min-w-0 max-w-sm flex-1 items-center gap-2">
            <div className="flex min-w-0 flex-1 items-center gap-2 rounded-full px-3.5 py-1.5 glass">
              <Search size={15} className="flex-shrink-0 text-ink-faint" />
              <input
                type="search"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder="Search YouTube Music"
                className="w-full bg-transparent text-[13px] outline-none placeholder:text-ink-faint"
              />
            </div>
            {results && (
              <button
                type="button"
                onClick={() => setResults(null)}
                className="rounded-full px-3 py-1.5 text-[13px] text-ink-dim select-none transition-colors hover:bg-surface hover:text-ink"
              >
                Clear
              </button>
            )}
          </form>
        )}
      </header>

      {signedIn === null && <p className="p-12 text-center text-ink-dim select-none">Checking session…</p>}

      {signedIn === false && (
        <div className="flex flex-1 items-center justify-center gap-3">
          <select
            value={browser}
            onChange={(e) => setBrowser(e.target.value)}
            disabled={busy}
            className="rounded-full bg-surface-2 px-4 py-2 text-[13px] outline-none"
          >
            {browsers.map((b) => (
              <option key={b} value={b}>
                {b}
              </option>
            ))}
          </select>
          <button
            onClick={onSignIn}
            disabled={busy || !browser}
            className="rounded-full bg-accent px-5 py-2 text-[13px] font-medium text-white select-none transition hover:bg-accent-2 disabled:opacity-40"
          >
            {busy ? "Reading cookies…" : "Sign in"}
          </button>
        </div>
      )}

      {error && (
        <p className="mx-6 mb-3 rounded-xl border-[0.5px] border-accent/25 bg-accent/10 px-4 py-2 font-mono text-xs whitespace-pre-wrap text-accent-2 select-text">
          {error}
        </p>
      )}

      {signedIn === true && (
        <div className="flex min-h-0 flex-1">
          {/* No bottom reservation on this row -- the two glass panels run the
              full height of the window, because the island can never reach
              them. It is centred over the content *between* them (see
              `.player-rail`), so the space it takes is exactly the space they
              do not. Only the middle column passes underneath, and only that
              column pays for the clearance. Reserving it here instead cut all
              three short and left a strip of empty canvas under the sidebar. */}
          {!results && (
            <PlaylistNav
              playlists={playlists}
              selected={selected}
              onSelect={setSelected}
              onRetry={onRetryPlaylist}
              onContextMenu={onPlaylistMenu}
            />
          )}

          {/* The island floats over the foot of this column, so the padding
              is inside the scroller: the last track scrolls clear of the pill
              rather than the column stopping above it. Content passing under
              glass is the whole reason for the material -- a floating pane
              with nothing beneath it is a decal. */}
          <div className="no-scrollbar min-w-0 flex-1 overflow-y-auto px-5 pt-2 pb-[var(--player-bar-h)]">
            {/* No playlist selected and nothing searched is Home, which is
                where the app starts. It used to be an empty "Tracks" heading
                over nothing. */}
            {!results && selected === null ? (
              <HomeView
                history={history}
                currentTrackId={currentTrack?.video_id}
                paused={paused}
                onPlayTrack={onPlayHistory}
              />
            ) : (
            <>
            <div className="flex items-center justify-between gap-4 px-3 pb-2 select-none">
              <p className="text-[11px] font-semibold tracking-wider text-ink-faint uppercase">
                {results ? "Search results" : "Tracks"}
              </p>
              {/* The TUI's `/`: filters the playlist already loaded, which is
                  a different thing from the header's search of YouTube. Only
                  offered where there is a playlist to filter. */}
              {!results && selected !== null && (
                <div className="flex min-w-0 max-w-56 flex-1 items-center gap-1.5 rounded-full bg-surface px-2.5 py-1">
                  <Filter size={12} className="flex-shrink-0 text-ink-ghost" />
                  <input
                    value={filter}
                    onChange={(e) => setFilter(e.target.value)}
                    onKeyDown={(e) => e.key === "Escape" && setFilter("")}
                    placeholder="Filter"
                    className="w-full bg-transparent text-xs outline-none placeholder:text-ink-ghost"
                  />
                  {filter && (
                    <button
                      onClick={() => setFilter("")}
                      className="flex-shrink-0 text-ink-ghost transition-colors hover:text-ink"
                      aria-label="Clear filter"
                    >
                      <X size={12} />
                    </button>
                  )}
                </div>
              )}
            </div>
            {!results && songs.length > 0 && (
              <TrackHeader sortKey={sortKey} sortDir={sortDir} onSort={onSort} />
            )}
            <ul className="select-none">
              {results ? (
                <SearchResultsList
                  results={results}
                  onPlayResult={onPlayResult}
                  onContextMenu={onResultMenu}
                />
              ) : (
                <TrackList
                  songs={songs}
                  currentTrackId={currentTrack?.video_id}
                  paused={paused}
                  onPlaySong={onPlaySong}
                  onHoverSong={onHoverSong}
                  onContextMenu={onTrackMenu}
                />
              )}
            </ul>
            {!results && selected !== null && songs.length === 0 && filter && (
              <p className="px-3 py-6 text-center text-[13px] text-ink-dim select-none">
                Nothing matches “{filter}”.
              </p>
            )}
            </>
            )}
          </div>

          {showQueue && (
            <QueuePanel
              entries={queue}
              onJump={onJumpQueue}
              onRemove={onRemoveQueue}
              onClear={onClearQueue}
              onContextMenu={onQueueMenu}
            />
          )}
        </div>
      )}

    </div>
  );
});

interface PlayerBarProps {
  playback: PlaybackStateView;
  currentTrack: Track | null;
  onOpenNowPlaying: () => void;
  /** Whether the playlist sidebar is showing. The island centres over the
   * content *between* the two side panels rather than over the window, so it
   * is the one piece of chrome that has to know either is there. */
  navOpen: boolean;
  queueOpen: boolean;
  onToggleQueue: () => void;
}

/** The one piece that genuinely has to redraw on every playback tick, kept as
 * a sibling of `LibraryView` so it is also the *only* piece that does. */
function PlayerBar({
  playback,
  currentTrack,
  onOpenNowPlaying,
  navOpen,
  queueOpen,
  onToggleQueue,
}: PlayerBarProps) {
  const remaining = Math.max(0, playback.total - playback.elapsed);
  const album = currentTrack?.album?.name;

  return (
    /* Apple Music's island: transport left, the track in the middle, the icons
       right, and the progress as a line under the middle rather than as a row
       of its own. Losing that row is most of where the height went -- 96px to
       64px to 56px -- and the rest is a smaller cover.

       The rail is what moves when a side panel opens; see `.player-rail` in
       `App.css` for why the pill itself is not animated. */
    <div className={`player-rail ${navOpen ? "has-nav" : ""} ${queueOpen ? "has-queue" : ""}`}>
      {/* `auto` either side of a `minmax(0, 1fr)` middle, which is what
          Apple's literal `152px 394px 90px` buys and this gets without
          hardcoding the width of a row of icons. What matters is that the
          side columns are sized by their contents rather than by fractions of
          the window: on `1fr 2fr 1fr` every column moved when the window did,
          so a title reflowed and re-truncated on a resize, and on a drag it
          did so continuously. Here the only thing a resize can reach is the
          middle column, and only once the pill is below its own width.

          Every column is plainly centred, and stays that way because the seek
          line is absolute rather than a row of its own; see
          `.slider-seekbar`. */}
      <footer className="player-island grid grid-cols-[auto_minmax(0,1fr)_auto] items-stretch gap-4 px-4 select-none">
        <div className="flex items-center gap-4">
          {/* Shuffle's place in Apple's bar, doing the work of both its shuffle
              and its repeat: `PlayMode` fuses them into one tri-state, and
              `modeIcon` draws whichever of the three is on. */}
          <button
            onClick={() => invoke("cycle_mode")}
            aria-label="Play mode"
            title={`Play mode: ${playback.mode}`}
            className={`transition-colors ${
              playback.mode.toLowerCase().includes("cycle") ? "text-ink-dim hover:text-ink" : "text-accent"
            }`}
          >
            {modeIcon(playback.mode)}
          </button>
          <button onClick={() => invoke("prev")} className="text-ink-dim transition-colors hover:text-ink">
            <SkipBack size={18} fill="currentColor" />
          </button>
          <button
            onClick={() => invoke("play_pause")}
            className="flex h-8 w-8 items-center justify-center rounded-full bg-ink text-bg transition hover:scale-105 active:scale-95"
          >
            {playback.paused ? <Play size={15} fill="currentColor" /> : <Pause size={15} fill="currentColor" />}
          </button>
          <button onClick={() => invoke("next")} className="text-ink-dim transition-colors hover:text-ink">
            <SkipForward size={18} fill="currentColor" />
          </button>
        </div>

        {/* The LCD. Two things in one space, swapped by hovering the seek line
            below them: the track normally, the clock while scrubbing. The
            track is left-aligned in the column and the two times sit at its
            far ends, which is Apple's arrangement and is what makes the
            column read as one instrument rather than as three centred things
            -- the words start where the line starts, and the line's ends are
            labelled. The swap is CSS; see `.player-island-lcd` in `App.css`
            for why it is not a `peer` selector and not React state. */}
        <div className="player-island-lcd flex min-w-0 items-center">
          <button
            onClick={onOpenNowPlaying}
            className="player-bar-info flex min-w-0 items-center gap-3 text-left"
          >
            <Thumbnail srcs={[currentTrack?.thumbnail]} className="h-10 w-10 flex-shrink-0 rounded-md object-cover" />
            <span className="min-w-0">
              <p className="truncate text-[13px] font-semibold text-ink">
                {currentTrack?.title ?? "Nothing playing"}
              </p>
              {currentTrack && (
                <p className="truncate text-xs text-ink-dim">
                  {artistNames(currentTrack)}
                  {album && ` — ${album}`}
                </p>
              )}
            </span>
          </button>

          {/* Elapsed and *remaining*, not elapsed and total: the line already
              says how much is left as a proportion, and a countdown is the one
              of the two that answers "can I start something else yet". */}
          <div className="player-bar-clock pointer-events-none absolute inset-x-0 flex items-center justify-between font-mono text-[11px] text-ink tabular-nums">
            <span>{formatTime(playback.elapsed)}</span>
            <span>−{formatTime(remaining)}</span>
          </div>

          <Slider
            className="slider-seekbar"
            value={playback.elapsed}
            max={playback.total}
            onChange={(v) => invoke("seek_to", { secs: v })}
          />
        </div>

        <div className="flex min-w-0 flex-shrink-0 items-center justify-end gap-3">
          <button
            onClick={onToggleQueue}
            aria-label="Up Next"
            title="Up Next"
            className={`transition-colors ${queueOpen ? "text-accent" : "text-ink-dim hover:text-ink"}`}
          >
            <ListMusic size={16} />
          </button>
          <button onClick={() => invoke("toggle_mute")} className="text-ink-dim transition-colors hover:text-ink">
            {playback.muted ? <VolumeX size={16} /> : <Volume2 size={16} />}
          </button>
          <Slider
            className="w-20"
            value={playback.volume}
            max={100}
            onChange={(v) => invoke("set_volume", { volume: Math.round(v) })}
          />
        </div>
      </footer>
    </div>
  );
}

/* How the lyric sheet moves, measured off Apple Music's web player by
 * sampling `scrollTop` every frame through a line change: 234px in 334ms,
 * on a curve that fits `cubic-bezier(0.4, 0, 0.6, 1)` to under a pixel.
 *
 * `scrollIntoView({ behavior: "smooth" })` can express none of that. WebKit
 * picks its own duration and grows it with distance, so a chorus repeat and a
 * seek across the song move at visibly different speeds; and successive calls
 * queue rather than retarget, so scrubbing sets off a run of scrolls that
 * arrive seconds after the audio has moved on. Driving `scrollTop` from a
 * rAF loop settles both -- one animation at a time, always the same length. */
const LYRIC_SCROLL_MS = 334;

/** How long the sheet stays where the user put it after they stop scrolling.
 * Auto-scroll is right nearly all the time and wrong exactly when someone
 * wants to re-read a line, so it yields rather than fights -- but it has to
 * come back on its own, since the alternative is a panel that silently stops
 * following the song and a control to un-stick it. */
const LYRIC_IDLE_MS = 3000;

/** `cubic-bezier(0.4, 0, 0.6, 1)` evaluated directly.
 *
 * The control points are symmetric about the diagonal, which collapses the
 * curve to `x(u) = 1.2u - 0.6u² + 0.4u³` and `y(u) = 3u² - 2u³`; only x has
 * to be inverted, and four Newton steps from `u = t` are enough for a
 * sub-pixel result over any distance a lyric sheet scrolls. */
function lyricEase(t: number): number {
  let u = t;
  for (let i = 0; i < 4; i++) {
    const x = 1.2 * u - 0.6 * u * u + 0.4 * u * u * u;
    const dx = 1.2 - 1.2 * u + 1.2 * u * u;
    u -= (x - t) / dx;
  }
  return 3 * u * u - 2 * u * u * u;
}

interface LyricsPanelProps {
  lines: LyricLineView[];
  activeLine: number;
  /** Whether the record carries timings. Without them no line is "current",
   * so the three-state styling below has nothing to be relative to. */
  synced: boolean;
  /** One entry per line, empty where nothing was translated. */
  translation: string[] | null;
  /** The cover's box, which is what the sung line is centred against rather
   * than the panel's own middle. The cover sits above the row's centre line
   * -- it shares a column with the title and artist, and the column as a
   * whole is what's centred -- so the two are tens of pixels apart, and the
   * lyric column reads as hanging low against it. */
  coverRef: RefObject<HTMLElement | null>;
  /** Jump playback to a line's timestamp. Given the lyric's own `at`, not a
   * playback position: the configured offset sits between the two, and
   * subtracting it belongs beside the place that adds it. */
  onSeek: (at: number) => void;
}

/** Memoized against the playback ticker: `activeLine` only changes when
 * `elapsed` crosses into a new line's timestamp -- every few seconds, not on
 * every ~250ms tick -- so a long lyric sheet doesn't remap and re-render (and
 * the active-line auto-scroll doesn't compete for the main thread against a
 * render storm) four times a second while a song plays. */
const LyricsPanel = memo(function LyricsPanel({
  lines,
  activeLine,
  synced,
  translation,
  coverRef,
  onSeek,
}: LyricsPanelProps) {
  const activeTextRef = useRef<HTMLParagraphElement>(null);
  const boxRef = useRef<HTMLDivElement>(null);
  const frame = useRef(0);
  const idle = useRef<number | undefined>(undefined);

  /* Whether the user has taken the sheet over. While they have, auto-scroll
     stands down *and* the sung lines come back -- the whole reason to scroll
     up here is to read something that has already gone past, and fading it to
     nothing is exactly what makes that impossible. */
  const [browsing, setBrowsing] = useState(false);

  useEffect(() => {
    const box = boxRef.current;
    if (!box) return;
    /* Bound to the input events rather than to `scroll`, which cannot tell
       the user's wheel from the rAF loop below writing `scrollTop` -- and a
       loop that hands over to the user on its own output stops on its first
       frame and never moves again. */
    const grab = () => {
      cancelAnimationFrame(frame.current);
      setBrowsing(true);
      window.clearTimeout(idle.current);
      idle.current = window.setTimeout(() => setBrowsing(false), LYRIC_IDLE_MS);
    };
    box.addEventListener("wheel", grab, { passive: true });
    box.addEventListener("touchmove", grab, { passive: true });
    return () => {
      box.removeEventListener("wheel", grab);
      box.removeEventListener("touchmove", grab);
      window.clearTimeout(idle.current);
    };
  }, []);

  useEffect(() => {
    const box = boxRef.current;
    if (!box || browsing) return;

    /* No line is current: an unsynced record, or the run-in before the first
       timestamp. Go to the top, and go there instantly -- there is nothing on
       screen for the movement to be about, and animating out of wherever the
       previous track left the panel is how a new song opens mid-scroll. */
    const el = activeTextRef.current;
    if (!synced || activeLine < 0 || !el) {
      cancelAnimationFrame(frame.current);
      box.scrollTop = 0;
      return;
    }

    /* The target is the *lyric's* centre against the cover's centre, not the
       wrapper's against the panel's. Both halves of that matter: the wrapper
       grows a second line when a translation is showing, so centring it would
       push the words themselves half a translation high; and the cover is not
       where the panel's middle is (see `coverRef`).

       Measured rather than read off `offsetTop`, which would need the panel to
       be the offset parent and would silently mean something else the moment
       anything between them grew a `position`. Every term is read in the same
       frame, so a measurement taken mid-animation is still consistent. */
    const line = el.getBoundingClientRect();
    const cover = coverRef.current?.getBoundingClientRect();
    const box_ = box.getBoundingClientRect();
    const centre = cover ? cover.top + cover.height / 2 : box_.top + box_.height / 2;

    const from = box.scrollTop;
    const limit = box.scrollHeight - box.clientHeight;
    const to = Math.max(0, Math.min(from + (line.top + line.height / 2 - centre), limit));
    if (Math.abs(to - from) < 1) return;

    const start = performance.now();
    cancelAnimationFrame(frame.current);
    const step = (now: number) => {
      const t = Math.min(1, (now - start) / LYRIC_SCROLL_MS);
      box.scrollTop = from + (to - from) * lyricEase(t);
      if (t < 1) frame.current = requestAnimationFrame(step);
    };
    frame.current = requestAnimationFrame(step);
    return () => cancelAnimationFrame(frame.current);
  }, [activeLine, synced, lines, browsing, coverRef]);

  return (
    /* The mask is Apple's, and it is deliberately asymmetric: a short fade at
       the top, then nothing until halfway down, then a long fade over the
       whole bottom half. Symmetric was wrong for the same reason centring the
       active line was -- the top of the panel holds lines on their way out
       and needs no ceremony, while the bottom holds everything still to come
       and wants to recede rather than stop. */
    <div
      ref={boxRef}
      /* `px-4` is not spacing, it is headroom for the active line's `scale
         (1.05)`. Setting `overflow-y` computes `overflow-x` to `auto` rather
         than leaving it `visible`, so a line that fills the column had its
         outer 2.5% clipped off each end as it grew -- the left edge of a
         centred line visibly shaved. 16px covers the ~11px a full-width line
         at this column's size overhangs by. */
      className={`no-scrollbar h-full w-full max-w-lg overflow-y-auto px-4 pt-[45vh] pb-[70vh] [mask-image:linear-gradient(to_bottom,transparent,black_80px,black_50%,transparent)] ${
        browsing ? "lyrics-browsing" : ""
      }`}
    >
      {lines.map((l, i) => {
        // A translation identical to the original, or blank, earns no row --
        // it would just be the same words twice at half the contrast.
        const under = translation?.[i];
        const showUnder = Boolean(under) && under !== l.text;
        const state =
          !synced || activeLine < 0
            ? "is-plain"
            : i === activeLine
              ? "is-active"
              : i < activeLine && !browsing
                ? "is-past"
                : ""; // still to come, or sung and being read back
        return (
          /* Original and translation share one wrapper, and the wrapper is
             what carries the state -- so a translated pair dims, blurs and
             scales as the one line it is. The scroll targets the lyric inside
             it rather than the wrapper; see the effect above.

             A `button`, as Apple's lines are: clicking one seeks to it, which
             is the only way to move around a song *by its words* rather than
             by its seconds. `disabled` on an unsynced sheet, where the lines
             carry no timings and there is nowhere to seek to.

             `preventDefault` on mousedown suppresses the focus a click would
             otherwise take, and with it the browser's own scroll-into-view --
             which would fight the rAF loop above for `scrollTop` and land the
             line somewhere other than on the cover's centre line. Keyboard
             focus is untouched, and there the scroll is wanted. */
          <button
            key={i}
            type="button"
            disabled={!synced}
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => onSeek(l.at)}
            className={`lyric-line mb-7 block w-full cursor-pointer text-center disabled:cursor-default ${state}`}
          >
            <p ref={i === activeLine ? activeTextRef : undefined} className="text-[30px] leading-[1.28] font-semibold">
              {l.text || "• • •"}
            </p>
            {showUnder && <p className="mt-1.5 text-[19px] leading-[1.28] font-medium opacity-70">{under}</p>}
          </button>
        );
      })}
    </div>
  );
});

interface LyricsPickerProps {
  choices: LyricsChoice[] | null;
  onScreen: number | undefined;
  onPick: (id: number) => void;
  onClose: () => void;
}

/** The `c` overlay: every lrclib record for this track, so a bad automatic
 * match can be replaced by hand. The record on screen is marked, since it is
 * often the exact `/get` hit the search ladder never sees. */
function LyricsPicker({ choices, onScreen, onPick, onClose }: LyricsPickerProps) {
  return (
    <div className="absolute inset-0 z-40 flex items-center justify-center bg-black/50 p-8" onClick={onClose}>
      <div
        className="flex max-h-[70vh] w-full max-w-lg flex-col overflow-hidden rounded-2xl glass-heavy"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex flex-shrink-0 items-center justify-between px-4 py-3">
          <p className="text-[11px] font-semibold tracking-wider text-ink-faint uppercase">Choose lyrics</p>
          <button onClick={onClose} className="text-ink-faint transition-colors hover:text-ink" aria-label="Close">
            <X size={16} />
          </button>
        </div>
        <div className="thin-scrollbar min-h-0 flex-1 overflow-y-auto px-2 pb-2">
          {choices === null && <p className="px-3 py-6 text-center text-[13px] text-ink-dim">Searching lrclib…</p>}
          {choices?.length === 0 && (
            <p className="px-3 py-6 text-center text-[13px] text-ink-dim">No records for this track.</p>
          )}
          {choices?.map((c) => (
            <button
              key={c.id}
              onClick={() => onPick(c.id)}
              className={`flex w-full items-center gap-3 rounded-xl px-3 py-2 text-left transition-colors hover:bg-surface-2 ${
                c.id === onScreen ? "bg-surface" : ""
              }`}
            >
              <span className="min-w-0 flex-1">
                <p className="truncate text-[13px] text-ink">{c.trackName}</p>
                <p className="truncate text-xs text-ink-dim">
                  {c.artistName}
                  {c.albumName && ` — ${c.albumName}`}
                </p>
              </span>
              <span className="flex flex-shrink-0 items-center gap-2 text-[11px]">
                {c.id === onScreen && <span className="font-semibold text-accent">IN USE</span>}
                {c.timingMismatch && <span className="text-ink-faint">off-length</span>}
                <span className={c.synced ? "text-ink-dim" : "text-ink-faint"}>{c.synced ? "synced" : "plain"}</span>
                <span className="font-mono text-ink-ghost">{c.duration ? formatTime(c.duration) : "—"}</span>
              </span>
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}

interface NowPlayingViewProps {
  playback: PlaybackStateView;
  currentTrack: Track | null;
  lyrics: LyricsView | null;
  lyricsError: boolean;
  lyricsLoading: boolean;
  config: ConfigView | null;
  translation: string[] | null;
  translateMode: TranslateMode;
  translateBusy: boolean;
  choices: LyricsChoice[] | null;
  pickerOpen: boolean;
  onClose: () => void;
  onSetTranslateMode: (m: TranslateMode) => void;
  onRetryLyrics: () => void;
  onOpenPicker: () => void;
  onClosePicker: () => void;
  onPickRecord: (id: number) => void;
}

function NowPlayingView({
  playback,
  currentTrack,
  lyrics,
  lyricsError,
  lyricsLoading,
  config,
  translation,
  translateMode,
  translateBusy,
  choices,
  pickerOpen,
  onClose,
  onSetTranslateMode,
  onRetryLyrics,
  onOpenPicker,
  onClosePicker,
  onPickRecord,
}: NowPlayingViewProps) {
  // The configured offset shifts the clock handed to the active-line search,
  // never the cached records -- same rule as the TUI's `Config::lyric_time`.
  const clock = playback.elapsed + (config?.lyricsOffset ?? 0);
  const activeLine = lyrics?.synced ? activeLyricIndex(lyrics.lines, clock) : -1;
  /* The inverse of that line, and the reason the offset is not applied in
     `LyricsPanel`: a lyric's `at` is on the shifted clock, so seeking to it
     means undoing the shift. Memoised because `LyricsPanel` is, and an inline
     arrow would defeat that on every 250ms tick. */
  const onSeekLyric = useCallback(
    (at: number) => {
      invoke("seek_to", { secs: Math.max(0, at - (config?.lyricsOffset ?? 0)) });
    },
    [config?.lyricsOffset],
  );
  const [showLyrics, setShowLyrics] = useState(true);
  const coverRef = useRef<HTMLDivElement>(null);

  /* The cover's own proportions, once the picture has decoded. Square until
     then, which is what album art is and what the reserved block should look
     like while it loads -- the same answer `App::cover_aspect` gives the TUI.
     The video id is stored beside the ratio and compared during render rather
     than cleared by an effect: an effect runs after the frame that already
     drew the new track, so a cached 16:9 thumbnail could report its shape
     before the reset and have it thrown away a moment later. */
  const [cover, setCover] = useState<{ id: string | null; ratio: number }>({ id: null, ratio: 1 });
  const trackId = currentTrack?.video_id ?? null;
  const coverAspect = cover.id === trackId ? cover.ratio : 1;
  const onCoverAspect = useCallback((ratio: number) => setCover({ id: trackId, ratio }), [trackId]);
  const hasLyrics = Boolean(lyrics && lyrics.lines.length > 0);
  const canTranslate = Boolean(config?.translateTo) && hasLyrics;

  /* Whether the right-hand column exists at all -- lyrics, the "try again"
     block, or the spinner between them. One predicate for both the column and
     the cover's width, because they have to agree: deciding the column on
     `hasLyrics` alone meant that during a re-fetch (a retry, or any track
     change) it briefly held none of the three, and the row's `justify-center`
     slid the cover into the middle and back out again while the cover itself
     animated between two max-widths. */
  const lyricsColumn = showLyrics && (hasLyrics || lyricsLoading || lyricsError);

  return (
    <motion.div
      initial={{ opacity: 0, y: 24 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: 24 }}
      transition={{ duration: 0.35, ease: [0.22, 1, 0.36, 1] }}
      className="absolute inset-0 overflow-hidden bg-bg text-ink"
    >
      <AnimatePresence mode="wait">
        {currentTrack?.thumbnail ? (
          // Full opacity, and lifted rather than dimmed: the artwork *is* the
          // background here, and holding it at 0.6 over a near-black base was
          // averaging every cover towards black regardless of what it actually
          // looked like. Saturation and brightness push it the other way, so a
          // light cover reads as a light room.
          <motion.div
            key={currentTrack.video_id ?? "bg"}
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.5 }}
            className="absolute inset-0 scale-125 bg-cover bg-center blur-3xl saturate-[1.7] brightness-110"
            style={{ backgroundImage: `url(${bestCoverUrl(currentTrack.thumbnail, 200)})` }}
          />
        ) : (
          <motion.div
            key="bg-fallback"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.5 }}
            className="absolute inset-0 bg-linear-to-br from-surface-2 via-bg to-surface"
          />
        )}
      </AnimatePresence>
      {/* A scrim, not a curtain. It only has to buy the white transport
          controls enough contrast at the bottom, so it stays near-clear at the
          top where the artwork should show through, and never reaches full
          opacity -- `to-bg` did, which is what flattened the lower half of the
          page to black whatever the cover was. */}
      <div className="absolute inset-0 bg-linear-to-b from-black/10 via-black/25 to-black/55" />

      <div className="relative z-10 flex h-full flex-col">
        {/* Also the title bar while this view is up -- same drag/traffic-light
            reasoning as `LibraryView`'s header. This one's control is
            left-aligned rather than centred, so it has to clear the lights. */}
        <div
          data-tauri-drag-region
          className={`flex flex-shrink-0 items-center pr-6 ${TRAFFIC_LIGHT_AXIS} ${IS_MAC ? TRAFFIC_LIGHT_SPAN : "pl-6"}`}
        >
          <button
            onClick={onClose}
            aria-label="Back to library"
            title="Back to library"
            className="flex h-8 w-8 items-center justify-center rounded-full text-white/70 select-none transition-colors hover:bg-white/10 hover:text-white"
          >
            <ChevronDown size={18} />
          </button>
        </div>

        {/* Apple Music's split: two halves of the window, the artwork and its
            transport as one block centred in the left one and the lyric sheet
            centred in the right. Half rather than "as wide as it needs to be"
            is the point -- it fixes where the two columns sit whatever the
            cover's shape or the length of a line, so nothing slides sideways
            between one track and the next. With no lyric column there is
            nothing to split against, and the block takes the whole width. */}
        <div className="flex min-h-0 flex-1 overflow-hidden">
          <div className={`flex items-center justify-center px-8 ${lyricsColumn ? "w-1/2" : "w-full"}`}>
            <motion.div
              key={currentTrack?.video_id ?? "np-none"}
              initial={{ opacity: 0, scale: 0.92, y: 10 }}
              animate={{ opacity: 1, scale: 1, y: 0 }}
              transition={{ duration: 0.4, ease: [0.22, 1, 0.36, 1] }}
              className="flex w-full max-w-[29rem] flex-col items-center text-center select-none"
            >
              {/* The wrapper exists to be measured -- the lyric sheet centres
                  the line being sung on this box, and `Thumbnail` renders an
                  `img` or a placeholder `div` depending on whether the cover
                  loaded, so there is no one element to hang a ref on. It
                  carries the sizing so its box is the cover's box exactly.

                  The shape comes from the *picture*, not from an assumption
                  about it: album art is square and a video's thumbnail is
                  16:9, and at this size `object-cover`ing one into the other
                  shows a strip out of the middle of the artwork rather than
                  the artwork. `aspectRatio` on the box means the crop is
                  always a no-op, and `maxHeight` keeps a square cover from
                  pushing the transport off a short window -- with a ratio
                  set, clamping the height narrows the width to match, so the
                  box stays exactly the cover's shape either way. */}
              <div
                ref={coverRef}
                className="relative w-full"
                style={{ aspectRatio: coverAspect, maxHeight: "48vh" }}
              >
                {/* Radiosity: the cover again, blurred and over-saturated,
                    just larger than the cover itself so the colour it throws
                    clears the artwork's edges. Apple's own is `opacity: 0.4;
                    filter: blur(20px) saturate(2)`, and this is that.

                    Not redundant with the page background, which is the same
                    picture at `blur-3xl` across the whole window -- that is a
                    diffuse wash with no relationship to where the artwork is,
                    and this is a tight halo that hugs it. Together they read
                    as one lit object rather than as a picture pasted onto a
                    coloured field. The 200px source is the background's, so
                    it is already fetched and decoded, and a 20px blur has no
                    use for more. */}
                {currentTrack?.thumbnail && (
                  <div
                    aria-hidden
                    className="pointer-events-none absolute inset-0 scale-[1.04] rounded-2xl bg-cover bg-center opacity-40 blur-[20px] saturate-[2]"
                    style={{ backgroundImage: `url(${bestCoverUrl(currentTrack.thumbnail, 200)})` }}
                  />
                )}
                <Thumbnail
                  /* 1200, not 800. The box caps at 29rem -- 464 CSS px,
                     which is 928 device pixels on a 2x display, so 800 was
                     under what the panel can actually resolve and the browser
                     was enlarging it. The CDN serves any size exactly up to
                     1400 and anything past that as 1400, so asking high costs
                     one larger decode and nothing else. */
                  srcs={currentTrack?.thumbnail ? coverCandidates(currentTrack.thumbnail, 1200) : []}
                  className="relative h-full w-full rounded-2xl object-cover shadow-2xl shadow-black/60"
                  onAspect={onCoverAspect}
                />
              </div>

              <div className="on-artwork mt-6">
                <h2 className="text-2xl font-bold">{currentTrack?.title ?? "Nothing playing"}</h2>
                {currentTrack && <p className="mt-1 text-[15px] text-white/70">{artistNames(currentTrack)}</p>}
              </div>

              {/* Under the artwork rather than across the foot of the window.
                  The transport belongs to the thing it is transporting, and
                  spanning the page put it under the lyric column too, where
                  it was a row of controls for a sheet of words. */}
              <div className="mt-7 w-full">
                <Slider
                  className="w-full"
                  value={playback.elapsed}
                  max={playback.total}
                  onChange={(v) => invoke("seek_to", { secs: v })}
                />
                <div className="on-artwork mt-1.5 flex justify-between font-mono text-[11px] text-white/60 tabular-nums select-none">
                  <span>{formatTime(playback.elapsed)}</span>
                  <span>{formatTime(playback.total)}</span>
                </div>
                <div className="mt-5 flex items-center justify-center gap-7 select-none">
                  <button
                    onClick={() => invoke("cycle_mode")}
                    className={`transition-colors ${
                      playback.mode.toLowerCase().includes("cycle") ? "text-white/60 hover:text-white" : "text-accent"
                    }`}
                  >
                    {modeIcon(playback.mode)}
                  </button>
                  <button onClick={() => invoke("prev")} className="text-white transition-transform hover:scale-110">
                    <SkipBack size={22} fill="currentColor" />
                  </button>
                  <button
                    onClick={() => invoke("play_pause")}
                    className="flex h-14 w-14 items-center justify-center rounded-full bg-white text-black transition hover:scale-105 active:scale-95"
                  >
                    {playback.paused ? <Play size={24} fill="currentColor" /> : <Pause size={24} fill="currentColor" />}
                  </button>
                  <button onClick={() => invoke("next")} className="text-white transition-transform hover:scale-110">
                    <SkipForward size={22} fill="currentColor" />
                  </button>
                  <button
                    onClick={() => invoke("toggle_mute")}
                    className="text-white/60 transition-colors hover:text-white"
                  >
                    {playback.muted ? <VolumeX size={18} /> : <Volume2 size={18} />}
                  </button>
                </div>
              </div>
            </motion.div>
          </div>

          {/* One box, three states. They share the sizing classes so swapping
              between them never changes the row's layout -- see
              `lyricsColumn`. Lyrics win over the spinner while a re-fetch is
              in flight, so a retry that lands on the same words doesn't blank
              the panel on the way. */}
          {lyricsColumn && (
            <div className="flex h-full w-1/2 justify-center px-8">
              {hasLyrics ? (
                <LyricsPanel
                  lines={lyrics!.lines}
                  activeLine={activeLine}
                  synced={lyrics!.synced}
                  translation={translation}
                  coverRef={coverRef}
                  onSeek={onSeekLyric}
                />
              ) : lyricsLoading ? (
                <div className="flex h-full flex-col items-center justify-center">
                  <p className="text-[13px] text-ink-faint">Looking for lyrics…</p>
                </div>
              ) : (
                /* Nothing came back. The TUI answers this with `r`, and so
                   does this -- a failed lookup is usually a transient lrclib
                   timeout. */
                <div className="flex h-full flex-col items-center justify-center gap-3 text-center">
                  <p className="text-[13px] text-ink-dim">Couldn't load lyrics for this track.</p>
                  <button
                    onClick={onRetryLyrics}
                    className="flex items-center gap-1.5 rounded-full px-3 py-1.5 text-[13px] text-ink transition-colors hover:bg-surface-2 glass"
                  >
                    <RotateCw size={13} /> Try again
                  </button>
                </div>
              )}
            </div>
          )}
        </div>

        <div className="absolute right-4 bottom-4 z-20 flex items-center gap-2 select-none">
          {/* `i` and `I`: one control between them, since they are one
              TranslateMode. The AI button appears only where config.toml set
              it up, so the paid path is never a click away by accident. */}
          {canTranslate && showLyrics && (
            <>
              <button
                onClick={() => onSetTranslateMode(translateMode === "free" ? "off" : "free")}
                aria-label="Translate"
                title={`Translate to ${config?.translateTo}`}
                className={`flex h-10 w-10 items-center justify-center rounded-full transition ${
                  translateMode === "free" ? "text-ink glass-heavy" : "text-ink-dim hover:text-ink glass"
                } ${translateBusy && translateMode === "free" ? "animate-pulse" : ""}`}
              >
                <Languages size={18} />
              </button>
              {config?.aiAvailable && (
                <button
                  onClick={() => onSetTranslateMode(translateMode === "ai" ? "off" : "ai")}
                  aria-label="Translate with AI"
                  title={`Translate to ${config.translateTo} with the AI model`}
                  className={`flex h-10 items-center justify-center gap-1 rounded-full px-3 text-[11px] font-semibold transition ${
                    translateMode === "ai" ? "text-ink glass-heavy" : "text-ink-dim hover:text-ink glass"
                  } ${translateBusy && translateMode === "ai" ? "animate-pulse" : ""}`}
                >
                  <Languages size={16} /> AI
                </button>
              )}
            </>
          )}

          {hasLyrics && showLyrics && (
            <button
              onClick={onOpenPicker}
              aria-label="Choose a different lyrics record"
              title="Choose a different lyrics record"
              className="flex h-10 w-10 items-center justify-center rounded-full text-ink-dim transition hover:text-ink glass"
            >
              <ListMusic size={18} />
            </button>
          )}

          {hasLyrics && (
            <button
              onClick={() => setShowLyrics((v) => !v)}
              aria-label={showLyrics ? "Hide lyrics" : "Show lyrics"}
              className={`flex h-10 w-10 items-center justify-center rounded-full transition ${
                showLyrics ? "text-ink glass-heavy" : "text-ink-dim hover:text-ink glass"
              }`}
            >
              <MessageSquareText size={18} />
            </button>
          )}
        </div>

        {pickerOpen && (
          <LyricsPicker
            choices={choices}
            onScreen={lyrics?.recordId}
            onPick={onPickRecord}
            onClose={onClosePicker}
          />
        )}

      </div>
    </motion.div>
  );
}

export default App;
