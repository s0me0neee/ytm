import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { AnimatePresence } from "motion/react";
import * as Tooltip from "@radix-ui/react-tooltip";
import { Toaster, toast } from "sonner";
import {
  ChevronLeft,
  ChevronRight,
  Disc3,
  ExternalLink,
  Heart,
  HeartOff,
  Link,
  ListEnd,
  ListStart,
  MicVocal,
  MinusCircle,
  Play,
  Plus,
  Radio,
  RotateCw,
  Search,
  Shuffle,
} from "lucide-react";
import { call, send } from "./api";
import { AddToPlaylistDialog, type AddRequest } from "./components/AddToPlaylistDialog";
import { KeymapOverlay } from "./components/KeymapOverlay";
import { MenuProvider, SEPARATOR, nativeMenu, type MenuEntry } from "./components/Menu";
import { PlayerBar } from "./components/PlayerBar";
import { PlaylistNav } from "./components/PlaylistNav";
import { QueuePanel } from "./components/QueuePanel";
import { SignIn } from "./components/SignIn";
import { Tip } from "./components/Tip";
import { ActionsContext, StateContext, type AppActions, type AppState, type Menus, type TrackMenuExtras } from "./context";
import { errorText } from "./format";
import type {
  ConfigView,
  HistoryView,
  LyricsChoice,
  LyricsView,
  PlaybackStateView,
  PlaylistView,
  QueueEntry,
  Route,
  Target,
  Track,
  TranslateMode,
} from "./types";
import { LIKED_PLAYLIST_ID, SEARCH_PLAYLIST_ID } from "./types";
import { AlbumView, loadAlbum } from "./views/AlbumView";
import { ArtistView } from "./views/ArtistView";
import { HomeView } from "./views/HomeView";
import { NowPlayingView } from "./views/NowPlayingView";
import { FILTER_INPUT_ID, PlaylistPage } from "./views/PlaylistPage";
import { SearchPage } from "./views/SearchPage";
import "./App.css";

const SEEK_STEP = 5;
const VOLUME_STEP = 5;
const SEARCH_INPUT_ID = "search";
const MAX_BACK = 50;
const TOAST_OFFSET = "calc(var(--player-bar-h) + 0.25rem)";
const TOAST_OPTIONS = { unstyled: true, classNames: { toast: "toast", error: "toast-error" } };
// macOS overlays the traffic lights on the header; they end at x 72 and are centred on y 27.
const IS_MAC = navigator.userAgent.includes("Mac OS X");

function sameRoute(a: Route, b: Route): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

function overlayOpen(): boolean {
  return document.querySelector('[role="dialog"], [role="menu"]') !== null;
}

/** The middle column, memoised so the 250ms playback tick never reaches it. */
const Content = memo(function Content({
  route,
  history,
  onPlayHistory,
  onRetry,
}: {
  route: Route;
  history: HistoryView | null;
  onPlayHistory: (i: number) => void;
  onRetry: (i: number) => void;
}) {
  switch (route.kind) {
    case "home":
      return <HomeView history={history} onPlayHistory={onPlayHistory} />;
    case "playlist":
      return <PlaylistPage index={route.index} onRetry={onRetry} />;
    case "search":
      return <SearchPage query={route.query} />;
    case "album":
      return <AlbumView id={route.id} />;
    case "artist":
      return <ArtistView id={route.id} />;
  }
});

