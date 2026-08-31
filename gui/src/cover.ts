// Mirrors ytm-core/src/cover.rs's at_size/hd_variant string rewrites, done
// client-side since they're pure URL manipulation with no fetch involved.

/** The named frames YouTube serves for a video, smallest first.
 *
 * `hq720.jpg` is deliberately absent though it exists: it is the same
 * 1280x720 as `maxresdefault` and is generated under the same condition, and
 * measured over the library every video that fell past `maxresdefault` fell
 * past `hq720` too -- so it only ever added a request to the slow path. See
 * `hd_ladder` in ytm-core/src/cover.rs. */
const YT_THUMB_SIZES = ["default.jpg", "mqdefault.jpg", "hqdefault.jpg", "sddefault.jpg", "maxresdefault.jpg"];

/** Rewrites a Google image URL's `w120-h120-...` size params to `px`. */
export function coverAtSize(url: string, px: number): string {
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
export function hdLadder(url: string): string[] {
  const base = url.split("?")[0];
  const idx = base.lastIndexOf("/");
  if (idx === -1) return [];
  const prefix = base.slice(0, idx);
  const name = base.slice(idx + 1);
  if (!prefix.includes("i.ytimg.com/vi")) return [];
  const at = YT_THUMB_SIZES.indexOf(name);
  if (at === -1) return [];
  return YT_THUMB_SIZES.slice(at + 1)
    .reverse()
    .map((s) => `${prefix}/${s}`);
}

/** Everything worth trying for one cover, best first, ending with the URL the
 * API advertised -- which always exists and is what the last rung falls back
 * to. Shaped for `Thumbnail`'s `srcs`, which walks exactly this order. */
export function coverCandidates(url: string, px: number): string[] {
  const ladder = hdLadder(url);
  // Art tracks have no named frames; their size lives in the URL and the CDN
  // serves any of them, so the rewrite *is* the high-quality copy.
  return ladder.length > 0 ? [...ladder, url] : [coverAtSize(url, px), url];
}

/** The single best URL for a cover, where only one can be given (a CSS
 * `background-image`, which has no fallback chain). */
export function bestCoverUrl(url: string, px: number): string {
  return coverCandidates(url, px)[0];
}
