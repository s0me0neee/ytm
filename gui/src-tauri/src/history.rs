//! What has been played lately — the home page's whole content.
//!
//! Recording happens in one place rather than in each of the several commands
//! that can start a song. `play`, `jump_to`, `next`, `prev`, a media key, the
//! queue advancing at the end of a track and a search result all end with mpv
//! being handed a different video, and `AudioState::track` is what says so —
//! set by `begin_track` on the caller's thread the moment a track is started.
//! So [`observe`] watches that one field, and every path that can start a song
//! is covered by construction, including the ones added later.
//!
//! What is stored is the whole `Track`, not a reference to one: a `TrackRef` is
//! a position and means nothing across a restart, and a track played from
//! search belongs to no playlist that will exist next time. See
//! `ytm_core::persistence::PlayedTrack`.

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use ytm_core::persistence::{self, PlayedTrack};

use crate::state::AppState;

/// The home page's list.
#[derive(Serialize)]
pub struct HistoryView {
    pub tracks: Vec<PlayedTrack>,
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // tauri::command requires State by value
pub fn get_history(state: State<'_, AppState>) -> HistoryView {
    state.history.lock().map_or_else(
        |_| HistoryView { tracks: Vec::new() },
        |h| HistoryView {
            tracks: h.tracks().to_vec(),
        },
    )
}

/// Notices that a different song has started, and records it.
///
/// Called from the same two places `playback-state` is emitted from, so it sees
/// every start without a hook of its own in each command. `last_noted` is what
/// makes it once-per-song rather than once-per-tick: the ticker passes through
/// here four times a second while a track plays, and the video id is the same
/// every time but the first.
pub fn observe(app: &AppHandle, state: &AppState, playing_video: Option<&str>) {
    let Some(video_id) = playing_video else { return };
    // Read, but not yet written. Every `return` below this point is a
    // resolution that did not work -- the library still loading, a playlist
    // refetched out from under the position -- and claiming the id before
    // doing the work meant the retry on the next tick was suppressed by our
    // own bookkeeping. The play was then dropped for good, even where the
    // cause lasted a single tick. Committed at the end instead, so "noted"
    // means noted.
    {
        let Ok(last) = state.last_noted.lock() else {
            return;
        };
        if last.as_deref() == Some(video_id) {
            return;
        }
    }

    // The track and its playlist, resolved while the library is to hand --
    // library then player, the workspace's order.
    let resolved = {
        let (Ok(library), Ok(player)) = (state.library.lock(), state.player.lock()) else {
            return;
        };
        let Some((pl, song)) = player.playing() else {
            return;
        };
        let Some(track) = library.track(pl, song).cloned() else {
            return;
        };
        // Recorded so `play_history_track` can put the song back where it came
        // from rather than under the search playlist. The synthetic one exists
        // for a single session and is nowhere to go back to, so a track played
        // from search is recorded with no playlist at all.
        let playlist_id = (!library.is_search_playlist(pl))
            .then(|| library.playlist(pl))
            .flatten()
            .map(|p| p.playlist_id.clone());
        (track, playlist_id)
    };
    let (track, playlist_id) = resolved;

    // Claiming the id is what stops the next tick doing this again, so it is
    // deliberately *not* conditional on the entry being kept. `note_track`
    // answers false for a track with no video id, which is a permanent
    // property of that track rather than a moment that will pass -- retrying
    // it four times a second for the length of the song would be a lock pair
    // and a library lookup per tick for an answer that cannot change. The
    // failures above are the opposite case and still fall through without
    // claiming anything.
    let kept = {
        let Ok(mut history) = state.history.lock() else {
            return;
        };
        history.note_track(track, playlist_id).then(|| history.clone())
    };
    if let Ok(mut last) = state.last_noted.lock() {
        *last = Some(video_id.to_string());
    }
    let Some(snapshot) = kept else { return };

    let _ = app.emit("history-changed", ());
    // Off the caller's thread: this is reached from the ticker and from every
    // command that starts a song, and neither should wait on a file write.
    // Written per song rather than at exit, because a history that a crash can
    // erase is one nobody trusts -- and a song is three minutes, not a frame.
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(e) = persistence::save_history(&snapshot) {
            log::warn!("failed to save history: {e}");
        }
    });
}

/// Plays a track from the home page.
///
/// The stored `Track` is the fallback, not the first choice: if the song is
/// still where it was played from, it is played *there*, so the queue and the
/// rest of that playlist behave exactly as they would from the library. Only
/// when it cannot be found — the playlist is gone, the track was removed, or it
/// came from search and never had one — is it filed under the search playlist,
/// which is the same path `play_search_result` takes.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // tauri::command requires State by value
pub fn play_history_track(
    app: AppHandle,
    state: State<'_, AppState>,
    index: usize,
) -> Result<(), String> {
    let entry = state
        .history
        .lock()
        .map_err(|e| e.to_string())?
        .tracks()
        .get(index)
        .cloned()
        .ok_or_else(|| "no such history entry".to_string())?;

    let video_id = entry
        .track
        .video_id
        .clone()
        .ok_or_else(|| "that track has no video id".to_string())?;

    {
        // Bound to a name rather than left as a temporary: the guard has to
        // outlive the `player.play` below, which reads through it.
        let mut library = state.library.lock().map_err(|e| e.to_string())?;
        let found = entry
            .playlist_id
            .as_deref()
            .and_then(|id| library.find_playlist_index(id))
            .and_then(|pl| library.find_song_index(pl, &video_id).map(|song| (pl, song)));
        let (pl, song) = match found {
            Some(at) => at,
            None => library.place_search_result(entry.track),
        };
        {
            let mut player = state.player.lock().map_err(|e| e.to_string())?;
            player.play(&library, pl, song);
        }
        drop(library);
    }
    crate::player::push(&app, &state);
    Ok(())
}
