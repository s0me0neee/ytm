// Mirrors ytm-core/src/cover.rs's at_size/hd_variant string rewrites, done
// client-side since they're pure URL manipulation with no fetch involved.

import { convertFileSrc } from "@tauri-apps/api/core";

/** `url`, fetched by the backend's `cover://` scheme rather than by the
 * webview, which fires a request per row at once and gets 429'd by the CDN
 * for it. See `src-tauri/src/cover.rs`. */
export function viaBackend(url: string): string {
  return convertFileSrc(url, "cover");
}

/** The named frames YouTube serves for a video, smallest first.
 *
 * `hq720.jpg` is deliberately absent though it exists: it is the same
 * 1280x720 as `maxresdefault` and is generated under the same condition, and
 * measured over the library every video that fell past `maxresdefault` fell
 * past `hq720` too -- so it only ever added a request to the slow path. See
 * `hd_ladder` in ytm-core/src/cover.rs. */
const YT_THUMB_SIZES = ["default.jpg", "mqdefault.jpg", "hqdefault.jpg", "sddefault.jpg", "maxresdefault.jpg"];

/** Each of those frames' width, in the same order. */
const YT_THUMB_WIDTHS = [120, 320, 480, 640, 1280];

/** Rewrites a Google image URL's `w120-h120-...` size params to `px`. */
function coverAtSize(url: string, px: number): string {
  const eq = url.lastIndexOf("=");
  if (eq === -1) return url;
  const base = url.slice(0, eq);
  const params = url.slice(eq + 1);
  if (!params.includes("-h") && !params.startsWith("w")) return url;
  const rewritten = params.split("-").map((part) => (/^[wh]\d+$/.test(part) ? part[0] + px : part));
  return `${base}=${rewritten.join("-")}`;
}

/** Every named frame larger than the one `url` advertises, biggest first, or
 * empty if `url` isn't a YouTube video thumbnail this applies to.
 *
 * A ladder rather than one guess: `maxresdefault` exists only for videos
 * uploaded in HD, and asking for it alone then falling straight back to the
 * advertised crop left a twentieth of the library at 400x225 when
 * `sddefault` (640x480) was there for nearly all of them. Each rung can 404 --
 * callers fall through. */
function hdLadder(url: string, px: number): string[] {
  const base = url.split("?")[0];
  const idx = base.lastIndexOf("/");
  if (idx === -1) return [];
  const prefix = base.slice(0, idx);
  const name = base.slice(idx + 1);
  if (!prefix.includes("i.ytimg.com/vi")) return [];
  const at = YT_THUMB_SIZES.indexOf(name);
  if (at === -1) return [];
  /* Capped at the smallest frame that already covers `px`. Every rung is a
     request, and under a rate limit a request is what there is least of: a
     160px home card asked for the 1280px `maxresdefault` first, 404'd on it
     for any video not uploaded in HD, then asked for `sddefault` -- three
     requests for a picture the advertised 480px frame already covered. */
  const fits = YT_THUMB_WIDTHS.findIndex((w) => w >= px);
  const top = fits === -1 ? YT_THUMB_SIZES.length - 1 : fits;
  return YT_THUMB_SIZES.slice(at + 1, top + 1)
    .reverse()
    .map((s) => `${prefix}/${s}`);
}

/** Everything worth trying for one cover, best first, ending with the URL the
 * API advertised -- which always exists and is what the last rung falls back
 * to. Shaped for `Thumbnail`'s `srcs`, which walks exactly this order. */
export function coverCandidates(url: string, px: number): string[] {
  const ladder = hdLadder(url, px);
  // Art tracks have no named frames; their size lives in the URL and the CDN
  // serves any of them, so the rewrite *is* the high-quality copy.
  const all = ladder.length > 0 ? [...ladder, url] : [coverAtSize(url, px), url];
  // Deduplicated: a video whose advertised frame already covers `px` has an
  // empty ladder, and `coverAtSize` leaves a URL with no size params alone --
  // listing it twice would draw it twice and, on failure, ask for it twice.
  return [...new Set(all)];
}

/** The cover for a blurred backdrop, where only one URL can be given (a CSS
 * `background-image` has no fallback chain).
 *
 * The advertised URL, not a larger rewrite: under a 20-40px blur no one can
 * see the difference, it is the one URL guaranteed to exist -- a video's
 * `maxresdefault`, which this used to pick, 404s for anything not uploaded in
 * HD and left the page black -- and the player bar has already fetched it, so
 * it costs no request at all. */
export function backdropUrl(url: string): string {
  return viaBackend(url);
}
