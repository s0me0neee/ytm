import { memo } from "react";
import { useActions } from "../context";
import { coverCandidates } from "../cover";
import type { Card } from "../types";
import { useMenu } from "./Menu";
import { Thumbnail } from "./Thumbnail";

/** A row of album or artist cards, scrolled sideways. */
export const CardShelf = memo(function CardShelf({ title, cards }: { title: string; cards: Card[] }) {
  const { navigate, menus } = useActions();
  const menu = useMenu();
  return (
    <section className="mb-6">
      <h2 className="section-heading">{title}</h2>
      <div className="no-scrollbar flex gap-2 overflow-x-auto px-1 pb-2">
        {cards.map((c) => {
          const round = c.kind === "Artist";
          return (
            <button
              key={c.id}
              onClick={() => navigate(c.kind === "Album" ? { kind: "album", id: c.id } : { kind: "artist", id: c.id })}
              onContextMenu={(e) => menu.show(e, c.kind === "Album" ? menus.album(c.id) : menus.artist(c.id))}
              className="card w-40 flex-shrink-0 rounded-2xl p-2 text-left"
            >
              <Thumbnail
                srcs={c.thumbnail ? coverCandidates(c.thumbnail, 320) : []}
                className={`mb-2 aspect-square w-full ${round ? "rounded-full" : "rounded-xl"}`}
              />
              <p className={`truncate text-body text-ink ${round ? "text-center" : ""}`}>{c.title}</p>
              {c.subtitle && <p className={`truncate text-meta text-ink-dim ${round ? "text-center" : ""}`}>{c.subtitle}</p>}
            </button>
          );
        })}
      </div>
    </section>
  );
});
