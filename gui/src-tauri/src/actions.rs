//! What the right-click menus do. Every one takes a [`Target`], so a row from a
//! playlist, a search, an album page or the history all reach the same code.

// tauri::command's macro expansion emits an unreachable!() on async fns, which the workspace lints deny by default.
#![allow(clippy::unreachable)]
#![allow(clippy::needless_pass_by_value)] // tauri::command requires State by value

use std::sync::Arc;

use serde::Deserialize;
use tauri::{AppHandle, State};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_opener::OpenerExt;
use ytm_core::browse::{self, AlbumPage, ArtistPage};
use ytm_core::search as core_search;
use ytm_core::{Library, Player, Playlist, Track, TrackRef, YTMusicClient, persistence};

use crate::state::{AppState, off_main};

/// Liked Music's playlist id, which is also the like button.
const LIKED: &str = "LM";

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Target {
    /// A row of one of the user's playlists, by position.
    Library { playlist: usize, song: usize },
    /// A track from anywhere else: search, an album or artist page.
    Track { track: Track },
    /// A row of the home page's history, played from its playlist when it is still there.
    History { index: usize },
}

fn client(state: &AppState) -> Result<Arc<YTMusicClient>, String> {
    state
        .client
        .lock()
        .map_err(|e| e.to_string())?
        .clone()
        .ok_or_else(|| "not signed in yet".to_string())
}

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    tauri::async_runtime::handle().inner().clone().block_on(future)
}

/// Where `target` is in the library, filing it there first if it is not.
fn place(state: &AppState, library: &mut Library, target: Target) -> Result<TrackRef, String> {
    match target {
        Target::Library { playlist, song } => library
            .track(playlist, song)
            .map(|_| (playlist, song))
            .ok_or_else(|| "that track is no longer in the playlist".to_string()),
        Target::Track { track } => Ok(library.place_search_result(track)),
        Target::History { index } => {
            let entry = state
                .history
                .lock()
                .map_err(|e| e.to_string())?
                .tracks()
                .get(index)
                .cloned()
                .ok_or("no such history entry")?;
            persistence::place_played(library, &entry).ok_or_else(|| "that track has no video id".to_string())
        }
    }
}

/// Runs `f` under the library and player locks, in the workspace's order, then
/// tells the frontend what changed.
fn edit(
    app: &AppHandle,
    state: &AppState,
    f: impl FnOnce(&mut Library, &mut Player) -> Result<(), String>,
) -> Result<(), String> {
    let mut library = state.library.lock().map_err(|e| e.to_string())?;
    let mut player = state.player.lock().map_err(|e| e.to_string())?;
    let result = f(&mut library, &mut player);
    drop(player);
    drop(library);
    crate::player::push(app, state);
    result
}

fn enqueue(library: &Library, player: &mut Player, refs: &[TrackRef], next: bool) {
    if next && player.playing().is_some() {
        // In reverse, so each lands right after what is playing and the batch keeps its order.
        for &(pl, song) in refs.iter().rev() {
            player.insert_next(library, pl, song);
        }
    } else {
        player.append_many(library, refs);
    }
}

/// Plays `target`, queueing the rest of its playlist behind it.
#[tauri::command]
pub fn play_target(app: AppHandle, state: State<'_, AppState>, target: Target) -> Result<(), String> {
    edit(&app, &state, |library, player| {
        let (pl, song) = place(&state, library, target)?;
        player.play(library, pl, song);
        Ok(())
    })
}

/// Plays `tracks[start]` with the whole list as the queue — an album, a result list.
#[tauri::command]
pub fn play_tracks(app: AppHandle, state: State<'_, AppState>, tracks: Vec<Track>, start: usize) -> Result<(), String> {
    edit(&app, &state, |library, player| {
        let refs = library.place_off_library(tracks);
        player.play_list(library, refs, start);
        Ok(())
    })
}

/// "Play Next" (`next`) or "Play Last" for one track.
#[tauri::command]
pub fn queue_target(app: AppHandle, state: State<'_, AppState>, target: Target, next: bool) -> Result<(), String> {
    edit(&app, &state, |library, player| {
        let at = place(&state, library, target)?;
        enqueue(library, player, &[at], next);
        Ok(())
    })
}

/// The same for a list of loose tracks — an album's Play Next.
#[tauri::command]
pub fn queue_tracks(app: AppHandle, state: State<'_, AppState>, tracks: Vec<Track>, next: bool) -> Result<(), String> {
    edit(&app, &state, |library, player| {
        let refs = library.place_off_library(tracks);
        enqueue(library, player, &refs, next);
        Ok(())
    })
}

/// The same for a whole playlist from the sidebar.
#[tauri::command]
pub fn queue_playlist(app: AppHandle, state: State<'_, AppState>, index: usize, next: bool) -> Result<(), String> {
    edit(&app, &state, |library, player| {
        let refs: Vec<TrackRef> = (0..library.songs(index).len()).map(|song| (index, song)).collect();
        if refs.is_empty() {
            return Err("that playlist hasn't loaded yet".to_string());
        }
        enqueue(library, player, &refs, next);
        Ok(())
    })
}

