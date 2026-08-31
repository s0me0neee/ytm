use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use ytm_core::library::LibraryFetcher;
use ytm_core::persistence::{History, LyricsOverrides, QueueState, Translations};
use ytm_core::{Config, Library, Player, Session, Track, YTMusicClient};

/// A playlist being refetched after an edit, and what is needed to carry the
/// queue across the result.
///
/// A `TrackRef` is a *position*, so a refetch that comes back reordered --
/// which is what adding to Liked Music does, since a like lands at the top --
/// silently changes what every queue entry means. `before` is the video ids
/// the playlist held when those positions were taken, and `playing` is the
/// track itself, kept for the one case a number can't cover: a track that has
/// left the playlist entirely but is still audibly playing.
pub struct PendingRefresh {
    pub before: Vec<Option<String>>,
    pub playing: Option<Track>,
}

#[derive(Clone)]
pub struct AppState {
    pub session: Session,
    pub library: Arc<Mutex<Library>>,
    pub player: Arc<Mutex<Player>>,
    pub client: Arc<Mutex<Option<Arc<YTMusicClient>>>>,
    /// Kept alive past `bootstrap` so a playlist whose fetch gave up can be
    /// asked for again -- `LibraryFetcher::fetch` is the only way to do that,
    /// and dropping the fetcher (as this used to) makes retry impossible.
    pub fetcher: Arc<Mutex<Option<LibraryFetcher>>>,
    /// `config.toml`, read once at startup exactly as the TUI reads it, so the
    /// two frontends honour the same hand-edited settings.
    pub config: Arc<Config>,
    /// Manual lyric choices, keyed by video id. Mirrored to `lyrics.json` on
    /// every change so a choice survives a restart.
    pub lyrics_overrides: Arc<Mutex<LyricsOverrides>>,
    /// AI translations, keyed by lrclib record id, so one is paid for once.
    pub translations: Arc<Mutex<Translations>>,
    /// Playlists whose refetch is in flight after an edit, keyed by index.
    /// Consumed by the song-batch loop, which is where the answer arrives.
    pub pending_refresh: Arc<Mutex<HashMap<usize, PendingRefresh>>>,
    /// The last `playback-state` the frontend was told, so the ticker and the
    /// commands can share one "emit only on a change" decision.
    ///
    /// It has to be shared or the two disagree: a command that emits directly
    /// leaves the ticker's private copy stale, and the ticker then re-sends
    /// the same payload on its next pass -- a wasted IPC round trip and a
    /// wasted React render for every button press.
    pub last_emitted: Arc<Mutex<Option<crate::player::PlaybackStateView>>>,
    /// The saved queue, until the playlists it names have loaded and it can be
    /// resolved to positions. `None` once that has happened, or once it has
    /// been given up on. See `persist::try_restore_queue`.
    pub pending_queue_restore: Arc<Mutex<Option<QueueState>>>,
    /// What the OS's media panel was last told, and when it was told — the
    /// outbound half of the same "emit only on a change" decision
    /// `last_emitted` makes for the frontend.
    ///
    /// The instant is the last *dispatch*, not the last comparison: the
    /// snapshot is stored on every pass so a seek is measured against where
    /// playback actually was, while the clock behind [`Instant`] only moves
    /// when something was really sent.
    pub last_published: Arc<Mutex<Option<(ytm_core::NowPlaying, std::time::Instant)>>>,
    /// Recently played tracks and playlists -- the home page. Mirrored to
    /// `history.json` on every change, since a song is three minutes apart and
    /// a history a crash can erase is one nobody trusts.
    pub history: Arc<Mutex<History>>,
    /// The video id the history has already been told about, so the ticker
    /// passing through four times a second records one play rather than a
    /// thousand. See `history::observe`.
    pub last_noted: Arc<Mutex<Option<String>>>,
    /// Guards `library::bootstrap` against running twice concurrently -- e.g. a
    /// fast double-click on "Sign in" firing two `sign_in` commands before the
    /// first disables the button. Two concurrent bootstraps would each spawn
    /// their own library fetch and race to overwrite `library`/`client`.
    pub bootstrapping: Arc<AtomicBool>,
}
