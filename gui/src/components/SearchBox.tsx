import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Clock, Search, X } from "lucide-react";
import { call } from "../api";
import { useActions } from "../context";
import { resultTrack } from "../format";
import { clearSearches, forgetSearch, recentSearches } from "../recent";
import type { Card, SearchResult, Suggestions } from "../types";
import { nativeMenu } from "./Menu";
import { Thumbnail } from "./Thumbnail";

// Long enough that a word typed at speed is one request, short enough to feel live.
const DEBOUNCE_MS = 150;
// Backspacing over a query asks for nothing it has already asked for.
const cache = new Map<string, Suggestions>();

type Item =
  | { kind: "recent"; query: string }
  | { kind: "query"; query: string }
  | { kind: "card"; card: Card }
  | { kind: "song"; song: SearchResult };

/** The header's search field, with Apple's dropdown: recent searches when empty, suggestions as you type. */
export function SearchBox({
  id,
  value,
  onChange,
  onSearch,
}: {
  id: string;
  value: string;
  onChange: (v: string) => void;
  onSearch: (query: string) => void;
}) {
  const { navigate } = useActions();
  const [open, setOpen] = useState(false);
  const [found, setFound] = useState<Suggestions | null>(null);
  const [recent, setRecent] = useState<string[]>([]);
  const [active, setActive] = useState(-1);
  const field = useRef<HTMLInputElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const typed = value.trim();

  useEffect(() => {
    if (!open || !typed) return;
    const hit = cache.get(typed);
    if (hit) {
      setFound(hit);
      return;
    }
    let live = true;
    const t = window.setTimeout(() => {
      invoke<Suggestions>("search_suggestions", { input: typed })
        .then((s) => {
          cache.set(typed, s);
          if (live) setFound(s);
        })
        // A nicety: Enter still searches without them.
        .catch(() => {});
    }, DEBOUNCE_MS);
    return () => {
      live = false;
      window.clearTimeout(t);
    };
  }, [open, typed]);

  // The previous answer stays up until the next lands, so the list doesn't blink per keystroke.
  const items = useMemo<Item[]>(() => {
    if (!typed) return recent.map((query) => ({ kind: "recent", query }));
    if (!found) return [];
    return [
      ...found.queries.map((query): Item => ({ kind: "query", query })),
      ...found.cards.map((card): Item => ({ kind: "card", card })),
      ...found.songs.map((song): Item => ({ kind: "song", song })),
    ];
  }, [typed, recent, found]);
  const shown = open && items.length > 0;

  useEffect(() => setActive(-1), [items]);
  useEffect(() => {
    if (active >= 0) list.current?.children[active + (typed ? 0 : 1)]?.scrollIntoView({ block: "nearest" });
  }, [active, typed]);

  const close = () => {
    setOpen(false);
    field.current?.blur();
  };
  const submit = (q: string) => {
    const query = q.trim();
    if (!query) return;
    onChange(query);
    close();
    onSearch(query);
  };
  const activate = (item: Item) => {
    switch (item.kind) {
      case "recent":
      case "query":
        submit(item.query);
        return;
      case "card":
        close();
        navigate(item.card.kind === "Album" ? { kind: "album", id: item.card.id } : { kind: "artist", id: item.card.id });
        return;
      case "song":
        close();
        void call("play_tracks", { tracks: [resultTrack(item.song)], start: 0 });
    }
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if ((e.key === "ArrowDown" || e.key === "ArrowUp") && shown) {
      e.preventDefault();
      const n = items.length;
      // -1 is the field itself, so the list wraps through it as Apple's does.
      setActive((i) => (e.key === "ArrowDown" ? (i + 1 >= n ? -1 : i + 1) : i - 1 < -1 ? n - 1 : i - 1));
    } else if (e.key === "Enter") {
      e.preventDefault();
      const item = items[active];
      if (shown && item) activate(item);
      else submit(value);
    } else if (e.key === "Escape" && shown) {
      // The first Esc closes the list; only the next leaves the field.
      e.stopPropagation();
      setOpen(false);
    }
  };

  const option = (i: number, item: Item, body: React.ReactNode) => (
    <div
      key={i}
      id={`${id}-opt-${i}`}
      role="option"
      aria-selected={i === active}
      data-active={i === active ? "" : undefined}
      className="search-option"
      onMouseDown={(e) => e.preventDefault()}
      onMouseMove={() => setActive(i)}
      onClick={() => activate(item)}
    >
      {body}
    </div>
  );

  const row = (i: number, item: Item) => {
    switch (item.kind) {
      case "recent":
        return option(
          i,
          item,
          <>
            <Clock size={14} className="flex-shrink-0 text-ink-faint" />
            <span className="min-w-0 flex-1 truncate">{item.query}</span>
            <button
              aria-label={`Forget ${item.query}`}
              className="search-forget flex-shrink-0 text-ink-faint hover:text-ink"
              onClick={(e) => {
                e.stopPropagation();
                forgetSearch(item.query);
                setRecent(recentSearches());
              }}
            >
              <X size={13} />
            </button>
          </>,
        );
      case "query": {
        // What was typed stays dim and the completion reads bright, so the eye lands on the new part.
        const lead = item.query.toLowerCase().startsWith(typed.toLowerCase()) ? typed.length : 0;
        return option(
          i,
          item,
          <>
            <Search size={14} className="flex-shrink-0 text-ink-faint" />
            <span className="min-w-0 flex-1 truncate">
              <span className="text-ink-dim">{item.query.slice(0, lead)}</span>
              <span className="font-semibold">{item.query.slice(lead)}</span>
            </span>
          </>,
        );
      }
      case "card": {
        const artist = item.card.kind === "Artist";
        return option(
          i,
          item,
          <>
            <Thumbnail srcs={[item.card.thumbnail]} className={`h-9 w-9 flex-shrink-0 ${artist ? "rounded-full" : "rounded-md"}`} />
            <span className="min-w-0 flex-1">
              <p className="truncate">{item.card.title}</p>
              <p className="truncate text-meta text-ink-dim">{artist ? "Artist" : item.card.subtitle}</p>
            </span>
          </>,
        );
      }
      case "song":
        return option(
          i,
          item,
          <>
            <Thumbnail srcs={[item.song.thumbnail]} className="h-9 w-9 flex-shrink-0 rounded-md" />
            <span className="min-w-0 flex-1">
              <p className="truncate">{item.song.title}</p>
              <p className="truncate text-meta text-ink-dim">
                {item.song.kind === "Song" ? "Song" : "Video"} · {item.song.artist}
              </p>
            </span>
          </>,
        );
    }
  };

  return (
    // The list is the field's sibling, not its child: a backdrop-filter parent would leave it only the field to blur.
    <div role="search" className="relative w-full max-w-sm min-w-0">
      <div className="flex items-center gap-2 rounded-full px-3.5 py-1.5 glass">
        <Search size={15} className="flex-shrink-0 text-ink-faint" />
        <input
          ref={field}
          id={id}
          type="search"
          value={value}
          onChange={(e) => {
            onChange(e.target.value);
            setOpen(true);
          }}
          onFocus={() => {
            setRecent(recentSearches());
            setOpen(true);
          }}
          onBlur={() => setOpen(false)}
          onKeyDown={onKeyDown}
          onContextMenu={nativeMenu}
          placeholder="Search YouTube Music (s)"
          aria-label="Search YouTube Music"
          role="combobox"
          aria-expanded={shown}
          aria-controls={`${id}-suggestions`}
          aria-activedescendant={active >= 0 ? `${id}-opt-${active}` : undefined}
          autoComplete="off"
          spellCheck={false}
          className="w-full bg-transparent text-body outline-none placeholder:text-ink-faint"
        />
      </div>
      {shown && (
        <div ref={list} id={`${id}-suggestions`} role="listbox" aria-label="Suggestions" className="search-pop">
          {!typed && (
            <div className="flex items-center justify-between px-3 pt-1.5 pb-1">
              <span className="text-label font-semibold text-ink-faint">Recent searches</span>
              <button
                onMouseDown={(e) => e.preventDefault()}
                onClick={() => {
                  clearSearches();
                  setRecent([]);
                }}
                className="text-label text-accent hover:text-accent-2"
              >
                Clear
              </button>
            </div>
          )}
          {items.map((item, i) => row(i, item))}
        </div>
      )}
    </div>
  );
}
