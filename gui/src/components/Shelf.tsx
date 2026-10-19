import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { ChevronLeft, ChevronRight } from "lucide-react";

/** A titled row scrolled sideways, with arrows: without a horizontal wheel, a hidden scrollbar is no way across. */
export function Shelf({
  title,
  className = "",
  rowClassName = "",
  children,
}: {
  title: string;
  className?: string;
  /** The row's own layout (`flex gap-2`, a column-flowing grid), padding included. */
  rowClassName?: string;
  children: ReactNode;
}) {
  const row = useRef<HTMLDivElement>(null);
  const [canBack, setCanBack] = useState(false);
  const [canForward, setCanForward] = useState(false);

  const measure = useCallback(() => {
    const el = row.current;
    if (!el) return;
    setCanBack(el.scrollLeft > 1);
    setCanForward(el.scrollLeft + el.clientWidth < el.scrollWidth - 1);
  }, []);

  useEffect(() => {
    const el = row.current;
    if (!el) return;
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, [measure]);
  // Cards arriving change the row's content width without resizing the row itself.
  useEffect(measure);

  // By whole items, as Apple pages: the one cut off at the edge becomes the first in view.
  const page = (dir: 1 | -1) => {
    const el = row.current;
    if (!el) return;
    const pad = parseFloat(getComputedStyle(el).paddingLeft) || 0;
    const items = Array.from(el.children) as HTMLElement[];
    const view = el.scrollLeft;
    const to =
      dir === 1
        ? items.find((c) => c.offsetLeft + c.offsetWidth > view + el.clientWidth + 1)?.offsetLeft
        : items.find((c) => c.offsetLeft - pad >= view - el.clientWidth)?.offsetLeft;
    el.scrollTo({ left: to === undefined ? (dir === 1 ? el.scrollWidth : 0) : to - pad, behavior: "smooth" });
  };

  const arrow = (dir: 1 | -1) => (
    <button
      onClick={() => page(dir)}
      aria-label={`Scroll ${title} ${dir === 1 ? "forward" : "back"}`}
      className={`shelf-arrow absolute top-1/2 z-10 -translate-y-1/2 ${dir === 1 ? "right-0" : "left-0"}`}
    >
      {dir === 1 ? <ChevronRight size={18} /> : <ChevronLeft size={18} />}
    </button>
  );

  return (
    <section className={`group/shelf @container ${className}`}>
      <h2 className="section-heading">{title}</h2>
      <div className="relative">
        <div ref={row} onScroll={measure} className={`no-scrollbar relative overflow-x-auto ${rowClassName}`}>
          {children}
        </div>
        {canBack && arrow(-1)}
        {canForward && arrow(1)}
      </div>
    </section>
  );
}
