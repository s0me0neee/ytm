use tauri::State;
use ytm_core::search as core_search;
use ytm_core::{SearchResult, YTMusicClient};

use crate::state::AppState;

fn client(state: &AppState) -> Result<std::sync::Arc<YTMusicClient>, String> {
    state
        .client
        .lock()
        .map_err(|e| e.to_string())?
        .clone()
        .ok_or_else(|| "not signed in yet".to_string())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // tauri::command requires State by value
pub fn search(state: State<'_, AppState>, query: String) -> Result<Vec<SearchResult>, String> {
    let yt = client(&state)?;
    let rt_handle = tauri::async_runtime::handle().inner().clone();
    rt_handle.block_on(core_search::search(&yt, &query)).map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // tauri::command requires State by value
pub fn add_to_playlist(state: State<'_, AppState>, playlist_id: String, video_id: String) -> Result<(), String> {
    let yt = client(&state)?;
    let rt_handle = tauri::async_runtime::handle().inner().clone();
    rt_handle
        .block_on(core_search::add_to_playlist(&yt, &playlist_id, &video_id))
        .map_err(|e| e.to_string())?;

    // Refetch, so the track is playable in the session that added it rather
    // than at the next start. The refetch is also what makes the queue-remap
    // in `follow_tracks` necessary -- see `refresh_after_edit`.
    let index = state
        .library
        .lock()
        .map_err(|e| e.to_string())?
        .find_playlist_index(&playlist_id);
    if let Some(index) = index {
        crate::library::refresh_after_edit(&state, index)?;
    }
    Ok(())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // tauri::command requires State by value
pub fn like_track(state: State<'_, AppState>, video_id: String) -> Result<(), String> {
    let yt = client(&state)?;
    let rt_handle = tauri::async_runtime::handle().inner().clone();
    rt_handle.block_on(core_search::like(&yt, &video_id)).map_err(|e| e.to_string())
}

/// Queues `result` without playing it -- "Play Next" (`next`) or "Play Last".
///
/// A search hit has no `(playlist, song)` pair until it is filed, and the
/// queue holds nothing else, so `place_search_result` runs first here exactly
/// as it does in `play_search_result`. Filing is idempotent, so queueing the
/// same hit twice lands on the same pair rather than growing the synthetic
/// playlist.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // tauri::command requires State by value
pub fn queue_search_result(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    result: SearchResult,
    next: bool,
) -> Result<(), String> {
    let (pl, song) = state
        .library
        .lock()
        .map_err(|e| e.to_string())?
        .place_search_result(result.to_track());

    // Scoped so both guards are released at the block's end rather than being
    // held across the return -- library before player, the workspace's order.
    {
        let library = state.library.lock().map_err(|e| e.to_string())?;
        let mut player = state.player.lock().map_err(|e| e.to_string())?;
        if next {
            player.insert_next(&library, pl, song);
        } else {
            player.append_to_queue(&library, pl, song);
        }
    }
    crate::player::push(&app, &state);
    Ok(())
}

/// Files `result` under the library's synthetic search playlist and plays it
/// immediately, the same path a library track takes.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // tauri::command requires State by value
pub fn play_search_result(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    result: SearchResult,
) -> Result<(), String> {
    let track = result.to_track();
    let (pl, song) = state
        .library
        .lock()
        .map_err(|e| e.to_string())?
        .place_search_result(track);
    {
        let library = state.library.lock().map_err(|e| e.to_string())?;
        state.player.lock().map_err(|e| e.to_string())?.play(&library, pl, song);
    }
    crate::player::push(&app, &state);
    Ok(())
}
