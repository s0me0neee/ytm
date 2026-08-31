import * as RadixSlider from "@radix-ui/react-slider";

interface SliderProps {
  value: number;
  max: number;
  onChange: (value: number) => void;
  className?: string;
}

/** A Radix-backed slider: track, filled range, and thumb are three
 * primitives Radix positions together, so they can't drift apart the way a
 * hand-styled native `<input type="range">` did. */
export function Slider({ value, max, onChange, className }: SliderProps) {
  return (
    <RadixSlider.Root
      className={`slider-root ${className ?? ""}`}
      value={[value]}
      max={max || 1}
      step={0.1}
      onValueChange={([v]) => onChange(v)}
    >
      <RadixSlider.Track className="slider-track">
        <RadixSlider.Range className="slider-range" />
      </RadixSlider.Track>
      <RadixSlider.Thumb className="slider-thumb" />
    </RadixSlider.Root>
  );
}