#[tauri::command]
pub fn radio_target(app: AppHandle, state: State<'_, AppState>, target: Target) -> Result<(), String> {
    let mut library = state.library.lock().map_err(|e| e.to_string())?;
    let (pl, song) = place(&state, &mut library, target)?;
    drop(library);
    crate::radio::start(&app, &state, pl, song)
}

/// The video ids in Liked Music, which is what fills a heart in.
#[tauri::command]
pub fn liked_ids(state: State<'_, AppState>) -> Vec<String> {
    let Ok(library) = state.library.lock() else { return Vec::new() };
    library
        .find_playlist_index(LIKED)
        .map(|i| library.songs(i).iter().filter_map(|t| t.video_id.clone()).collect())
        .unwrap_or_default()
}

/// Likes or unlikes a track, then refetches Liked Music to match.
#[tauri::command]
pub async fn set_liked(state: State<'_, AppState>, video_id: String, liked: bool) -> Result<(), String> {
    off_main(&state, move |state| {
        let yt = client(state)?;
        let result = if liked {
            block_on(core_search::like(&yt, &video_id))
        } else {
            block_on(core_search::unlike(&yt, &video_id))
        };
        result.map_err(|e| e.to_string())?;
        let index = state.library.lock().map_err(|e| e.to_string())?.find_playlist_index(LIKED);
        if let Some(index) = index {
            crate::library::refresh_after_edit(state, index)?;
        }
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn remove_from_playlist(state: State<'_, AppState>, playlist: usize, song: usize) -> Result<(), String> {
    off_main(&state, move |state| {
        let library = state.library.lock().map_err(|e| e.to_string())?;
        let playlist_id = library.playlist(playlist).map(|p| p.playlist_id.clone());
        let track = library.track(playlist, song).cloned();
        drop(library);
        let (Some(playlist_id), Some(track)) = (playlist_id, track) else {
            return Err("that track is no longer in the playlist".to_string());
        };
        if track.set_video_id.is_none() {
            return Err("that row can't be removed — refresh the playlist and try again".to_string());
        }
        let yt = client(state)?;
        block_on(core_search::remove_from_playlist(&yt, &playlist_id, &track)).map_err(|e| e.to_string())?;
        crate::library::refresh_after_edit(state, playlist)
    })
    .await
}

/// Creates a playlist, puts `video_id` in it if given, and answers its index.
#[tauri::command]
pub async fn create_playlist(state: State<'_, AppState>, title: String, video_id: Option<String>) -> Result<usize, String> {
    off_main(&state, move |state| {
        let title = title.trim().to_string();
        if title.is_empty() {
            return Err("a playlist needs a name".to_string());
        }
        let yt = client(state)?;
        let playlist_id = block_on(core_search::create_playlist(&yt, &title)).map_err(|e| e.to_string())?;
        if let Some(video_id) = &video_id {
            block_on(core_search::add_to_playlist(&yt, &playlist_id, video_id)).map_err(|e| e.to_string())?;
        }
        let index = state.library.lock().map_err(|e| e.to_string())?.push_playlist(Playlist {
            playlist_id: playlist_id.clone(),
            title,
            count: Some(u32::from(video_id.is_some())),
        });
        state
            .fetcher
            .lock()
            .map_err(|e| e.to_string())?
            .as_ref()
            .ok_or("library not fetched yet")?
            .fetch(index, &playlist_id);
        Ok(index)
    })
    .await
}

#[tauri::command]
pub async fn get_album(state: State<'_, AppState>, browse_id: String) -> Result<AlbumPage, String> {
    off_main(&state, move |state| {
        let yt = client(state)?;
        block_on(browse::album(&yt, &browse_id)).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn get_artist(state: State<'_, AppState>, channel_id: String) -> Result<ArtistPage, String> {
    off_main(&state, move |state| {
        let yt = client(state)?;
        block_on(browse::artist(&yt, &channel_id)).map_err(|e| e.to_string())
    })
    .await
}

// Only music.youtube.com links, so the page can't open anything else through this.
fn music_url(path: &str) -> Result<String, String> {
    const KINDS: [&str; 4] = ["watch?v=", "playlist?list=", "browse/", "channel/"];
    match KINDS.iter().find_map(|k| path.strip_prefix(k)) {
        Some(id) if !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') => {
            Ok(format!("https://music.youtube.com/{path}"))
        }
        _ => Err(format!("not a YouTube Music link: {path}")),
    }
}

#[tauri::command]
pub fn open_in_browser(app: AppHandle, path: String) -> Result<(), String> {
    app.opener().open_url(music_url(&path)?, None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn copy_link(app: AppHandle, path: String) -> Result<(), String> {
    app.clipboard().write_text(music_url(&path)?).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_music_links_are_built() {
        assert_eq!(music_url("watch?v=abc").unwrap(), "https://music.youtube.com/watch?v=abc");
        assert!(music_url("browse/MPREb_x").is_ok());
        assert!(music_url("https://evil.example").is_err());
        assert!(music_url("watch?v=a/../../x").is_err());
    }
}
