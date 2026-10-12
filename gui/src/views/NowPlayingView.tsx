import { useCallback, useRef, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import { ChevronDown, Heart, Languages, ListMusic, MessageSquareText, MoreHorizontal, RotateCw } from "lucide-react";
import { send } from "../api";
import { openMenuFrom, useMenu } from "../components/Menu";
import { Slider } from "../components/Slider";
import { Thumbnail } from "../components/Thumbnail";
import { Tip } from "../components/Tip";
import { ArtistLinks } from "../components/TrackList";
import { Transport, Volume } from "../components/Transport";
import { useActions, useAppState } from "../context";
import { backdropUrl, coverCandidates } from "../cover";
import { artistNames, formatTime } from "../format";
import { useActiveLyric } from "../hooks/useActiveLyric";
import type { ConfigView, LyricsChoice, LyricsView, PlaybackStateView, Track, TranslateMode } from "../types";
import { LyricsPanel } from "./LyricsPanel";
import { LyricsPicker } from "./LyricsPicker";

// macOS overlays the traffic lights on this header; the lights sit at x 20–72, centred on y 27.
const IS_MAC = navigator.userAgent.includes("Mac OS X");

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
  upNext: Track | null;
  onClose: () => void;
  onSetTranslateMode: (m: TranslateMode) => void;
  onRetryLyrics: () => void;
  onOpenPicker: () => void;
  onClosePicker: () => void;
  onPickRecord: (id: number) => void;
}

