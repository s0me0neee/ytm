import { memo } from "react";
import { toast } from "sonner";
import { call } from "../api";
import { useActions } from "../context";
import { coverCandidates } from "../cover";
import { errorText } from "../format";
import type { Card } from "../types";
import { loadAlbum } from "../views/AlbumView";
import { CardActions } from "./CardActions";
import { useMenu } from "./Menu";
import { Shelf } from "./Shelf";
import { Thumbnail } from "./Thumbnail";

function playAlbum(id: string) {
  loadAlbum(id)
    .then((a) => void call("play_tracks", { tracks: a.tracks, start: 0 }))
    .catch((e) => toast.error(errorText(e)));
}

/** A row of album or artist cards, scrolled sideways. */
export const CardShelf = memo(function CardShelf({ title, cards }: { title: string; cards: Card[] }) {
  const { navigate, menus } = useActions();
  const menu = useMenu();
  return (
    <Shelf title={title} className="mb-6" rowClassName="flex gap-2 px-1 pb-2">
      {cards.map((c) => {
        const round = c.kind === "Artist";
        return (
          <div
            key={c.id}
            className="group relative w-40 flex-shrink-0"
            onContextMenu={(e) => menu.show(e, c.kind === "Album" ? menus.album(c.id) : menus.artist(c.id))}
          >
            <button
              onClick={() => navigate(c.kind === "Album" ? { kind: "album", id: c.id } : { kind: "artist", id: c.id })}
              className="card w-full rounded-2xl p-2 text-left group-hover:bg-surface"
            >
              <Thumbnail
                srcs={c.thumbnail ? coverCandidates(c.thumbnail, 320) : []}
                className={`mb-2 aspect-square w-full ${round ? "rounded-full" : "rounded-xl"}`}
              />
              <p className={`truncate text-body text-ink ${round ? "text-center" : ""}`}>{c.title}</p>
              {c.subtitle && <p className={`truncate text-meta text-ink-dim ${round ? "text-center" : ""}`}>{c.subtitle}</p>}
            </button>
            {/* A round portrait has no corners to put them in, and Apple's artist cards carry none. */}
            {!round && <CardActions label={c.title} onPlay={() => playAlbum(c.id)} />}
          </div>
        );
      })}
    </Shelf>
  );
});
