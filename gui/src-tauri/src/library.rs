use std::sync::Arc;
use std::sync::atomic::Ordering;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use ytm_core::{Track, library, session};

use crate::state::AppState;

/// Clears `AppState::bootstrapping` on every exit path out of `bootstrap`,
/// success or early-return `?` failure alike.
struct BootstrapGuard<'a>(&'a std::sync::atomic::AtomicBool);

impl Drop for BootstrapGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

#[derive(Serialize)]
pub struct PlaylistView {
    pub playlist_id: String,
    pub title: String,
    pub count: Option<u32>,
    pub loaded: bool,
    pub failed: bool,
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // tauri::command requires State by value
pub fn get_playlists(state: State<'_, AppState>) -> Vec<PlaylistView> {
    let Ok(lib) = state.library.lock() else {
        return Vec::new();
    };
    lib.entries()
        .iter()
        .map(|e| PlaylistView {
            playlist_id: e.playlist.playlist_id.clone(),
            title: e.playlist.title.clone(),
            count: e.playlist.count,
            loaded: e.loaded,
            failed: e.failed,
        })
        .collect()
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // tauri::command requires State by value
pub fn get_songs(state: State<'_, AppState>, index: usize) -> Vec<Track> {
    state.library.lock().map_or_else(|_| Vec::new(), |lib| lib.songs(index).to_vec())
}

/// Keeps the queue and the playing track meaning the same songs across a
/// refetch that replaced `pl`'s tracks.
///
/// The mirror of `tui/src/app.rs`'s own `follow_tracks`, over the same shared
/// `library::moved_indices`. `None` from it means nothing moved -- which is
/// what appending to an ordinary playlist does -- and then nothing is touched.
fn follow_tracks(state: &AppState, pl: usize, pending: &crate::state::PendingRefresh) {
    if pending.before.is_empty() {
        return;
    }
    // Library before player, matching the order every command and the ticker
    // take -- the opposite order here would be a lock-order inversion.
    let Ok(mut lib) = state.library.lock() else { return };
    let Some(moved) = library::moved_indices(&pending.before, lib.songs(pl)) else {
        return;
    };
    let Ok(mut player) = state.player.lock() else { return };

    let playing_ref = player.playing().filter(|(p, _)| *p == pl);
    player.remap_refs(|(p, song)| {
        if p != pl {
            return Some((p, song));
        }
        if let Some(new) = moved.get(song).copied().flatten() {
            return Some((pl, new));
        }
        // Gone from the playlist altogether. A queue entry goes with it, but
        // the track that is *playing* is still audibly playing, so it is filed
        // where tracks played from search live rather than lost.
        if Some((p, song)) == playing_ref {
            pending.playing.clone().map(|t| lib.place_search_result(t))
        } else {
            None
        }
    });
}

/// Refetches `pl` after an edit, recording what it held first so the queue can
/// be carried across a reorder. Called by `add_to_playlist`.
pub fn refresh_after_edit(state: &AppState, pl: usize) -> Result<(), String> {
    // The two locks are taken one at a time rather than nested. Nesting would
    // have to be library-then-player to match every other caller, and holding
    // the library across the refetch request is exactly the contention that
    // ordering exists to bound -- taking neither while holding the other
    // sidesteps the question entirely.
    let playing_ref = state
        .player
        .lock()
        .map_err(|e| e.to_string())?
        .playing()
        .filter(|(p, _)| *p == pl);

    let lib = state.library.lock().map_err(|e| e.to_string())?;
    if lib.is_search_playlist(pl) {
        return Ok(());
    }
    let Some(playlist_id) = lib.playlist(pl).map(|p| p.playlist_id.clone()) else {
        return Ok(());
    };
    let playing = match playing_ref {
        Some((p, s)) => lib.track(p, s).cloned(),
        None => None,
    };
    let before: Vec<Option<String>> = lib.songs(pl).iter().map(|t| t.video_id.clone()).collect();
    drop(lib);

    state
        .pending_refresh
        .lock()
        .map_err(|e| e.to_string())?
        .insert(pl, crate::state::PendingRefresh { before, playing });

    state
        .fetcher
        .lock()
        .map_err(|e| e.to_string())?
        .as_ref()
        .ok_or_else(|| "library not fetched yet".to_string())?
        .fetch(pl, &playlist_id);
    Ok(())
}

/// Asks again for a playlist whose fetch gave up -- the GUI's `r`.
///
/// `mark_retrying` clears the failed flag so the sidebar shows it loading
/// again; the answer arrives on the same `library-song-batch` channel every
/// other fetch uses, so nothing else has to know this happened.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // tauri::command requires State by value
pub fn refetch_playlist(state: State<'_, AppState>, index: usize) -> Result<(), String> {
    let playlist_id = {
        let mut lib = state.library.lock().map_err(|e| e.to_string())?;
        let id = lib
            .playlist(index)
            .ok_or_else(|| "no such playlist".to_string())?
            .playlist_id
            .clone();
        lib.mark_retrying(index);
        id
    };

    state
        .fetcher
        .lock()
        .map_err(|e| e.to_string())?
        .as_ref()
        .ok_or_else(|| "library not fetched yet".to_string())?
        .fetch(index, &playlist_id);
    Ok(())
}

/// Builds an authenticated client, kicks off a background cookie refresh
/// (mirroring `tui/src/main.rs`'s once-per-start behaviour), fetches
/// playlists, and streams each playlist's songs in as they arrive.
///
/// Callable both from the app's `setup()` hook (already signed in) and from
/// `sign_in` (just finished setup), so both paths reach a populated library
/// the same way. Plain sync fn driving `get_playlists` via `block_on` rather
/// than `async fn` + `.await`: this nightly's clippy hits an internal
/// compiler error over `layout_of` an opaque alias whenever one async fn's
/// opaque Future type is awaited from inside another's, and `get_playlists`
/// is exactly that. `block_on` sidesteps it entirely -- same pattern
/// `tui/src/main.rs` already uses from its own sync `start()`. Callers must
/// run this via `spawn_blocking`, never inline on an async-command's own
/// worker thread, since `block_on` here would otherwise nest inside the
/// runtime it's blocking on.
pub fn bootstrap(app: &AppHandle, state: &AppState) -> Result<(), String> {
    if state.bootstrapping.swap(true, Ordering::SeqCst) {
        return Err("sign-in already in progress".to_string());
    }
    let _guard = BootstrapGuard(&state.bootstrapping);

    let mut yt = state.session.build_client().map_err(|e| e.to_string())?;
    let rt_handle = tauri::async_runtime::handle().inner().clone();
    let mut playlists = rt_handle
        .block_on(library::get_playlists(&yt))
        .map_err(|e| e.to_string())?;

    // An empty library on the very first fetch usually means the session has
    // quietly expired: get_library_playlists answers that with an empty list
    // rather than an auth error, so nothing above would have caught it.
    // Mirrors tui/src/main.rs's own once-per-start auto-reauth.
    if playlists.is_empty()
        && let Some(browser) = session::configured_browser().filter(|_| session::can_auto_reauth())
    {
        eprintln!("[gui] bootstrap: no playlists — renewing from {browser} and retrying");
        if let Err(e) = state.session.setup_with_browser(browser) {
            eprintln!("[gui] bootstrap: renewal from {browser} failed: {e}");
        } else {
            yt = state.session.build_client().map_err(|e| e.to_string())?;
            playlists = rt_handle
                .block_on(library::get_playlists(&yt))
                .map_err(|e| e.to_string())?;
        }
    }

    {
        let session = state.session.clone();
        std::thread::spawn(move || {
            if let Err(e) = session.refresh_cookies() {
                eprintln!("[gui] cookie refresh failed (using cached): {e}");
            }
        });
    }

    let yt = Arc::new(yt);
    *state.client.lock().map_err(|e| e.to_string())? = Some(Arc::clone(&yt));

    let (fetcher, songs_rx) = library::LibraryFetcher::new(&rt_handle, Arc::clone(&yt), &playlists);
    // Kept rather than dropped: `refetch_playlist` below needs it to ask again
    // for a playlist whose fetch gave up.
    *state.fetcher.lock().map_err(|e| e.to_string())? = Some(fetcher);

    *state.library.lock().map_err(|e| e.to_string())? = ytm_core::Library::new(playlists);

    let batch_state = state.clone();
    let app_handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        while let Ok((idx, songs)) = songs_rx.recv() {
            if let Ok(mut lib) = batch_state.library.lock() {
                lib.apply_song_batch(idx, songs);
            }
            // A refetch asked for by `add_to_playlist` lands here like any
            // other batch, so this is the only place the queue can be carried
            // across it.
            let pending = batch_state
                .pending_refresh
                .lock()
                .ok()
                .and_then(|mut p| p.remove(&idx));
            if let Some(pending) = pending {
                follow_tracks(&batch_state, idx, &pending);
            }
            // The only thing that can advance a saved queue towards resolving:
            // its entries name playlists, and this is where a playlist arrives.
            crate::persist::try_restore_queue(&app_handle, &batch_state);
            let _ = app_handle.emit("library-song-batch", idx);
        }
    });

    let _ = app.emit("library-loaded", ());
    Ok(())
}
