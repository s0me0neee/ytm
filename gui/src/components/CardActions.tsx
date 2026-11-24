import { MoreHorizontal, Play } from "lucide-react";
import { openMenuFrom } from "./Menu";
import { PlayingIndicator } from "./PlayingIndicator";

/** Play and "…" over a card's artwork: beside the card's button, not in it, with the menu on the card's wrapper. */
export function CardActions({
  label,
  onPlay,
  playing = null,
}: {
  label: string;
  onPlay?: () => void;
  /** Whether this card is what's playing, and if so whether it's paused. */
  playing?: { paused: boolean } | null;
}) {
  return (
    <span className="pointer-events-none absolute inset-x-2 top-2 flex aspect-square items-end justify-between p-2">
      {onPlay ? (
        <button onClick={onPlay} aria-label={`Play ${label}`} className={`card-action ${playing ? "is-playing" : ""}`}>
          {playing ? <PlayingIndicator paused={playing.paused} /> : <Play size={14} fill="currentColor" />}
        </button>
      ) : (
        <span />
      )}
      <button onClick={(e) => openMenuFrom(e.currentTarget)} aria-label={`More for ${label}`} className="card-action">
        <MoreHorizontal size={16} />
      </button>
    </span>
  );
}
