import { useEffect, useRef, useState } from "react";
import { Music2 } from "lucide-react";
import { viaBackend } from "./cover";

interface ThumbnailProps {
  srcs: (string | null | undefined)[];
  alt?: string;
  className: string;
  /** Reports the decoded image's width ÷ height, once it is known.
   *
   * For callers that give the box a fixed shape this is noise, which is why it
   * is optional. It exists for the one that cannot: album art is square and a
   * video's thumbnail is 16:9, and the Now Playing page draws the cover at a
   * size where cropping one to the other is the difference between the artwork
   * and a strip out of the middle of it. The same split `ytm-core`'s
   * `cover::hd_ladder` and the TUI's `App::cover_aspect` deal with -- a cover
   * keeps its own shape, and the box is built to match rather than the picture
   * being made to fit the box. */
  onAspect?: (ratio: number) => void;
}

/* Whether a thumbnail is near enough to the screen to be worth fetching.

   `loading="lazy"` was meant to do this and measurably does nothing in
   WebKitGTK: opening a playlist fired a request per row, ~170 at once, which
   is the burst the CDN answers with 429 -- and every one of them queued in
   front of the cover actually being looked at. So the `src` is withheld until
   the row is close, and a playlist opens with the twenty-odd requests the
   screen can show.

   One observer for every thumbnail rather than one each: a playlist mounts
   hundreds of rows, and an observer per row is a native object and a set of
   intersection computations per row. `scrollMargin` is what reaches the rows
   just past the edge of the scrolling column -- `rootMargin` only grows the
   viewport, and each column is its own scroll container that clips first. An
   engine that doesn't know it ignores it, and rows then load as they enter. */
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
    { rootMargin: "400px", scrollMargin: "400px" } as IntersectionObserverInit,
  );
  nearWatchers.set(el, onNear);
  nearObserver.observe(el);
  return () => {
    nearWatchers.delete(el);
    nearObserver?.unobserve(el);
  };
}

/** Tries each candidate URL in order, falling back on load failure, and
 * finally to a quiet note-icon placeholder if every one 404s. The fade-in is
 * a plain CSS animation keyed to mount, not gated on the `load` event -- for
 * an already browser-cached image (revisiting a playlist, a recently-played
 * track) that event can fire before React attaches its listener, or not at
 * all, which left covers stuck invisible under a JS-driven opacity gate.
 *
 * `onLoad` also advances the fallback chain when `naturalWidth` comes back
 * 0 -- WebKit (the GUI's engine on macOS) renders its own broken-image glyph
 * for a response that came back 200 but failed to decode (a truncated or
 * corrupt body, seen from Google's thumbnail CDN under load) without firing
 * `error` the way a 404 does, so `error` alone left that glyph stuck on
 * screen instead of falling through to the next candidate. */
export function Thumbnail({ srcs, alt = "", className, onAspect }: ThumbnailProps) {
  const candidates = srcs.filter((s): s is string => Boolean(s)).map(viaBackend);
  const key = candidates.join("|");
  const [idx, setIdx] = useState(0);
  // Latched: once a row has been near the screen its cover stays wanted, so
  // scrolling back past it never re-requests or blanks it.
  const [near, setNear] = useState(false);
  const box = useRef<HTMLDivElement>(null);

  useEffect(() => {
    setIdx(0);
  }, [key]);

  // `key` too: a thumbnail that mounts with nothing to show (the player bar
  // before a track is known) renders no box to watch, so the watch has to
  // start when candidates arrive rather than only on mount.
  useEffect(() => {
    if (near || !box.current) return;
    return watchNear(box.current, () => setNear(true));
  }, [near, key]);

  // Final: the backend already retried whatever could succeed on a second ask.
  const failed = () => setIdx((i) => i + 1);

  if (!near && candidates.length > 0) {
    return <div ref={box} className={`${className} bg-surface-2`} aria-hidden />;
  }

  if (idx >= candidates.length) {
    return (
      <div
        className={`${className} flex items-center justify-center bg-surface-2 text-ink-ghost select-none`}
        aria-hidden
      >
        <Music2 className="h-2/5 w-2/5" strokeWidth={1.5} />
      </div>
    );
  }

  /* The smallest candidate, shown underneath while a better one is being
     fetched -- and, more to the point, while the backend is re-asking for it,
     which under a rate limit takes seconds. An empty square for that long is a
     worse answer than a soft picture immediately. It is the URL the API advertised, ~15KB and
     already in the fallback chain, so this costs one small request and never
     a second one for a row (where there is only ever one candidate).

     No state tracks the handover: an `img` paints nothing until it has
     decoded, so the underlay simply shows through until the one on top is
     ready, and the fade-in turns that into a crossfade. When every candidate
     including this one has failed, `idx` runs off the end and the note above
     is what's left -- which is correct, since the underlay is the last
     candidate and so failed too. */
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
        /* A playlist row's cover is one of hundreds on screen at once. `lazy`
         * keeps the ones scrolled out of view from being fetched at all, and
         * `async` decoding keeps the ones that are from decoding on the main
         * thread -- together they were most of the stall when a large playlist
         * was opened, since every row otherwise raced to fetch and decode
         * during the same frames the list was trying to scroll. */
        loading="lazy"
        decoding="async"
        className="absolute inset-0 h-full w-full animate-[thumbnail-fade-in_0.25s_ease] object-cover select-none"
        draggable={false}
        onError={failed}
        onLoad={(e) => {
          const img = e.currentTarget;
          if (img.naturalWidth === 0) {
            failed();
            return;
          }
          // Only the real candidate reports, never the underlay -- which is a
          // different resolution of the same picture and would be measuring
          // the placeholder's shape rather than the one on screen.
          onAspect?.(img.naturalWidth / img.naturalHeight);
        }}
      />
    </div>
  );
}
