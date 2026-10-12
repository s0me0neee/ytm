import { memo } from "react";
import { Keyboard } from "lucide-react";
import { Modal } from "./Modal";

/** The same letters as the TUI wherever the action exists in both. */
const GUI_KEYMAP: [string, string][] = [
  ["Space", "Pause / resume"],
  ["p / n", "Previous / next"],
  ["← / →", "Seek ∓5s"],
  ["↑ / ↓", "Volume ±5"],
  ["m", "Mute / unmute"],
  ["t", "Cycle play mode"],
  ["L", "Like / unlike the playing song"],
  ["R", "Radio from the playing song"],
  ["P", "Add the playing song to a playlist"],
  ["", ""],
  ["s", "Search YouTube Music"],
  ["/", "Filter this playlist"],
  ["o", "Toggle the queue"],
  ["H", "Home · recently played"],
  ["Enter", "Play the selected song"],
  ["", ""],
  ["y", "Now playing · lyrics"],
  ["c", "Choose lyrics source"],
  ["i", "Toggle translation"],
  ["I", "Translate with the AI model"],
  ["", ""],
  ["?", "Show this help"],
  ["Esc", "Close · back"],
];

export const KeymapOverlay = memo(function KeymapOverlay({ open, onClose }: { open: boolean; onClose: () => void }) {
  return (
    <Modal open={open} onClose={onClose} title="Keyboard shortcuts" icon={<Keyboard size={16} className="text-ink-dim" />}>
      <dl className="thin-scrollbar grid grid-cols-[auto_1fr] gap-x-5 gap-y-1.5 overflow-y-auto px-5 pb-5 text-body">
        {GUI_KEYMAP.map(([key, desc], i) =>
          key ? (
            <div key={key} className="contents">
              <dt className="text-right">
                <kbd className="rounded-md bg-surface-2 px-1.5 py-0.5 font-mono text-label text-ink">{key}</kbd>
              </dt>
              <dd className="text-ink-dim">{desc}</dd>
            </div>
          ) : (
            <div key={`gap-${i}`} className="col-span-2 h-1.5" />
          ),
        )}
      </dl>
    </Modal>
  );
});
