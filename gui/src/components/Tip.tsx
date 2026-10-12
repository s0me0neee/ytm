import type { ReactElement } from "react";
import * as Tooltip from "@radix-ui/react-tooltip";

/** A hover label for an icon-only control. Also names it, via `aria-label`, for screen readers. */
export function Tip({ label, children, side = "top" }: { label: string; children: ReactElement; side?: "top" | "bottom" }) {
  return (
    <Tooltip.Root>
      <Tooltip.Trigger asChild aria-label={label}>
        {children}
      </Tooltip.Trigger>
      <Tooltip.Portal>
        <Tooltip.Content side={side} sideOffset={8} className="tip">
          {label}
        </Tooltip.Content>
      </Tooltip.Portal>
    </Tooltip.Root>
  );
}
