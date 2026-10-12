import { Heart, ListMusic } from "lucide-react";
import { send } from "../api";
import { useActions, useAppState } from "../context";
import { artistNames, formatTime } from "../format";
import type { PlaybackStateView, Track } from "../types";
import { useMenu } from "./Menu";
import { Slider } from "./Slider";
import { Thumbnail } from "./Thumbnail";
import { Tip } from "./Tip";
import { Transport, Volume } from "./Transport";

interface PlayerBarProps {
  playback: PlaybackStateView;
  currentTrack: Track | null;
  onOpenNowPlaying: () => void;
  queueOpen: boolean;
  onToggleQueue: () => void;
}

/** The island: transport, the track with its seek line, and the icons — the one piece that redraws per tick. */
export function PlayerBar({ playback, currentTrack, onOpenNowPlaying, queueOpen, onToggleQueue }: PlayerBarProps) {
  const { liked } = useAppState();
  const { toggleLike, menus } = useActions();
  const menu = useMenu();
  const remaining = Math.max(0, playback.total - playback.elapsed);
  const album = currentTrack?.album?.name;
  const isLiked = Boolean(currentTrack?.video_id && liked.has(currentTrack.video_id));
  const target = playback.playing ? { kind: "library" as const, playlist: playback.playing[0], song: playback.playing[1] } : null;

  return (
    <div className={`player-rail has-nav ${queueOpen ? "has-queue" : ""}`}>
      <footer
        aria-label="Player"
        className="player-island grid grid-cols-[auto_minmax(0,1fr)_auto] items-stretch gap-4 px-4 select-none"
      >
        <div className="flex items-center">
          <Transport playback={playback} />
        </div>

        <div
          className="player-island-lcd flex min-w-0 items-center"
          onContextMenu={(e) => {
            if (currentTrack && target) menu.show(e, menus.track(currentTrack, target));
          }}
        >
          <button
            onClick={onOpenNowPlaying}
            disabled={!currentTrack}
            aria-label="Open Now Playing (y)"
            className="player-bar-info flex min-w-0 items-center gap-3 text-left"
          >
            <Thumbnail srcs={[currentTrack?.thumbnail]} className="h-10 w-10 flex-shrink-0 rounded-md object-cover" />
            <span className="min-w-0">
              <p className="truncate text-body font-semibold text-ink">{currentTrack?.title ?? "Nothing playing"}</p>
              {currentTrack && (
                <p className="truncate text-meta text-ink-dim">
                  {artistNames(currentTrack)}
                  {album && ` — ${album}`}
                </p>
              )}
            </span>
          </button>
          {/* Remaining rather than total: it answers "can I start something else yet". */}
          <div className="player-bar-clock pointer-events-none absolute inset-x-0 flex items-center justify-between font-mono text-label text-ink tabular-nums">
            <span>{formatTime(playback.elapsed)}</span>
            <span>−{formatTime(remaining)}</span>
          </div>
          <Slider
            className="slider-seekbar"
            label="Seek"
            value={playback.elapsed}
            max={playback.total}
            onCommit={(v) => send("seek_to", { secs: v })}
          />
        </div>

        <div className="flex min-w-0 flex-shrink-0 items-center justify-end gap-3">
          <Tip label={isLiked ? "Remove from Liked Music (L)" : "Add to Liked Music (L)"}>
            <button
              onClick={() => currentTrack && toggleLike(currentTrack)}
              disabled={!currentTrack?.video_id}
              aria-pressed={isLiked}
              className={`disabled:opacity-30 ${isLiked ? "text-accent" : "text-ink-dim hover:text-ink"}`}
            >
              <Heart size={16} fill={isLiked ? "currentColor" : "none"} />
            </button>
          </Tip>
          <Tip label="Up Next (o)">
            <button
              onClick={onToggleQueue}
              aria-pressed={queueOpen}
              className={queueOpen ? "text-accent" : "text-ink-dim hover:text-ink"}
            >
              <ListMusic size={16} />
            </button>
          </Tip>
          <Volume playback={playback} />
        </div>
      </footer>
    </div>
  );
}
