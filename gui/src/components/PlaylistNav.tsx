import { memo } from "react";
import { Home, RotateCw } from "lucide-react";
import { useActions, useAppState } from "../context";
import { SEARCH_PLAYLIST_ID } from "../types";
import { useMenu } from "./Menu";
import { Thumbnail } from "./Thumbnail";

/** Home and the user's playlists. `loaded` tells "still fetching" from "you have none". */
export const PlaylistNav = memo(function PlaylistNav({
  loaded,
  onRetry,
}: {
  loaded: boolean;
  onRetry: (i: number) => void;
}) {
  const { playlists, route } = useAppState();
  const { navigate, menus } = useActions();
  const menu = useMenu();
  const selected = route.kind === "playlist" ? route.index : null;
  const home = route.kind === "home";
  const shown = playlists.map((p, i) => ({ p, i })).filter(({ p }) => p.playlist_id !== SEARCH_PLAYLIST_ID);

  return (
    <nav aria-label="Library" className="thin-scrollbar m-3 w-60 flex-shrink-0 overflow-y-auto rounded-2xl p-2 select-none glass">
      <button
        onClick={() => navigate({ kind: "home" })}
        aria-current={home ? "page" : undefined}
        className="nav-item"
      >
        <span className="flex h-7 w-7 flex-shrink-0 items-center justify-center rounded-md bg-surface-2">
          <Home size={14} />
        </span>
        <span className="min-w-0 flex-1 truncate">Home</span>
      </button>

      <p className="px-3 pt-4 pb-2 text-label font-semibold tracking-wider text-ink-faint uppercase">Playlists</p>
      {!loaded && shown.length === 0 && (
        <ul aria-busy aria-label="Loading playlists">
          {Array.from({ length: 6 }, (_, i) => (
            <li key={i} className="flex items-center gap-2.5 px-2 py-1.5" style={{ opacity: 1 - i * 0.13 }}>
              <span className="h-7 w-7 rounded-md bg-surface-2" />
              <span className="h-2.5 w-28 rounded bg-surface-2" />
            </li>
          ))}
        </ul>
      )}
      {loaded && shown.length === 0 && <p className="px-3 text-body text-ink-faint">No playlists yet.</p>}

      <ul>
        {shown.map(({ p, i }) => (
          <li key={p.playlist_id} className="relative" onContextMenu={(e) => menu.show(e, menus.playlist(i))}>
            <button
              onClick={() => navigate({ kind: "playlist", index: i })}
              aria-current={selected === i ? "page" : undefined}
              className="nav-item"
            >
              <Thumbnail srcs={[p.thumbnail]} className="h-7 w-7 flex-shrink-0 rounded-md" />
              <span className="min-w-0 flex-1 truncate">{p.title}</span>
              {p.count !== null && !p.failed && (
                <span className="font-mono text-label text-ink-ghost tabular-nums">{p.count}</span>
              )}
              {!p.loaded && !p.failed && <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-ink-ghost" aria-label="Loading" />}
            </button>
            {p.failed && (
              <button
                onClick={() => onRetry(i)}
                title="Couldn't load this playlist — try again"
                className="absolute top-1/2 right-2 flex -translate-y-1/2 items-center gap-1 rounded-md px-1.5 py-0.5 text-label text-accent hover:bg-surface-2"
              >
                <RotateCw size={11} /> Retry
              </button>
            )}
          </li>
        ))}
      </ul>
    </nav>
  );
});