function App() {
  const [signedIn, setSignedIn] = useState<boolean | null>(null);
  const [browsers, setBrowsers] = useState<string[]>([]);
  const [libraryLoaded, setLibraryLoaded] = useState(false);
  const [bootstrapError, setBootstrapError] = useState<string | null>(null);
  const [playlists, setPlaylists] = useState<PlaylistView[]>([]);
  const [nav, setNav] = useState<{ stack: Route[]; at: number }>({ stack: [{ kind: "home" }], at: 0 });
  const [playback, setPlayback] = useState<PlaybackStateView | null>(null);
  const [currentTrack, setCurrentTrack] = useState<Track | null>(null);
  const [view, setView] = useState<"library" | "now-playing">("library");
  const [query, setQuery] = useState("");
  const [lyrics, setLyrics] = useState<LyricsView | null>(null);
  const [lyricsError, setLyricsError] = useState(false);
  const [lyricsLoading, setLyricsLoading] = useState(false);
  const [config, setConfig] = useState<ConfigView | null>(null);
  const [translateMode, setTranslateMode] = useState<TranslateMode>("off");
  const [translation, setTranslation] = useState<string[] | null>(null);
  const [translateBusy, setTranslateBusy] = useState(false);
  const [choices, setChoices] = useState<LyricsChoice[] | null>(null);
  const [pickerOpen, setPickerOpen] = useState(false);
  const [queue, setQueue] = useState<QueueEntry[]>([]);
  const [showQueue, setShowQueue] = useState(false);
  const [history, setHistory] = useState<HistoryView | null>(null);
  const [upNext, setUpNext] = useState<Track | null>(null);
  const [showKeymap, setShowKeymap] = useState(false);
  const [liked, setLiked] = useState<Set<string>>(() => new Set());
  const [addRequest, setAddRequest] = useState<AddRequest | null>(null);
  const route = nav.stack[nav.at];

  // Read by stable callbacks, so the menus and keys see current state without re-binding per tick.
  const latest = useRef({ playback, currentTrack, playlists, liked, route });
  latest.current = { playback, currentTrack, playlists, liked, route };

  const navigate = useCallback((next: Route) => {
    setView("library");
    setNav((n) => {
      if (sameRoute(n.stack[n.at], next)) return n;
      const stack = [...n.stack.slice(Math.max(0, n.at + 1 - MAX_BACK), n.at + 1), next];
      return { stack, at: stack.length - 1 };
    });
  }, []);
  const back = useCallback(() => setNav((n) => (n.at > 0 ? { ...n, at: n.at - 1 } : n)), []);
  const forward = useCallback(() => setNav((n) => (n.at < n.stack.length - 1 ? { ...n, at: n.at + 1 } : n)), []);

  const mainRef = useRef<HTMLElement>(null);
  useEffect(() => {
    mainRef.current?.scrollTo({ top: 0 });
  }, [route]);

  const refreshLiked = useCallback(() => {
    void invoke<string[]>("liked_ids").then((ids) => setLiked(new Set(ids)));
  }, []);
  // Liked too: a webview that loads after the library has missed every event that would have fetched it.
  const refreshPlaylists = useCallback(() => {
    void invoke<PlaylistView[]>("get_playlists").then((list) => {
      setPlaylists(list);
      if (list.length > 0) {
        setLibraryLoaded(true);
        refreshLiked();
      }
    });
  }, [refreshLiked]);

  // One refresh per burst: `library-song-batch` fires once per playlist while the library loads.
  const refreshTimer = useRef<number | undefined>(undefined);
  const scheduleRefresh = useCallback(() => {
    window.clearTimeout(refreshTimer.current);
    refreshTimer.current = window.setTimeout(() => {
      refreshPlaylists();
      refreshLiked();
    }, 150);
  }, [refreshPlaylists, refreshLiked]);

  useEffect(() => {
    void invoke<boolean>("auth_status").then(setSignedIn);
    void invoke<string[]>("list_browsers").then(setBrowsers);
    void invoke<PlaybackStateView | null>("playback_state").then(setPlayback);
    void invoke<ConfigView>("get_config").then(setConfig);
    void invoke<HistoryView>("get_history").then(setHistory);

    const unlisten = [
      listen("library-loaded", () => {
        setLibraryLoaded(true);
        setBootstrapError(null);
        refreshPlaylists();
        refreshLiked();
      }),
      listen("library-song-batch", scheduleRefresh),
      listen<string>("bootstrap-error", (e) => {
        setLibraryLoaded(true);
        setBootstrapError(e.payload);
      }),
      listen<PlaybackStateView>("playback-state", (e) => setPlayback(e.payload)),
      listen("history-changed", () => void invoke<HistoryView>("get_history").then(setHistory)),
      listen<string>("radio-notice", (e) => toast(e.payload)),
    ];
    return () => {
      window.clearTimeout(refreshTimer.current);
      unlisten.forEach((u) => void u.then((f) => f()));
    };
  }, [refreshPlaylists, refreshLiked, scheduleRefresh]);

  // Bootstrap can finish before this webview listens, and events aren't replayed, so poll until it lands.
  useEffect(() => {
    if (signedIn !== true) return;
    refreshPlaylists();
    const poll = window.setInterval(() => {
      if (latest.current.playlists.length > 0) window.clearInterval(poll);
      else refreshPlaylists();
    }, 1000);
    const stop = window.setTimeout(() => {
      window.clearInterval(poll);
      setLibraryLoaded(true);
    }, 15000);
    return () => {
      window.clearInterval(poll);
      window.clearTimeout(stop);
    };
  }, [signedIn, refreshPlaylists]);

  // Keyed on the queue entry and the video mpv has open, since either can move without the other.
  const playingKey =
    playback && (playback.playing || playback.track) ? `${playback.track ?? ""}@${playback.playing?.join(":") ?? ""}` : null;
  useEffect(() => {
    if (playingKey === null) {
      setCurrentTrack(null);
      return;
    }
    let live = true;
    void invoke<Track | null>("current_track").then((t) => live && setCurrentTrack(t));
    return () => {
      live = false;
    };
  }, [playingKey]);

  const lyricsRequest = useRef(0);
  const choicesRequest = useRef(0);

  const loadLyrics = useCallback(() => {
    const at = latest.current.playback?.playing;
    const seq = ++lyricsRequest.current;
    if (!at) {
      setLyrics(null);
      setLyricsError(false);
      setLyricsLoading(false);
      return;
    }
    const [playlist, song] = at;
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
  }, []);

  // A new song drops the translation on screen too: it belongs to the previous record's words.
  useEffect(() => {
    setTranslation(null);
    setChoices(null);
    setPickerOpen(false);
    loadLyrics();
  }, [playback?.track, loadLyrics]);

  useEffect(() => {
    if (translateMode === "off" || !lyrics || lyrics.lines.length === 0) {
      setTranslation(null);
      return;
    }
    let live = true;
    setTranslateBusy(true);
    invoke<{ lines: string[] }>("translate_lyrics", {
      recordId: lyrics.recordId,
      lines: lyrics.lines.map((l) => l.text),
      useAi: translateMode === "ai",
      force: false,
    })
      .then((t) => live && setTranslation(t.lines))
      .catch((e) => {
        if (!live) return;
        setTranslation(null);
        toast.error(`Translation failed: ${errorText(e)}`);
      })
      .finally(() => live && setTranslateBusy(false));
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [translateMode, lyrics?.recordId, lyrics?.lines.length]);

  const openPicker = useCallback(() => {
    const at = latest.current.playback?.playing;
    if (!at) return;
    const [playlist, song] = at;
    setPickerOpen(true);
    setChoices(null);
    const seq = ++choicesRequest.current;
    invoke<LyricsChoice[]>("get_lyrics_choices", { playlist, song, onScreen: lyrics?.recordId ?? null })
      .then((c) => seq === choicesRequest.current && setChoices(c))
      .catch(() => seq === choicesRequest.current && setChoices([]));
  }, [lyrics?.recordId]);

  const pickRecord = useCallback((recordId: number) => {
    const at = latest.current.playback?.playing;
    if (!at) return;
    const [playlist, song] = at;
    setPickerOpen(false);
    const seq = ++lyricsRequest.current;
    invoke<LyricsView | null>("choose_lyrics", { playlist, song, recordId })
      .then((l) => {
        if (seq !== lyricsRequest.current) return;
        setLyrics(l);
        setLyricsError(l === null);
      })
      .catch((e) => toast.error(errorText(e)));
  }, []);

  const refreshQueue = useCallback(() => {
    void invoke<QueueEntry[]>("get_queue").then(setQueue).catch(() => {});
  }, []);

  // Only while the panel is open: `get_queue` resolves every entry, and shut, nothing reads it.
  useEffect(() => {
    if (showQueue) refreshQueue();
  }, [showQueue, playback?.track, playback?.queue_revision, playback?.queue_position, refreshQueue]);

  useEffect(() => {
    if (!playback) return;
    let live = true;
    void invoke<Track | null>("get_up_next")
      .then((t) => live && setUpNext(t))
      .catch(() => {});
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [playback?.track, playback?.queue_revision, playback?.queue_position]);

  const toggleLike = useCallback((track: Track) => {
    const id = track.video_id;
    if (!id) return;
    const was = latest.current.liked.has(id);
    const flip = (on: boolean) =>
      setLiked((s) => {
        const next = new Set(s);
        if (on) next.add(id);
        else next.delete(id);
        return next;
      });
    flip(!was);
    invoke("set_liked", { videoId: id, liked: !was })
      .then(() => toast(was ? "Removed from Liked Music" : "Added to Liked Music"))
      .catch((e) => {
        flip(was);
        toast.error(errorText(e));
      });
  }, []);

  const retryPlaylist = useCallback(
    (index: number) => void call("refetch_playlist", { index }).then(refreshPlaylists),
    [refreshPlaylists],
  );
  const onPlayHistory = useCallback((index: number) => void call("play_target", { target: { kind: "history", index } }), []);
  // Only for a row the pointer rests on: sweeping across a list was an IPC call to the main thread per row.
  const prefetchTimer = useRef<number | undefined>(undefined);
  const prefetched = useRef("");
  const prefetch = useCallback((target: Target) => {
    window.clearTimeout(prefetchTimer.current);
    if (target.kind !== "library") return;
    const key = `${target.playlist}:${target.song}`;
    if (key === prefetched.current) return;
    prefetchTimer.current = window.setTimeout(() => {
      prefetched.current = key;
      void invoke("prefetch", { playlist: target.playlist, song: target.song }).catch(() => {});
    }, 250);
  }, []);

  const onJumpQueue = useCallback((qPos: number) => void call("jump_to", { qPos }).then(refreshQueue), [refreshQueue]);
  const onRemoveQueue = useCallback((qPos: number) => void call("remove_from_queue", { qPos }).then(refreshQueue), [refreshQueue]);
  const onClearQueue = useCallback(() => void call("clear_queue", {}, "Queue cleared").then(refreshQueue), [refreshQueue]);

  const menus = useMemo<Menus>(() => {
    const share = (path: string | null): MenuEntry[] => [
      {
        label: "Copy Link",
        icon: <Link size={14} />,
        disabled: !path,
        onSelect: () => void call("copy_link", { path }, "Link copied"),
      },
      {
        label: "Open in YouTube Music",
        icon: <ExternalLink size={14} />,
        disabled: !path,
        onSelect: () => void call("open_in_browser", { path }),
      },
    ];

    const addTo = (track: Track): MenuEntry[] => {
      const own = latest.current.playlists.filter(
        (p) => p.playlist_id !== LIKED_PLAYLIST_ID && p.playlist_id !== SEARCH_PLAYLIST_ID,
      );
      return [
        { label: "New Playlist…", icon: <Plus size={14} />, onSelect: () => setAddRequest({ track, create: true }) },
        ...(own.length > 0 ? [SEPARATOR, { heading: "Playlists" }] : []),
        ...own.map((p) => ({
          label: p.title,
          onSelect: () => void call("add_to_playlist", { playlistId: p.playlist_id, videoId: track.video_id }, `Added to ${p.title}`),
        })),
      ];
    };

    const track = (t: Track, target: Target, extras: TrackMenuExtras = {}): MenuEntry[] => {
      const vid = t.video_id;
      const title = t.title ?? "the song";
      const isLiked = Boolean(vid && latest.current.liked.has(vid));
      const albumId = t.album?.id ?? null;
      const artists = t.artists.filter((a) => a.id);
      const entry = extras.queueEntry;
      const danger: MenuEntry[] = [];
      if (extras.removeFromPlaylist) {
        danger.push({ label: "Remove from Playlist", icon: <MinusCircle size={14} />, danger: true, onSelect: extras.removeFromPlaylist });
      }
      if (entry) {
        danger.push({
          label: "Remove from Queue",
          icon: <MinusCircle size={14} />,
          danger: true,
          onSelect: () => void call("remove_from_queue", { qPos: entry.qPos }).then(refreshQueue),
        });
      }
      return [
        {
          label: "Play",
          icon: <Play size={14} />,
          onSelect: entry
            ? () => void call("jump_to", { qPos: entry.qPos })
            : () => void call("play_target", { target }),
        },
        {
          label: "Play Next",
          icon: <ListStart size={14} />,
          onSelect: () => void call("queue_target", { target, next: true }, `Playing next: ${title}`),
        },
        {
          label: "Play Last",
          icon: <ListEnd size={14} />,
          onSelect: () => void call("queue_target", { target, next: false }, `Added to the queue: ${title}`),
        },
        SEPARATOR,
        { label: "Start Radio", icon: <Radio size={14} />, disabled: !vid, onSelect: () => void call("radio_target", { target }) },
        { label: "Add to Playlist", disabled: !vid, items: addTo(t) },
        {
          label: isLiked ? "Remove from Liked Music" : "Add to Liked Music",
          icon: isLiked ? <HeartOff size={14} /> : <Heart size={14} />,
          disabled: !vid,
          onSelect: () => toggleLike(t),
        },
        SEPARATOR,
        {
          label: "Go to Album",
          icon: <Disc3 size={14} />,
          disabled: !albumId,
          onSelect: () => albumId && navigate({ kind: "album", id: albumId }),
        },
        artists.length > 1
          ? {
              label: "Go to Artist",
              items: artists.map((a) => ({ label: a.name, onSelect: () => navigate({ kind: "artist", id: a.id! }) })),
            }
          : {
              label: "Go to Artist",
              icon: <MicVocal size={14} />,
              disabled: artists.length === 0,
              onSelect: () => artists[0] && navigate({ kind: "artist", id: artists[0].id! }),
            },
        ...(danger.length > 0 ? [SEPARATOR, ...danger] : []),
        SEPARATOR,
        ...share(vid ? `watch?v=${vid}` : null),
      ];
    };

    const playlist = (index: number): MenuEntry[] => {
      const p = latest.current.playlists[index];
      if (!p) return [];
      const count = p.count ?? 0;
      const ready = p.loaded && count > 0;
      return [
        {
          label: "Play",
          icon: <Play size={14} />,
          disabled: !ready,
          onSelect: () => void call("play_target", { target: { kind: "library", playlist: index, song: 0 } }),
        },
        {
          label: "Shuffle",
          icon: <Shuffle size={14} />,
          disabled: !ready,
          onSelect: async () => {
            await call("set_mode", { mode: "shuffle" });
            void call("play_target", { target: { kind: "library", playlist: index, song: Math.floor(Math.random() * count) } });
          },
        },
        {
          label: "Play Next",
          icon: <ListStart size={14} />,
          disabled: !ready,
          onSelect: () => void call("queue_playlist", { index, next: true }, `Playing ${p.title} next`),
        },
        {
          label: "Play Last",
          icon: <ListEnd size={14} />,
          disabled: !ready,
          onSelect: () => void call("queue_playlist", { index, next: false }, `Added ${p.title} to the queue`),
        },
        SEPARATOR,
        {
          label: p.failed ? "Try Loading Again" : "Refresh",
          icon: <RotateCw size={14} />,
          onSelect: () => void call("refetch_playlist", { index }).then(refreshPlaylists),
        },
        SEPARATOR,
        ...share(`playlist?list=${p.playlist_id}`),
      ];
    };

    const withAlbum = (id: string, f: (tracks: Track[]) => void) =>
      loadAlbum(id)
        .then((a) => f(a.tracks))
        .catch((e) => toast.error(errorText(e)));

    const album = (id: string): MenuEntry[] => [
      { label: "Play", icon: <Play size={14} />, onSelect: () => void withAlbum(id, (tracks) => void call("play_tracks", { tracks, start: 0 })) },
      {
        label: "Shuffle",
        icon: <Shuffle size={14} />,
        onSelect: () =>
          void withAlbum(id, async (tracks) => {
            await call("set_mode", { mode: "shuffle" });
            void call("play_tracks", { tracks, start: Math.floor(Math.random() * tracks.length) });
          }),
      },
      {
        label: "Play Next",
        icon: <ListStart size={14} />,
        onSelect: () => void withAlbum(id, (tracks) => void call("queue_tracks", { tracks, next: true }, "Album plays next")),
      },
      {
        label: "Play Last",
        icon: <ListEnd size={14} />,
        onSelect: () => void withAlbum(id, (tracks) => void call("queue_tracks", { tracks, next: false }, "Album added to the queue")),
      },
      SEPARATOR,
      { label: "Go to Album", icon: <Disc3 size={14} />, onSelect: () => navigate({ kind: "album", id }) },
      SEPARATOR,
      ...share(`browse/${id}`),
    ];

    const artist = (id: string): MenuEntry[] => [
      { label: "Go to Artist", icon: <MicVocal size={14} />, onSelect: () => navigate({ kind: "artist", id }) },
      SEPARATOR,
      ...share(`channel/${id}`),
    ];

    return { track, playlist, album, artist };
  }, [navigate, toggleLike, refreshQueue, refreshPlaylists]);

  const currentTrackId = currentTrack?.video_id ?? null;
  const paused = playback?.paused ?? true;
  const actions = useMemo<AppActions>(() => ({ navigate, toggleLike, menus, prefetch }), [navigate, toggleLike, menus, prefetch]);
  const appState = useMemo<AppState>(
    () => ({ playlists, liked, currentTrackId, paused, route }),
    [playlists, liked, currentTrackId, paused, route],
  );

  // Stable, so the dialogs' memo holds through the 250ms playback tick.
  const closeKeymap = useCallback(() => setShowKeymap(false), []);
  const closeAdd = useCallback(() => setAddRequest(null), []);

  const nowPlaying = view === "now-playing";
  const hasLyrics = Boolean(lyrics && lyrics.lines.length > 0);
  const openNowPlaying = useCallback(() => {
    if (latest.current.currentTrack) setView("now-playing");
  }, []);

  // Rebuilt every render so each action sees current state; the one listener below reads it through a ref.
  const keyActions = useRef<Record<string, () => void>>({});
  keyActions.current = {
    n: () => send("next"),
    p: () => send("prev"),
    m: () => send("toggle_mute"),
    t: () => send("cycle_mode"),
    L: () => currentTrack && toggleLike(currentTrack),
    R: () => {
      const at = playback?.playing;
      if (at) void call("radio_target", { target: { kind: "library", playlist: at[0], song: at[1] } });
    },
    P: () => currentTrack?.video_id && setAddRequest({ track: currentTrack, create: false }),
    "?": () => setShowKeymap(true),
    y: () => (nowPlaying ? setView("library") : openNowPlaying()),
    Escape: () => {
      if (nowPlaying) setView("library");
      else if (showQueue) setShowQueue(false);
      else back();
    },
    ...(nowPlaying
      ? {
          c: () => hasLyrics && openPicker(),
          i: () => config?.translateTo && setTranslateMode((m) => (m === "free" ? "off" : "free")),
          I: () => config?.aiAvailable && setTranslateMode((m) => (m === "ai" ? "off" : "ai")),
        }
      : {
          s: () => document.getElementById(SEARCH_INPUT_ID)?.focus(),
          "/": () => document.getElementById(FILTER_INPUT_ID)?.focus(),
          o: () => setShowQueue((v) => !v),
          H: () => navigate({ kind: "home" }),
        }),
  };

  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      const target = e.target as HTMLElement | null;
      const tag = target?.tagName;
      if (tag === "INPUT" || tag === "SELECT" || tag === "TEXTAREA") {
        // Esc leaves a text field, so the letter keys work again without the mouse.
        if (e.key === "Escape") target?.blur();
        return;
      }
      // A dialog or menu owns the keyboard while it is open.
      if (overlayOpen()) return;
      if (e.metaKey && (e.key === "[" || e.key === "]")) {
        e.preventDefault();
        if (e.key === "[") back();
        else forward();
        return;
      }
      if (e.ctrlKey || e.metaKey || e.altKey) return;
      // Enter on a focused row plays it; on a button it presses it.
      if (e.key === "Enter") return;

      const current = latest.current.playback;
      switch (e.key) {
        case " ":
          e.preventDefault();
          send("play_pause");
          return;
        case "ArrowLeft":
          e.preventDefault();
          send("seek", { deltaSecs: -SEEK_STEP });
          return;
        case "ArrowRight":
          e.preventDefault();
          send("seek", { deltaSecs: SEEK_STEP });
          return;
        case "ArrowUp":
          e.preventDefault();
          if (current) send("set_volume", { volume: Math.min(100, current.volume + VOLUME_STEP) });
          return;
        case "ArrowDown":
          e.preventDefault();
          if (current) send("set_volume", { volume: Math.max(0, current.volume - VOLUME_STEP) });
          return;
      }
      const action = keyActions.current[e.key];
      if (action) {
        e.preventDefault();
        action();
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [back, forward]);

  const onSearch = (e: React.FormEvent) => {
    e.preventDefault();
    const q = query.trim();
    if (q) navigate({ kind: "search", query: q });
  };

  return (
    <Tooltip.Provider delayDuration={500} skipDelayDuration={200}>
      <ActionsContext.Provider value={actions}>
      <StateContext.Provider value={appState}>
        <MenuProvider>
          <div className="fixed inset-0 flex flex-col overflow-hidden bg-bg text-ink">
            <header data-tauri-drag-region className={`flex h-[54px] flex-shrink-0 items-center gap-1 pr-3 ${IS_MAC ? "pl-20" : "pl-3"}`}>
              {signedIn && (
                <>
                  <Tip label="Back (⌘[)" side="bottom">
                    <button onClick={back} disabled={nav.at === 0} className="nav-arrow">
                      <ChevronLeft size={18} />
                    </button>
                  </Tip>
                  <Tip label="Forward (⌘])" side="bottom">
                    <button onClick={forward} disabled={nav.at >= nav.stack.length - 1} className="nav-arrow">
                      <ChevronRight size={18} />
                    </button>
                  </Tip>
                  <div data-tauri-drag-region className="flex flex-1 justify-center">
                    <form role="search" onSubmit={onSearch} className="flex w-full max-w-sm min-w-0 items-center gap-2 rounded-full px-3.5 py-1.5 glass">
                      <Search size={15} className="flex-shrink-0 text-ink-faint" />
                      <input
                        id={SEARCH_INPUT_ID}
                        type="search"
                        value={query}
                        onChange={(e) => setQuery(e.target.value)}
                        onContextMenu={nativeMenu}
                        placeholder="Search YouTube Music (s)"
                        aria-label="Search YouTube Music"
                        className="w-full bg-transparent text-body outline-none placeholder:text-ink-faint"
                      />
                    </form>
                  </div>
                  {/* Balances the arrows, so the search field sits at the window's centre. */}
                  <span className="w-16 flex-shrink-0" />
                </>
              )}
            </header>

            {signedIn === null && <p className="p-12 text-center text-ink-dim select-none">Checking session…</p>}
            {signedIn === false && browsers.length > 0 && (
              <SignIn
                browsers={browsers}
                onSignedIn={() => {
                  setSignedIn(true);
                  setBootstrapError(null);
                }}
              />
            )}

            {signedIn === true && (
              <div className="flex min-h-0 flex-1">
                <PlaylistNav loaded={libraryLoaded} onRetry={retryPlaylist} />
                {/* The island floats over this column's foot, so the clearance is padding inside the scroller. */}
                <main ref={mainRef} className="thin-scrollbar min-w-0 flex-1 overflow-y-auto px-5 pt-2 pb-[var(--player-bar-h)]">
                  {bootstrapError && playlists.length === 0 && (
                    <div role="alert" className="mx-3 mb-6 flex items-center gap-4 rounded-2xl bg-accent/10 px-4 py-3">
                      <p className="min-w-0 flex-1 text-body text-ink">
                        Couldn't load your library. <span className="text-ink-dim">{bootstrapError}</span>
                      </p>
                      <button onClick={() => setSignedIn(false)} className="btn btn-secondary flex-shrink-0">
                        Sign in again
                      </button>
                    </div>
                  )}
                  <Content route={route} history={history} onPlayHistory={onPlayHistory} onRetry={retryPlaylist} />
                </main>
                {showQueue && (
                  <QueuePanel entries={queue} onJump={onJumpQueue} onRemove={onRemoveQueue} onClear={onClearQueue} />
                )}
              </div>
            )}

            {playback && signedIn && !nowPlaying && (
              <PlayerBar
                playback={playback}
                currentTrack={currentTrack}
                onOpenNowPlaying={openNowPlaying}
                queueOpen={showQueue}
                onToggleQueue={() => setShowQueue((v) => !v)}
              />
            )}

            <AnimatePresence>
              {nowPlaying && playback && (
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
                  upNext={upNext}
                  onClose={() => setView("library")}
                  onSetTranslateMode={setTranslateMode}
                  onRetryLyrics={loadLyrics}
                  onOpenPicker={openPicker}
                  onClosePicker={() => setPickerOpen(false)}
                  onPickRecord={pickRecord}
                />
              )}
            </AnimatePresence>

            <KeymapOverlay open={showKeymap} onClose={closeKeymap} />
            <AddToPlaylistDialog request={addRequest} playlists={playlists} onClose={closeAdd} onCreated={refreshPlaylists} />
            <Toaster position="bottom-center" offset={TOAST_OFFSET} toastOptions={TOAST_OPTIONS} />
          </div>
        </MenuProvider>
      </StateContext.Provider>
      </ActionsContext.Provider>
    </Tooltip.Provider>
  );
}

export default App;
