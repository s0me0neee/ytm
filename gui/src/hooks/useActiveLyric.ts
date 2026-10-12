import { useCallback, useEffect, useState } from "react";
import type { LyricLineView } from "../types";

/** Which line is playing at `elapsed`, or -1 before the first; binary search, as `lrclib::lrc::active_index`. */
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

// How long a clicked line outranks playback ticks, which still carry the pre-seek position.
const SEEK_HOLD_MS = 1000;

/** The active line on a clock extrapolated between the 250ms ticks, re-rendered at each line's timestamp. */
export function useActiveLyric(
  lines: LyricLineView[] | null,
  elapsed: number,
  paused: boolean,
  offset: number,
): [number, (at: number) => void] {
  const [base, setBase] = useState({ at: elapsed, time: performance.now(), seeked: 0 });
  const [, setWake] = useState(0);

  useEffect(() => {
    setBase((b) => {
      const now = performance.now();
      const projected = b.at + (paused ? 0 : (now - b.time) / 1000);
      if (now - b.seeked < SEEK_HOLD_MS && Math.abs(elapsed - projected) > 1) return b;
      return { at: elapsed, time: now, seeked: 0 };
    });
  }, [elapsed, paused]);

  const clock = base.at + (paused ? 0 : (performance.now() - base.time) / 1000) + offset;
  const active = lines ? activeLyricIndex(lines, clock) : -1;

  useEffect(() => {
    if (!lines || paused) return;
    const next = lines[active + 1];
    if (!next) return;
    const t = window.setTimeout(() => setWake((w) => w + 1), Math.max(0, (next.at - clock) * 1000) + 5);
    return () => window.clearTimeout(t);
  }, [lines, active, clock, paused]);

  const seeked = useCallback(
    (at: number) => setBase({ at: at - offset, time: performance.now(), seeked: performance.now() }),
    [offset],
  );
  return [active, seeked];
}
