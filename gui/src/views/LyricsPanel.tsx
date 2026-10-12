import { memo, useEffect, useRef, useState, type RefObject } from "react";
import type { LyricLineView } from "../types";

// Apple's scroll, measured: 334ms on cubic-bezier(0.4, 0, 0.6, 1). Driven by rAF since smooth scrollIntoView queues.
const LYRIC_SCROLL_MS = 334;
// How long the sheet stays where the user scrolled it before following the song again.
const LYRIC_IDLE_MS = 3000;

/** `cubic-bezier(0.4, 0, 0.6, 1)`: x is inverted by Newton's method, four steps being sub-pixel here. */
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
  synced: boolean;
  translation: string[] | null;
  /** The cover's box: the sung line is centred on it, not on the panel. */
  coverRef: RefObject<HTMLElement | null>;
  onSeek: (at: number) => void;
}

export const LyricsPanel = memo(function LyricsPanel({ lines, activeLine, synced, translation, coverRef, onSeek }: LyricsPanelProps) {
  const activeTextRef = useRef<HTMLParagraphElement>(null);
  const boxRef = useRef<HTMLDivElement>(null);
  const frame = useRef(0);
  const idle = useRef<number | undefined>(undefined);
  // While the user scrolls, auto-scroll stands down and sung lines come back.
  const [browsing, setBrowsing] = useState(false);

  useEffect(() => {
    const box = boxRef.current;
    if (!box) return;
    // Input events rather than `scroll`, which can't tell the wheel from our own rAF writes.
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
    const el = activeTextRef.current;
    if (!synced || activeLine < 0 || !el) {
      cancelAnimationFrame(frame.current);
      box.scrollTop = 0;
      return;
    }
    // The lyric's centre (not its wrapper's, which grows with a translation) against the cover's.
    const line = el.getBoundingClientRect();
    const cover = coverRef.current?.getBoundingClientRect();
    const boxRect = box.getBoundingClientRect();
    const centre = cover ? cover.top + cover.height / 2 : boxRect.top + boxRect.height / 2;
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
    // `px-4` is headroom for the active line's 1.05 scale, which overflow-y would otherwise clip.
    <div
      ref={boxRef}
      className={`no-scrollbar h-full w-full max-w-lg overflow-y-auto px-4 pt-[45vh] pb-[70vh] [mask-image:linear-gradient(to_bottom,transparent,black_80px,black_50%,transparent)] ${
        browsing ? "lyrics-browsing" : ""
      }`}
    >
      {lines.map((l, i) => {
        const under = translation?.[i];
        const showUnder = Boolean(under) && under !== l.text;
        const state =
          !synced || activeLine < 0 ? "is-plain" : i === activeLine ? "is-active" : i < activeLine && !browsing ? "is-past" : "";
        return (
          // A button, so a line is a seek target; mousedown's default would focus it and fight the rAF scroll.
          <button
            key={i}
            type="button"
            disabled={!synced}
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => onSeek(l.at)}
            className={`lyric-line mb-7 block w-full cursor-pointer text-center disabled:cursor-default ${state}`}
          >
            <p ref={i === activeLine ? activeTextRef : undefined} className="text-lyric leading-[1.28] font-semibold">
              {l.text || "• • •"}
            </p>
            {showUnder && <p className="mt-1.5 text-lyric-sub leading-[1.28] font-medium opacity-70">{under}</p>}
          </button>
        );
      })}
    </div>
  );
});
