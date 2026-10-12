import { memo } from "react";

/** Three bars that bounce while playing and hold still, dimmed, when paused. */
export const PlayingIndicator = memo(function PlayingIndicator({ paused }: { paused: boolean }) {
  return (
    <span className="flex h-3.5 w-4 items-end justify-center gap-0.5" aria-label={paused ? "Paused" : "Playing"}>
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
