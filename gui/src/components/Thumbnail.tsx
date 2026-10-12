import { useEffect, useRef, useState } from "react";
import { Music2 } from "lucide-react";
import { viaBackend } from "../cover";

interface ThumbnailProps {
  srcs: (string | null | undefined)[];
  alt?: string;
  className: string;
  /** Reports the decoded image's width ÷ height, for a box that takes the picture's shape. */
  onAspect?: (ratio: number) => void;
}

// One shared observer: `loading="lazy"` defers nothing in WebKitGTK, and a playlist mounts hundreds of rows.
const nearWatchers = new Map<Element, () => void>();
let nearObserver: IntersectionObserver | null = null;

function watchNear(el: Element, onNear: () => void): () => void {
  if (typeof IntersectionObserver === "undefined") {
    onNear();
    return () => {};
  }
  nearObserver ??= new IntersectionObserver(
    (entries) => {
      for (const entry of entries) {
        if (!entry.isIntersecting) continue;
        const cb = nearWatchers.get(entry.target);
        nearWatchers.delete(entry.target);
        nearObserver?.unobserve(entry.target);
        cb?.();
      }
    },
    // `scrollMargin` reaches past each column's own scroll clip, which `rootMargin` cannot.
    { rootMargin: "400px", scrollMargin: "400px" } as IntersectionObserverInit,
  );
  nearWatchers.set(el, onNear);
  nearObserver.observe(el);
  return () => {
    nearWatchers.delete(el);
    nearObserver?.unobserve(el);
  };
}

/** Tries each candidate in order, ending on a note glyph if all fail. */
export function Thumbnail({ srcs, alt = "", className, onAspect }: ThumbnailProps) {
  const candidates = srcs.filter((s): s is string => Boolean(s)).map(viaBackend);
  const key = candidates.join("|");
  const [idx, setIdx] = useState(0);
  // Latched, so scrolling back past a row never re-requests or blanks its cover.
  const [near, setNear] = useState(false);
  const box = useRef<HTMLDivElement>(null);

  useEffect(() => {
    setIdx(0);
  }, [key]);

  useEffect(() => {
    if (near || !box.current) return;
    return watchNear(box.current, () => setNear(true));
  }, [near, key]);

  const failed = () => setIdx((i) => i + 1);

  if (!near && candidates.length > 0) {
    return <div ref={box} className={`${className} bg-surface-2`} aria-hidden />;
  }

  if (idx >= candidates.length) {
    return (
      <div className={`${className} flex items-center justify-center bg-surface-2 text-ink-ghost select-none`} aria-hidden>
        <Music2 className="h-2/5 w-2/5" strokeWidth={1.5} />
      </div>
    );
  }

  // The advertised (smallest) URL shows underneath while a bigger one loads or is retried.
  const under = idx < candidates.length - 1 ? candidates[candidates.length - 1] : null;

  return (
    <div className={`${className} relative overflow-hidden`}>
      {under && (
        <img
          src={under}
          alt=""
          aria-hidden
          loading="lazy"
          decoding="async"
          draggable={false}
          className="absolute inset-0 h-full w-full object-cover select-none"
        />
      )}
      <img
        key={candidates[idx]}
        src={candidates[idx]}
        alt={alt}
        loading="lazy"
        decoding="async"
        className="absolute inset-0 h-full w-full animate-[thumbnail-fade-in_0.25s_ease] object-cover select-none"
        draggable={false}
        onError={failed}
        onLoad={(e) => {
          const img = e.currentTarget;
          // WebKit draws its broken-image glyph for a 200 that failed to decode, without firing `error`.
          if (img.naturalWidth === 0) {
            failed();
            return;
          }
          onAspect?.(img.naturalWidth / img.naturalHeight);
        }}
      />
    </div>
  );
}
