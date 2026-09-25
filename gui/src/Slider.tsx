import { useEffect, useState } from "react";
import * as RadixSlider from "@radix-ui/react-slider";

interface SliderProps {
  value: number;
  max: number;
  onChange?: (value: number) => void;
  // Once, on release: a seek per pointer-move made mpv sound like it played at the wrong speed.
  onCommit?: (value: number) => void;
  className?: string;
}

// Longest a released position is held if playback never reports reaching it.
const HOLD_MS = 1500;

/** A Radix-backed slider: track, filled range, and thumb are three
 * primitives Radix positions together, so they can't drift apart the way a
 * hand-styled native `<input type="range">` did. */
export function Slider({ value, max, onChange, onCommit, className }: SliderProps) {
  // The thumb's position while dragging, and after release until `value` catches up.
  const [held, setHeld] = useState<{ at: number; released: boolean } | null>(null);

  // Let go once playback reaches the released position...
  useEffect(() => {
    if (held?.released && Math.abs(value - held.at) < 1.5) setHeld(null);
  }, [value, held]);

  // ...or after HOLD_MS regardless. Keyed on `held` alone, so playback ticks can't keep re-arming it.
  useEffect(() => {
    if (!held?.released) return;
    const t = window.setTimeout(() => setHeld(null), HOLD_MS);
    return () => window.clearTimeout(t);
  }, [held]);

  return (
    <RadixSlider.Root
      className={`slider-root ${className ?? ""}`}
      value={[held?.at ?? value]}
      max={max || 1}
      step={0.1}
      onValueChange={([v]) => (onCommit ? setHeld({ at: v, released: false }) : onChange?.(v))}
      onValueCommit={([v]) => {
        if (!onCommit) return;
        setHeld({ at: v, released: true });
        onCommit(v);
      }}
    >
      <RadixSlider.Track className="slider-track">
        <RadixSlider.Range className="slider-range" />
      </RadixSlider.Track>
      <RadixSlider.Thumb className="slider-thumb" />
    </RadixSlider.Root>
  );
}
