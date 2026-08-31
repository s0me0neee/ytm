---
# yt-music-tui — feature plan
Last updated: 2026-08-29

Legend: ✅ done  🔄 in progress  ❌ not started

---

## Completed

- ✅ libmpv2 embedding — no longer spawning the mpv binary, lower latency, cleaner cleanup
- ✅ Persistent queue — queue.json written on exit, restored on startup (without auto-play)
- ✅ Prefetch / hot CDN URL — j/k navigation fires `Cmd::Prefetch`; concurrent resolution capped at 2
- ✅ Cookie-refresh gating — yt-dlp cookie extraction skipped when cookies are fresh enough
- ✅ Mouse support — scroll wheel maps to j/k, click selects panels
- ✅ Config dir — `~/.config/yt-music-tui/` with `config.toml` stub, `queue.json`, `browser.json`
- ✅ Cross-playlist queue — queue entries track (playlist_idx, song_idx) pairs
- ✅ Hotpath profiling — `#[hotpath::measure]` on `resolve_url`, gated behind `--features hotpath`

---

## In progress

~~**rustypipe URL resolution**~~ — dropped; `rustypipe` never made it into `Cargo.toml` and
`resolve_url` (now `ytm-core/src/playback.rs`, not `src/audio.rs` — the crate split happened
since this was written) still resolves through `yt-dlp --get-url`. Current work on that path
is perf investigation of the yt-dlp subprocess itself, not replacing it.

---

## Tauri GUI

✅ **A full second frontend** (`gui/` + `gui/src-tauri`, package `ytm-gui`), alongside the TUI
rather than replacing it — sign-in, library, search, full playback, lyrics, all working against
the same `ytm-core` engine `tui/` already drives.

Auth took a different path than first planned. An embedded-webview Google login (the original
design below this section used to describe) was blocked outright by Google's own anti-phishing
policy — "This browser or app may not be secure" — confirmed by actually trying it, not a bug to
route around. The GUI instead uses the **`rookie` crate**, which reads cookies straight out of an
already-installed browser's own disk storage (Chrome, Firefox, Edge, Brave, Opera, Chromium,
Vivaldi, and Safari on macOS only) — no subprocess, no yt-dlp, no embedded login page. This
replaced yt-dlp's `--cookies-from-browser` for **both** the TUI and the GUI, for **both** initial
sign-in and the silent ~6h refresh (`Session::REFRESH_AFTER`) — see `ytm-core/src/session.rs`.
yt-dlp remains a hard runtime dependency for stream-URL resolution in `playback.rs`; this change
removed it from the auth path only.

- ✅ `Session::setup_with_browser` / `refresh_cookies` in `ytm-core/src/session.rs` — cookie
  extraction and the shared staleness/write-back logic, used by both frontends. `browser.json`'s
  shape is unchanged, so a session started in one frontend is one the other can already read.
- ✅ `gui/src-tauri` commands: `auth_status`/`list_browsers`/`sign_in`, `get_playlists`/
  `get_songs`, `play`/`play_pause`/`next`/`prev`/`seek`/`seek_to`/`set_volume`/`toggle_mute`/
  `cycle_mode`/`append_to_queue`/`remove_from_queue`/`jump_to`, `search`/`add_to_playlist`/
  `like_track`/`play_search_result`, `get_lyrics`.
- ✅ Auto-reauth on empty playlists — an expired session answers `get_library_playlists` with an
  empty list rather than an auth error, so `bootstrap()` mirrors the TUI's own once-per-start
  heuristic (`session::configured_browser`/`can_auto_reauth`) rather than showing an empty
  library.
- ✅ Frontend: React + TypeScript + Vite, Tailwind CSS v4, Radix UI (`react-slider`, for a
  scrubber/volume slider whose thumb and fill can't drift apart the way a hand-styled native
  `<input type="range">` did), `lucide-react` icons, `motion` (framer-motion) for transitions —
  sign-in → sidebar (playlists, search) → track list with cover art → persistent player bar →
  a dedicated Now Playing view (blurred/scaled cover background, synced lyrics) for a focused
  one-song view, matching the TUI's own lyrics-mode intent.
- ❌ Not in this pass, matching the TUI's own tiering below: radio/up-next, album drill-down,
  history/albums tabs, crossfade, playback speed, offline download cache, lyric translation UI.
- ❌ Packaging not solved yet: `flake.nix`'s devShell needs Linux's Tauri deps (webkit2gtk-4.1,
  librsvg, …) added under `stdenv.isLinux`; `dist-workspace.toml` (cargo-dist) only builds the
  TUI's shell/npm installers, not a Tauri `.app`/`.dmg`/`.msi`/`.AppImage`; libmpv bundling for a
  packaged GUI build is unsolved on every platform (`ytm-core` links it rather than spawning it).

---

## Tier 1 — Core usability

✅ **Search** (`s` key)
- Songs and videos fetched as two filtered requests (`ytm-core/src/search.rs`), not
  `ytmusicapi`'s own unfiltered search — see the "why" in CLAUDE.md's `search.rs` section
- `↵` plays, `a` adds to a playlist (refetches it so the addition is playable this session),
  `/` edits the query
- Hits are filed into a synthetic `__search__` playlist so the queue/player/lyrics machinery
  can address a search result exactly like any other track

