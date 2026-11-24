// tauri::command's macro expansion emits an unreachable!() on async fns, which the workspace lints deny by default.
#![allow(clippy::unreachable)]
use serde::Serialize;
use tauri::State;
use ytm_core::search::{self as core_search, SearchPage, Suggestions};
use ytm_core::{Track, YTMusicClient};

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
pub async fn search(state: State<'_, AppState>, query: String) -> Result<SearchPage, String> {
    crate::state::off_main(&state, move |state| {
        let yt = client(state)?;
        let rt_handle = tauri::async_runtime::handle().inner().clone();
        rt_handle.block_on(core_search::search_page(&yt, &query)).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // tauri::command requires State by value
pub async fn search_suggestions(state: State<'_, AppState>, input: String) -> Result<Suggestions, String> {
    crate::state::off_main(&state, move |state| {
        let yt = client(state)?;
        let rt_handle = tauri::async_runtime::handle().inner().clone();
        rt_handle.block_on(core_search::suggestions(&yt, &input)).map_err(|e| e.to_string())
    })
    .await
}

/// A library track a query matched, addressed the way every library row is.
#[derive(Serialize)]
pub struct LibraryHit {
    playlist: usize,
    song: usize,
    track: Track,
}

// Enough to answer "do I already have it"; the rest is what the playlists are for.
const MAX_LIBRARY_HITS: usize = 24;

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // tauri::command requires State by value
pub fn search_library(state: State<'_, AppState>, query: String) -> Vec<LibraryHit> {
    let Ok(lib) = state.library.lock() else { return Vec::new() };
    lib.search(&query, MAX_LIBRARY_HITS)
        .into_iter()
        .filter_map(|(playlist, song)| Some(LibraryHit { playlist, song, track: lib.track(playlist, song)?.clone() }))
        .collect()
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
