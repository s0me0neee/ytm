// Mirrors ytm-core/src/cover.rs's at_size/hd_ladder rewrites; pure URL work, no fetch.

import { convertFileSrc } from "@tauri-apps/api/core";

/** `url`, fetched by the backend's `cover://` scheme, which throttles what the webview would burst. */
export function viaBackend(url: string): string {
  return convertFileSrc(url, "cover");
}

// `hq720.jpg` is left out: it exists exactly when `maxresdefault` does, so it only adds a request.
const YT_THUMB_SIZES = ["default.jpg", "mqdefault.jpg", "hqdefault.jpg", "sddefault.jpg", "maxresdefault.jpg"];
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

/** Every named video frame above the advertised one, biggest first, up to the first that covers `px`. */
function hdLadder(url: string, px: number): string[] {
  const base = url.split("?")[0];
  const idx = base.lastIndexOf("/");
  if (idx === -1) return [];
  const prefix = base.slice(0, idx);
  const name = base.slice(idx + 1);
  if (!prefix.includes("i.ytimg.com/vi")) return [];
  const at = YT_THUMB_SIZES.indexOf(name);
  if (at === -1) return [];
  const fits = YT_THUMB_WIDTHS.findIndex((w) => w >= px);
  const top = fits === -1 ? YT_THUMB_SIZES.length - 1 : fits;
  return YT_THUMB_SIZES.slice(at + 1, top + 1)
    .reverse()
    .map((s) => `${prefix}/${s}`);
}

/** Everything worth trying for one cover, best first, ending with the advertised URL. */
export function coverCandidates(url: string, px: number): string[] {
  const ladder = hdLadder(url, px);
  const all = ladder.length > 0 ? [...ladder, url] : [coverAtSize(url, px), url];
  return [...new Set(all)];
}

/** For a blurred backdrop: the advertised URL, which always exists and is already cached. */
export function backdropUrl(url: string): string {
  return viaBackend(url);
}
