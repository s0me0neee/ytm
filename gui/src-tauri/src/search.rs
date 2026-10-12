// tauri::command's macro expansion emits an unreachable!() on async fns, which the workspace lints deny by default.
#![allow(clippy::unreachable)]
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
pub async fn search(state: State<'_, AppState>, query: String) -> Result<Vec<SearchResult>, String> {
    crate::state::off_main(&state, move |state| {
        let yt = client(state)?;
        let rt_handle = tauri::async_runtime::handle().inner().clone();
        rt_handle.block_on(core_search::search(&yt, &query)).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // tauri::command requires State by value
pub async fn add_to_playlist(state: State<'_, AppState>, playlist_id: String, video_id: String) -> Result<(), String> {
    crate::state::off_main(&state, move |state| {
        let yt = client(state)?;
        let rt_handle = tauri::async_runtime::handle().inner().clone();
        rt_handle
            .block_on(core_search::add_to_playlist(&yt, &playlist_id, &video_id))
            .map_err(|e| e.to_string())?;

        // Refetch so the track is playable this session; `refresh_after_edit` carries the queue across it.
        let index = state
            .library
            .lock()
            .map_err(|e| e.to_string())?
            .find_playlist_index(&playlist_id);
        if let Some(index) = index {
            crate::library::refresh_after_edit(state, index)?;
        }
        Ok(())
    })
    .await
}