/** Apple Music's split: artwork and transport centred in the left half, the lyric sheet in the right. */
export function NowPlayingView(props: NowPlayingViewProps) {
  const { playback, currentTrack, lyrics, lyricsError, lyricsLoading, config, translation, translateMode, translateBusy } = props;
  const { liked } = useAppState();
  const { toggleLike, menus, navigate } = useActions();
  const menu = useMenu();
  const offset = config?.lyricsOffset ?? 0;
  const [activeLine, lyricSeeked] = useActiveLyric(lyrics?.synced ? lyrics.lines : null, playback.elapsed, playback.paused, offset);
  // A lyric's `at` is on the offset clock, so seeking to it undoes the offset.
  const onSeekLyric = useCallback(
    (at: number) => {
      lyricSeeked(at);
      send("seek_to", { secs: Math.max(0, at - offset) });
    },
    [offset, lyricSeeked],
  );
  const [showLyrics, setShowLyrics] = useState(true);
  const coverRef = useRef<HTMLDivElement>(null);

  // The cover's own shape (square art, 16:9 video), keyed by track so a stale ratio is never applied.
  const [cover, setCover] = useState<{ id: string | null; ratio: number }>({ id: null, ratio: 1 });
  const trackId = currentTrack?.video_id ?? null;
  const coverAspect = cover.id === trackId ? cover.ratio : 1;
  const onCoverAspect = useCallback((ratio: number) => setCover({ id: trackId, ratio }), [trackId]);
  const hasLyrics = Boolean(lyrics && lyrics.lines.length > 0);
  const canTranslate = Boolean(config?.translateTo) && hasLyrics;
  // One predicate for the column and the cover's width, so they never disagree mid-fetch.
  const lyricsColumn = showLyrics && (hasLyrics || lyricsLoading || lyricsError);
  const isLiked = Boolean(trackId && liked.has(trackId));
  const target = playback.playing ? { kind: "library" as const, playlist: playback.playing[0], song: playback.playing[1] } : null;
  const showMenu = (e: React.MouseEvent<HTMLElement>) => {
    if (currentTrack && target) menu.show(e, menus.track(currentTrack, target));
  };

  return (
    <motion.div
      initial={{ opacity: 0, y: 24 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: 24 }}
      transition={{ duration: 0.35, ease: [0.22, 1, 0.36, 1] }}
      className="absolute inset-0 z-20 overflow-hidden bg-bg text-ink"
    >
      <AnimatePresence mode="wait">
        {currentTrack?.thumbnail ? (
          // Its own layer, or WebKitGTK re-blurs it on every seek-bar tick.
          <motion.div
            key={currentTrack.video_id ?? "bg"}
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.5 }}
            className="absolute inset-0 scale-125 bg-cover bg-center blur-3xl saturate-[1.7] brightness-110 will-change-transform"
            style={{ backgroundImage: `url(${backdropUrl(currentTrack.thumbnail)})` }}
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
      {/* A scrim for contrast; heavy enough that white text survives a near-white cover. */}
      <div className="absolute inset-0 bg-linear-to-b from-black/25 via-black/35 to-black/60" />

      <div className="relative z-10 flex h-full flex-col">
        <div data-tauri-drag-region className={`flex h-[54px] flex-shrink-0 items-center pr-6 ${IS_MAC ? "pl-20" : "pl-6"}`}>
          <Tip label="Back to library (y)" side="bottom">
            <button
              onClick={props.onClose}
              className="flex h-8 w-8 items-center justify-center rounded-full text-ink-dim select-none hover:bg-white/10 hover:text-ink"
            >
              <ChevronDown size={18} />
            </button>
          </Tip>
        </div>

        <div className="flex min-h-0 flex-1 overflow-hidden">
          <div className={`flex items-center justify-center px-8 ${lyricsColumn ? "w-1/2" : "w-full"}`}>
            <motion.div
              key={currentTrack?.video_id ?? "np-none"}
              initial={{ opacity: 0, scale: 0.92, y: 10 }}
              animate={{ opacity: 1, scale: 1, y: 0 }}
              transition={{ duration: 0.4, ease: [0.22, 1, 0.36, 1] }}
              className="flex w-full max-w-[29rem] flex-col items-center text-center select-none"
            >
              <div
                ref={coverRef}
                className="relative w-full"
                style={{ aspectRatio: coverAspect, maxHeight: "48vh" }}
                onContextMenu={showMenu}
              >
                {currentTrack?.thumbnail && (
                  // A tight halo of the cover's own colour, so it reads as a lit object on the wash behind.
                  <div
                    aria-hidden
                    className="pointer-events-none absolute inset-0 scale-[1.04] rounded-2xl bg-cover bg-center opacity-40 blur-[20px] saturate-[2]"
                    style={{ backgroundImage: `url(${backdropUrl(currentTrack.thumbnail)})` }}
                  />
                )}
                <Thumbnail
                  // 1200: the box is 464 CSS px, 928 device px on 2x; the CDN serves anything up to 1400.
                  srcs={currentTrack?.thumbnail ? coverCandidates(currentTrack.thumbnail, 1200) : []}
                  className="relative h-full w-full rounded-2xl object-cover shadow-2xl shadow-black/60"
                  onAspect={onCoverAspect}
                  alt={currentTrack?.title ?? ""}
                />
              </div>

              <div className="on-artwork mt-6 flex w-full items-center gap-3" onContextMenu={showMenu}>
                <Tip label={isLiked ? "Remove from Liked Music (L)" : "Add to Liked Music (L)"}>
                  <button
                    onClick={() => currentTrack && toggleLike(currentTrack)}
                    disabled={!trackId}
                    aria-pressed={isLiked}
                    className={`flex h-8 w-8 flex-shrink-0 items-center justify-center rounded-full hover:bg-white/10 disabled:opacity-30 ${
                      isLiked ? "text-accent" : "text-ink-dim hover:text-ink"
                    }`}
                  >
                    <Heart size={18} fill={isLiked ? "currentColor" : "none"} />
                  </button>
                </Tip>
                <div className="min-w-0 flex-1">
                  <h2 className="truncate text-title font-bold">{currentTrack?.title ?? "Nothing playing"}</h2>
                  {currentTrack && (
                    <p className="mt-1 truncate text-heading text-ink-dim">
                      <ArtistLinks artists={currentTrack.artists} />
                      {currentTrack.album?.id && (
                        <>
                          {" — "}
                          <button className="link" onClick={() => navigate({ kind: "album", id: currentTrack.album!.id! })}>
                            {currentTrack.album.name}
                          </button>
                        </>
                      )}
                    </p>
                  )}
                </div>
                <Tip label="More">
                  <button
                    onClick={(e) => openMenuFrom(e.currentTarget)}
                    disabled={!currentTrack}
                    className="flex h-8 w-8 flex-shrink-0 items-center justify-center rounded-full text-ink-dim hover:bg-white/10 hover:text-ink"
                  >
                    <MoreHorizontal size={18} />
                  </button>
                </Tip>
              </div>

              <div className="mt-7 w-full">
                <Slider
                  className="w-full"
                  label="Seek"
                  value={playback.elapsed}
                  max={playback.total}
                  onCommit={(v) => send("seek_to", { secs: v })}
                />
                <div className="on-artwork mt-1.5 flex justify-between font-mono text-label text-ink-dim tabular-nums select-none">
                  <span>{formatTime(playback.elapsed)}</span>
                  <span>−{formatTime(Math.max(0, playback.total - playback.elapsed))}</span>
                </div>
                <div className="mt-5 flex justify-center select-none">
                  <Transport playback={playback} large />
                </div>
                <div className="mt-5 flex justify-center">
                  <Volume playback={playback} width="w-40" />
                </div>
                {/* Reserved when empty, so the block above doesn't jump when the queue runs out. */}
                <p className="on-artwork mt-5 h-5 truncate text-meta text-ink-faint">
                  {props.upNext && (
                    <>
                      <span className="font-semibold tracking-wider uppercase">Up next</span>
                      <span className="mx-2 text-ink-ghost">·</span>
                      <span className="text-ink-dim">{props.upNext.title ?? "Untitled"}</span>
                      {artistNames(props.upNext) && <span> — {artistNames(props.upNext)}</span>}
                    </>
                  )}
                </p>
              </div>
            </motion.div>
          </div>

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
                  <p className="text-body text-ink-faint">Looking for lyrics…</p>
                </div>
              ) : (
                <div className="flex h-full flex-col items-center justify-center gap-3 text-center">
                  <p className="text-body text-ink-dim">Couldn't load lyrics for this track.</p>
                  <button onClick={props.onRetryLyrics} className="round-control w-auto gap-1.5 px-4 text-body">
                    <RotateCw size={13} /> Try again
                  </button>
                </div>
              )}
            </div>
          )}
        </div>

        <div className="absolute right-4 bottom-4 z-20 flex items-center gap-2 select-none">
          {canTranslate && showLyrics && (
            <>
              <Tip label={`Translate to ${config?.translateTo} (i)`}>
                <button
                  onClick={() => props.onSetTranslateMode(translateMode === "free" ? "off" : "free")}
                  aria-pressed={translateMode === "free"}
                  className={`round-control ${translateMode === "free" ? "is-on" : ""} ${translateBusy && translateMode === "free" ? "animate-pulse" : ""}`}
                >
                  <Languages size={18} />
                </button>
              </Tip>
              {config?.aiAvailable && (
                <Tip label={`Translate to ${config.translateTo} with the AI model (I)`}>
                  <button
                    onClick={() => props.onSetTranslateMode(translateMode === "ai" ? "off" : "ai")}
                    aria-pressed={translateMode === "ai"}
                    className={`round-control w-auto gap-1 px-3 text-label font-semibold ${translateMode === "ai" ? "is-on" : ""} ${
                      translateBusy && translateMode === "ai" ? "animate-pulse" : ""
                    }`}
                  >
                    <Languages size={16} /> AI
                  </button>
                </Tip>
              )}
            </>
          )}
          {hasLyrics && showLyrics && (
            <Tip label="Choose a different lyrics record (c)">
              <button onClick={props.onOpenPicker} className="round-control">
                <ListMusic size={18} />
              </button>
            </Tip>
          )}
          {hasLyrics && (
            <Tip label={showLyrics ? "Hide lyrics" : "Show lyrics"}>
              <button
                onClick={() => setShowLyrics((v) => !v)}
                aria-pressed={showLyrics}
                className={`round-control ${showLyrics ? "is-on" : ""}`}
              >
                <MessageSquareText size={18} />
              </button>
            </Tip>
          )}
        </div>

        <LyricsPicker
          open={props.pickerOpen}
          choices={props.choices}
          onScreen={lyrics?.recordId}
          onPick={props.onPickRecord}
          onClose={props.onClosePicker}
        />
      </div>
    </motion.div>
  );
}