❌ **Like / unlike current song** (`L` key)
- `rate_song(video_id, "LIKE" | "INDIFFERENT")` is in the ytmusicapi crate
- Show a heart indicator in the player bar next to the title
- Need to track `liked: bool` on the currently playing song

---

## Tier 2 — Library navigation

❌ **Library views** (keys `1`/`2`/`3`/`4` or a tab bar)
- Tab 1: Playlists (current default)
- Tab 2: Liked Songs — `get_liked_songs()`
- Tab 3: History — `get_history()`
- Tab 4: Albums — `get_library_albums()`
- Add a `LibraryTab` enum; render a tab header row at the top of the playlist panel

❌ **Album drill-down**
- Press `Enter` on an album entry to load tracks via `get_album(browse_id)`
- Push a new songs view; `Backspace` pops back to the album list

❌ **Radio / "Up next"** (`r` key)
- `get_watch_playlist(video_id=current)` returns a "Up next" list
- Append results to the queue automatically
- Show "Radio seeded from <title>" in the notification bar

---

## Tier 3 — Polish

✅ **Lyrics panel** (`y` key)
- Sourced from **lrclib.net**, not ytmusicapi: `get_lyrics(browse_id)` does not exist in
  ytmusicapi 0.4.2. The `lrclib` crate was vendored into the workspace as a member.
- `y` replaces the right column with synced lyrics that auto-centre the active line, driven by
  `AudioState::elapsed`; falls back to unsynced when no synced record exists
- `c` opens a modal to pick a different lrclib record; the choice persists in `lyrics.json`
- Scrollable with `j`/`k`; cached per video_id so toggling never re-fetches

✅ **Config file** (`~/.config/yt-music-tui/config.toml`)
- Read once at startup by `ytm-core/src/config.rs`; a syntax error falls back to defaults
  whole, an unreadable individual field falls back to just that field, unknown keys are
  logged by name — see CLAUDE.md's `config.rs` section for the full forgiving-parse design
- Covers `lyrics.offset`/`ai-translation`/`ai-model`/`ai-key-env`/`translate-to`, `ui.covers`,
  `auth.auto-reauth`/`cookie-browser` — no `keybindings` map or `default_volume` (volume is
  persisted separately in `settings.json`, not `config.toml`)

❌ **Session expiry warning**
- On startup, parse the `expires` fields in `browser.json` cookies
- If any cookie expires within 7 days, show a warning in the help bar: "session expires in N days"

---

## Tier 4 — Playback power features

❌ **Speed control** (`[` / `]` keys)
- Adjust `speed` property on the libmpv2 handle: 0.5× → 0.75× → 1.0× → 1.25× → 1.5× → 2.0×
- Display current speed in the player bar (only when ≠ 1.0× to avoid clutter)
- Persist speed setting across sessions in `config.toml`

❌ **Local playback history** (automatic, no key)
- On every `do_play()`, append `{video_id, title, artist, timestamp}` to
  `~/.local/share/yt-music-tui/history.json`
- Cap at 1000 entries; deduplicate by recency (most-recent occurrence wins)
- Show as Library tab 3 or 4; no API call needed, instant load

❌ **Download / offline cache** (`D` key)
- Run `yt-dlp -x --audio-format opus -o ~/.cache/yt-music-tui/<id>.opus`
- Check cache in `resolve_url()` before calling yt-dlp or rustypipe
- Show ↓ indicator next to cached songs in the list
- Progress shown in the notification bar during download (background thread)

❌ **Crossfade** (`C` key cycles: off → 2s → 5s)
- Pre-load next song in a second libmpv2 instance; fade volume of the first out
- Store crossfade duration in `config.toml`
- Complex: requires careful state management for two mpv handles; implement after speed control

---

## Tier 5 — Auth improvements

~~**Chrome CDP auto-auth**~~ — superseded by the Tauri webview plan below (same
idea — drive a real Google login and lift cookies out of it — but a bundled
webview needs no external Chrome, no debug port, no WebSocket protocol client.

~~**Upstream OAuth**~~ — dead end, confirmed: YouTube Music rejects Bearer
tokens from user-created OAuth clients
([sigma67/ytmusicapi#813](https://github.com/sigma67/ytmusicapi/issues/813)),
and the `ytmusicapi-rs` sibling project's README documents hitting this
directly. Not worth wiring up `yup_oauth2` against — there is no token this
flow can produce that the API will accept. Cookie auth stays the only real
path; the plan below changes *how* the cookie is obtained, not what it is.

~~**Tauri frontend + webview login**~~ — the embedded-webview login this entry used to describe
was tried and blocked outright by Google's own anti-phishing policy ("this browser or app may
not be secure"), not a bug to route around. Superseded by `rookie`-based cookie reading, covering
both frontends and both initial login and silent refresh — see the **Tauri GUI** section near the
top of this file for what actually shipped.

---

## Backlog / nice-to-have

- Playlist management from the TUI (create playlist, add/remove songs)
- macOS/Linux native notifications on track change (`notify-rust` crate)
- Visualizer bar in the player area (requires PCM data from mpv's audio output)
- Vim-style `gg`/`G` jump-to-top/bottom in any list
