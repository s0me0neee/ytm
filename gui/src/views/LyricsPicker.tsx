import { Mic2 } from "lucide-react";
import { Modal } from "../components/Modal";
import { formatTime } from "../format";
import type { LyricsChoice } from "../types";

interface LyricsPickerProps {
  open: boolean;
  choices: LyricsChoice[] | null;
  onScreen: number | undefined;
  onPick: (id: number) => void;
  onClose: () => void;
}

/** `c`: every lrclib record for this track, so a bad automatic match can be replaced. */
export function LyricsPicker({ open, choices, onScreen, onPick, onClose }: LyricsPickerProps) {
  return (
    <Modal open={open} onClose={onClose} title="Choose lyrics" icon={<Mic2 size={16} className="text-ink-dim" />} width="max-w-lg">
      <div className="thin-scrollbar min-h-0 flex-1 overflow-y-auto px-3 pb-3">
        {choices === null && <p className="px-3 py-6 text-center text-body text-ink-dim">Searching lrclib…</p>}
        {choices?.length === 0 && <p className="px-3 py-6 text-center text-body text-ink-dim">No records for this track.</p>}
        {choices?.map((c) => (
          <button
            key={c.id}
            onClick={() => onPick(c.id)}
            className={`list-button ${c.id === onScreen ? "bg-surface" : ""}`}
          >
            <span className="min-w-0 flex-1">
              <p className="truncate text-body text-ink">{c.trackName}</p>
              <p className="truncate text-meta text-ink-dim">
                {c.artistName}
                {c.albumName && ` — ${c.albumName}`}
              </p>
            </span>
            <span className="flex flex-shrink-0 items-center gap-2 text-label">
              {c.id === onScreen && <span className="font-semibold text-accent">IN USE</span>}
              {c.timingMismatch && <span className="text-ink-faint">off-length</span>}
              <span className={c.synced ? "text-ink-dim" : "text-ink-faint"}>{c.synced ? "synced" : "plain"}</span>
              <span className="font-mono text-ink-ghost">{c.duration ? formatTime(c.duration) : "—"}</span>
            </span>
          </button>
        ))}
      </div>
    </Modal>
  );
}
