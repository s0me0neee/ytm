import { memo, useEffect, useMemo, useState } from "react";
import { ListPlus, Plus, Search } from "lucide-react";
import { call } from "../api";
import { artistNames } from "../format";
import { LIKED_PLAYLIST_ID, SEARCH_PLAYLIST_ID, type PlaylistView, type Track } from "../types";
import { Modal } from "./Modal";
import { nativeMenu } from "./Menu";
import { Thumbnail } from "./Thumbnail";

export interface AddRequest {
  track: Track | null;
  /** Straight to naming a new playlist, as "New Playlist…" in a menu asks. */
  create: boolean;
}

interface Props {
  request: AddRequest | null;
  playlists: PlaylistView[];
  onClose: () => void;
  onCreated: (index: number) => void;
}

/** `P`'s picker, and the form behind "New Playlist…". */
export const AddToPlaylistDialog = memo(function AddToPlaylistDialog({ request, playlists, onClose, onCreated }: Props) {
  const [filter, setFilter] = useState("");
  const [creating, setCreating] = useState(false);
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const track = request?.track ?? null;
  const videoId = track?.video_id ?? null;

  useEffect(() => {
    setFilter("");
    setName("");
    setBusy(false);
    setCreating(Boolean(request?.create));
  }, [request]);

  const own = useMemo(() => {
    const q = filter.trim().toLowerCase();
    return playlists
      .map((p, i) => ({ p, i }))
      .filter(({ p }) => p.playlist_id !== LIKED_PLAYLIST_ID && p.playlist_id !== SEARCH_PLAYLIST_ID)
      .filter(({ p }) => !q || p.title.toLowerCase().includes(q));
  }, [playlists, filter]);

  async function add(p: PlaylistView) {
    if (!videoId) return;
    onClose();
    await call("add_to_playlist", { playlistId: p.playlist_id, videoId }, `Added to ${p.title}`);
  }

  async function create() {
    const title = name.trim();
    if (!title || busy) return;
    setBusy(true);
    const index = await call<number>(
      "create_playlist",
      { title, videoId },
      videoId ? `Created ${title} with ${track?.title ?? "the song"}` : `Created ${title}`,
    );
    setBusy(false);
    if (index !== undefined) {
      onClose();
      onCreated(index);
    }
  }

  return (
    <Modal
      open={request !== null}
      onClose={onClose}
      title={creating ? "New Playlist" : "Add to Playlist"}
      icon={<ListPlus size={16} className="text-ink-dim" />}
    >
      {track && (
        <div className="mx-5 mb-3 flex items-center gap-3 rounded-xl bg-surface p-2">
          <Thumbnail srcs={[track.thumbnail]} className="h-10 w-10 flex-shrink-0 rounded-md" />
          <span className="min-w-0">
            <p className="truncate text-body text-ink">{track.title ?? "Untitled"}</p>
            <p className="truncate text-meta text-ink-dim">{artistNames(track)}</p>
          </span>
        </div>
      )}

      {creating ? (
        <form
          className="flex flex-col gap-3 px-5 pb-5"
          onSubmit={(e) => {
            e.preventDefault();
            void create();
          }}
        >
          <input
            autoFocus
            value={name}
            onChange={(e) => setName(e.target.value)}
            onContextMenu={nativeMenu}
            placeholder="Playlist name"
            maxLength={150}
            className="field"
          />
          <p className="text-meta text-ink-faint">New playlists are private. You can share them from YouTube Music.</p>
          <div className="flex justify-end gap-2">
            <button type="button" className="btn btn-secondary" onClick={() => (request?.create ? onClose() : setCreating(false))}>
              {request?.create ? "Cancel" : "Back"}
            </button>
            <button type="submit" className="btn btn-primary" disabled={!name.trim() || busy}>
              {busy ? "Creating…" : "Create"}
            </button>
          </div>
        </form>
      ) : (
        <>
          <div className="mx-5 mb-2 flex items-center gap-2 rounded-full bg-surface px-3 py-1.5">
            <Search size={13} className="text-ink-faint" />
            <input
              autoFocus
              value={filter}
              onChange={(e) => setFilter(e.target.value)}
              onContextMenu={nativeMenu}
              placeholder="Find a playlist"
              className="w-full bg-transparent text-body outline-none placeholder:text-ink-faint"
            />
          </div>
          <div className="thin-scrollbar min-h-0 flex-1 overflow-y-auto px-3 pb-3">
            <button onClick={() => setCreating(true)} className="list-button">
              <span className="flex h-8 w-8 items-center justify-center rounded-md bg-surface-2">
                <Plus size={15} />
              </span>
              New Playlist…
            </button>
            {own.map(({ p }) => (
              <button key={p.playlist_id} onClick={() => void add(p)} disabled={!videoId} className="list-button">
                <Thumbnail srcs={[p.thumbnail]} className="h-8 w-8 flex-shrink-0 rounded-md" />
                <span className="min-w-0 flex-1 truncate">{p.title}</span>
                {p.count !== null && <span className="font-mono text-label text-ink-ghost">{p.count}</span>}
              </button>
            ))}
            {own.length === 0 && filter && <p className="px-3 py-4 text-center text-body text-ink-faint">No playlist matches “{filter}”.</p>}
          </div>
        </>
      )}
    </Modal>
  );
});
