use serde::Serialize;
use tauri::State;
use ytm_core::persistence;
use ytm_core::{LyricsKind, LyricsQuery, LyricsService, TrackLyrics};

use crate::state::AppState;

#[derive(Serialize)]
pub struct LyricLineView {
    pub at: f64,
    pub text: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricsView {
    pub synced: bool,
    pub lines: Vec<LyricLineView>,
    /// The lrclib record these words came from. The frontend keys its
    /// translation cache on this, and passes it back as `onScreen` so the
    /// picker can mark which row is in use.
    pub record_id: u64,
    /// True when a manual choice (rather than automatic matching) produced
    /// this, so the panel can say so.
    pub overridden: bool,
}

/// One row of the `c` picker.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricsChoiceView {
    pub id: u64,
    pub track_name: String,
    pub artist_name: String,
    pub album_name: String,
    pub duration: Option<f64>,
    pub synced: bool,
    pub line_count: usize,
    /// Its length is too far from the track's for the timings to be trusted.
    pub timing_mismatch: bool,
}

impl From<&TrackLyrics> for LyricsChoiceView {
    fn from(r: &TrackLyrics) -> Self {
        Self {
            id: r.id,
            track_name: r.track_name.clone(),
            artist_name: r.artist_name.clone(),
            album_name: r.album_name.clone(),
            duration: r.duration,
            synced: r.is_synced(),
            line_count: r.line_count(),
            timing_mismatch: r.timing_mismatch,
        }
    }
}

fn view_of(record: TrackLyrics, overridden: bool) -> LyricsView {
    let record_id = record.id;
    let (synced, lines) = match record.kind {
        LyricsKind::Synced(lines) => (
            true,
            lines
                .into_iter()
                .map(|l| LyricLineView { at: l.at, text: l.text })
                .collect(),
        ),
        LyricsKind::Plain(lines) => (
            false,
            lines
                .into_iter()
                .map(|text| LyricLineView { at: 0.0, text })
                .collect(),
        ),
        LyricsKind::Instrumental => (false, Vec::new()),
    };
    LyricsView { synced, lines, record_id, overridden }
}

fn query_for(state: &AppState, playlist: usize, song: usize) -> Result<(LyricsQuery, String), String> {
    let track = state
        .library
        .lock()
        .map_err(|e| e.to_string())?
        .track(playlist, song)
        .cloned()
        .ok_or_else(|| "no such track".to_string())?;
    let video_id = track.video_id.clone().unwrap_or_default();
    let query = LyricsQuery::from_track(&track).ok_or_else(|| "track has no title".to_string())?;
    Ok((query, video_id))
}

/// Automatic match, unless a manual choice has been saved for this video --
/// the same precedence `best_for`'s `override_id` implements for the TUI.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // tauri::command requires State by value
pub fn get_lyrics(state: State<'_, AppState>, playlist: usize, song: usize) -> Result<Option<LyricsView>, String> {
    let (query, video_id) = query_for(&state, playlist, song)?;
    let override_id = state
        .lyrics_overrides
        .lock()
        .map_err(|e| e.to_string())?
        .get(&video_id);

    let service = LyricsService::new();
    let rt_handle = tauri::async_runtime::handle().inner().clone();
    let found = rt_handle
        .block_on(service.best_for(&query, override_id))
        .map_err(|e| e.to_string())?;

    Ok(found.map(|record| {
        let overridden = override_id == Some(record.id);
        view_of(record, overridden)
    }))
}

/// The `c` picker's rows: every record lrclib offers for this track, with the
/// one on screen guaranteed a place.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // tauri::command requires State by value
pub fn get_lyrics_choices(
    state: State<'_, AppState>,
    playlist: usize,
    song: usize,
    on_screen: Option<u64>,
) -> Result<Vec<LyricsChoiceView>, String> {
    let (query, _) = query_for(&state, playlist, song)?;
    let service = LyricsService::new();
    let rt_handle = tauri::async_runtime::handle().inner().clone();
    let found = rt_handle
        .block_on(service.candidates(&query, on_screen))
        .map_err(|e| e.to_string())?;
    Ok(found.iter().map(LyricsChoiceView::from).collect())
}

/// Records a manual choice and returns the chosen record's words.
///
/// Written straight through to `lyrics.json`: the whole point of choosing is
/// that the choice outlives the session.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // tauri::command requires State by value
pub fn choose_lyrics(
    state: State<'_, AppState>,
    playlist: usize,
    song: usize,
    record_id: u64,
) -> Result<Option<LyricsView>, String> {
    let (_, video_id) = query_for(&state, playlist, song)?;

    let service = LyricsService::new();
    let rt_handle = tauri::async_runtime::handle().inner().clone();
    let found = rt_handle
        .block_on(service.by_id(record_id))
        .map_err(|e| e.to_string())?;

    if found.is_some() && !video_id.is_empty() {
        // Snapshot under the lock, write outside it: the write is a rename
        // over a temporary file, and holding the mutex across it would stall
        // every other command that needs the overrides.
        let snapshot = {
            let mut overrides = state.lyrics_overrides.lock().map_err(|e| e.to_string())?;
            overrides.set(&video_id, record_id);
            overrides.clone()
        };
        if let Err(e) = persistence::save_lyrics_overrides(&snapshot) {
            eprintln!("[gui] could not save the lyric choice: {e}");
        }
    }

    Ok(found.map(|record| view_of(record, true)))
}

/// Drops a manual choice, putting the track back on automatic matching.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // tauri::command requires State by value
pub fn clear_lyrics_override(state: State<'_, AppState>, playlist: usize, song: usize) -> Result<(), String> {
    let (_, video_id) = query_for(&state, playlist, song)?;
    let snapshot = {
        let mut overrides = state.lyrics_overrides.lock().map_err(|e| e.to_string())?;
        overrides.clear(&video_id);
        overrides.clone()
    };
    if let Err(e) = persistence::save_lyrics_overrides(&snapshot) {
        eprintln!("[gui] could not save the cleared lyric choice: {e}");
    }
    Ok(())
}
