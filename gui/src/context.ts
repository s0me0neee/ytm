import { createContext, useContext } from "react";
import type { MenuEntry } from "./components/Menu";
import type { PlaylistView, QueueEntry, Route, Target, Track } from "./types";

/** Extra rows a list adds to a track's menu, where the action exists. */
export interface TrackMenuExtras {
  removeFromPlaylist?: () => void;
  queueEntry?: QueueEntry;
}

export interface Menus {
  track: (track: Track, target: Target, extras?: TrackMenuExtras) => MenuEntry[];
  playlist: (index: number) => MenuEntry[];
  album: (browseId: string) => MenuEntry[];
  artist: (channelId: string) => MenuEntry[];
}

/** What the app can do. Never changes, so reading it never re-renders a row. */
export interface AppActions {
  navigate: (route: Route) => void;
  toggleLike: (track: Track) => void;
  menus: Menus;
  prefetch: (target: Target) => void;
}

/** What the app is showing. Changes on real events only, never on the playback tick. */
export interface AppState {
  playlists: PlaylistView[];
  liked: Set<string>;
  currentTrackId: string | null;
  paused: boolean;
  route: Route;
}

export const ActionsContext = createContext<AppActions | null>(null);
export const StateContext = createContext<AppState | null>(null);

export function useActions(): AppActions {
  const v = useContext(ActionsContext);
  if (!v) throw new Error("useActions outside ActionsContext");
  return v;
}

export function useAppState(): AppState {
  const v = useContext(StateContext);
  if (!v) throw new Error("useAppState outside StateContext");
  return v;
}
