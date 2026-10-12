import { Pause, Play, Repeat, Repeat1, Shuffle, SkipBack, SkipForward, Volume1, Volume2, VolumeX } from "lucide-react";
import { send } from "../api";
import type { PlaybackStateView } from "../types";
import { Slider } from "./Slider";
import { Tip } from "./Tip";

function modeIcon(mode: string, size: number) {
  const m = mode.toLowerCase();
  if (m.includes("shuffle")) return <Shuffle size={size} />;
  if (m.includes("single") || m.includes("one")) return <Repeat1 size={size} />;
  return <Repeat size={size} />;
}

/** Mode, previous, play/pause, next — the bar's compact set or Now Playing's large one. */
export function Transport({ playback, large = false }: { playback: PlaybackStateView; large?: boolean }) {
  const cycling = playback.mode.toLowerCase().includes("cycle");
  const skip = large ? 22 : 18;
  return (
    <div className={`flex items-center ${large ? "gap-7" : "gap-4"}`}>
      <Tip label={`Play mode: ${playback.mode.replace(/^\S+\s+/, "")} (t)`}>
        <button
          onClick={() => send("cycle_mode")}
          className={cycling ? "text-ink-dim hover:text-ink" : "text-accent"}
        >
          {modeIcon(playback.mode, large ? 18 : 16)}
        </button>
      </Tip>
      <Tip label="Previous (p)">
        <button onClick={() => send("prev")} className="text-ink-dim hover:text-ink active:scale-90">
          <SkipBack size={skip} fill="currentColor" />
        </button>
      </Tip>
      <Tip label={playback.paused ? "Play (Space)" : "Pause (Space)"}>
        <button
          onClick={() => send("play_pause")}
          className={`flex items-center justify-center rounded-full bg-ink text-bg hover:bg-white active:scale-95 ${
            large ? "h-14 w-14" : "h-8 w-8"
          }`}
        >
          {playback.paused ? (
            <Play size={large ? 24 : 15} fill="currentColor" />
          ) : (
            <Pause size={large ? 24 : 15} fill="currentColor" />
          )}
        </button>
      </Tip>
      <Tip label="Next (n)">
        <button onClick={() => send("next")} className="text-ink-dim hover:text-ink active:scale-90">
          <SkipForward size={skip} fill="currentColor" />
        </button>
      </Tip>
    </div>
  );
}

/** Mute and the volume slider. */
export function Volume({ playback, width = "w-20" }: { playback: PlaybackStateView; width?: string }) {
  const Icon = playback.muted || playback.volume === 0 ? VolumeX : playback.volume < 50 ? Volume1 : Volume2;
  return (
    <div className="flex items-center gap-2">
      <Tip label={playback.muted ? "Unmute (m)" : "Mute (m)"}>
        <button onClick={() => send("toggle_mute")} className="text-ink-dim hover:text-ink">
          <Icon size={16} />
        </button>
      </Tip>
      <Slider
        className={width}
        label="Volume"
        value={playback.volume}
        max={100}
        onChange={(v) => send("set_volume", { volume: Math.round(v) })}
      />
    </div>
  );
}
