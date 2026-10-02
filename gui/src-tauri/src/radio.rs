//! Radio: "Start Radio" in the track menus, and `R`. When to ask for the next
//! page is `ytm_core::radio`'s rule, shared with the TUI; this holds the
//! station and does the fetching.

use tauri::{AppHandle, Emitter, State};
use ytm_core::radio::{self, Page};
use ytm_core::SearchResult;

use crate::state::AppState;

/// Replaces the queue with `(playlist, song)` and starts a station from it.
fn start(app: &AppHandle, state: &AppState, playlist: usize, song: usize) -> Result<(), String> {
    let title = {
        let library = state.library.lock().map_err(|e| e.to_string())?;
        let track = library.track(playlist, song).ok_or("no such track")?;
        let video_id = track.video_id.clone().ok_or("that track can't seed a radio")?;
        let title = track.title.clone().unwrap_or_default();
        state
            .player
            .lock()
            .map_err(|e| e.to_string())?
            .play_seed(&library, playlist, song);
        drop(library);
        *state.station.lock().map_err(|e| e.to_string())? = Some(radio::new_station(&video_id, &title));
        title
    };
    crate::player::push(app, state);
    let _ = app.emit("radio-notice", format!("Radio: finding songs like {title}…"));
    refill(app, state);
    Ok(())
}

/// Asks for the station's next page once the queue runs low, and drops the
/// station once something it didn't queue is playing. Called from the ticker
/// on every pass, since a song ending, a skip and a queue edit all move what
/// is left.
pub fn refill(app: &AppHandle, state: &AppState) {
    if state.station.lock().is_ok_and(|s| s.is_none()) {
        return;
    }
    // Library, player, then station: the workspace's order, station last.
    let (playing, remaining) = {
        let (Ok(library), Ok(player)) = (state.library.lock(), state.player.lock()) else {
            return;
        };
        let playing = player
            .playing()
            .and_then(|(pl, song)| library.track(pl, song))
            .and_then(|t| t.video_id.clone());
        (playing, player.remaining())
    };
    let Ok(mut slot) = state.station.lock() else { return };
    let Some(station) = slot.as_mut() else { return };
    if !radio::is_live(station, playing.as_deref()) {
        log::info!("radio: {} is no longer playing, station dropped", station.seed);
        *slot = None;
        return;
    }
    let Some(req) = radio::begin_refill(station, playing.as_deref(), remaining, std::time::Instant::now())
    else {
        return;
    };
    drop(slot);
    let Some(yt) = state.client.lock().ok().and_then(|c| c.clone()) else {
        return;
    };
    let (app, state) = (app.clone(), state.clone());
    // `block_on` in a blocking task rather than an `.await`, for the clippy
    // ICE `library::bootstrap` describes.
    tauri::async_runtime::spawn_blocking(move || {
        let rt_handle = tauri::async_runtime::handle().inner().clone();
        let result = rt_handle
            .block_on(radio::fetch(&yt, &req.seed, req.continuation.as_deref(), |id| {
                req.skip.contains(id)
            }))
            .map_err(|e| e.to_string());
        land(&app, &state, &req.seed, result);
    });
}

/// Queues a page that has landed. One for a station since replaced is
/// dropped by its seed.
fn land(app: &AppHandle, state: &AppState, seed: &str, result: Result<Page, String>) {
    let notice = {
        let (Ok(mut library), Ok(mut player)) = (state.library.lock(), state.player.lock()) else {
            return;
        };
        let Ok(mut slot) = state.station.lock() else { return };
        let Some(station) = slot.as_mut().filter(|s| s.seed == seed) else {
            return;
        };
        match result {
            Ok(page) => {
                let first = station.pages == 0;
                let tracks = radio::accept_page(station, page);
                let n = tracks.len();
                let refs = library.place_off_library(tracks);
                player.append_many(&library, &refs);
                let title = &station.seed_title;
                match (first, n) {
                    (true, 0) => Some(format!("No radio for {title}")),
                    (true, n) => Some(format!("Radio from {title}: {n} songs queued")),
                    _ => None,
                }
            }
            Err(e) => {
                log::warn!("radio: page for {seed} failed: {e}");
                radio::page_failed(station, std::time::Instant::now())
                    .then(|| "Radio stopped: YouTube isn't answering".to_string())
            }
        }
    };
    crate::player::push(app, state);
    if let Some(notice) = notice {
        let _ = app.emit("radio-notice", notice);
    }
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // tauri::command requires State by value
pub fn start_radio(app: AppHandle, state: State<'_, AppState>, playlist: usize, song: usize) -> Result<(), String> {
    start(&app, &state, playlist, song)
}

/// [`start_radio`] for a search hit, which has to be filed before it has a
/// `(playlist, song)` pair.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // tauri::command requires State by value
pub fn start_radio_from_search(
    app: AppHandle,
    state: State<'_, AppState>,
    result: SearchResult,
) -> Result<(), String> {
    let (playlist, song) = state
        .library
        .lock()
        .map_err(|e| e.to_string())?
        .place_search_result(result.to_track());
    start(&app, &state, playlist, song)
}
