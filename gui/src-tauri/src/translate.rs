use serde::Serialize;
use tauri::State;
use ytm_core::persistence;
use ytm_core::translate;

use crate::state::AppState;

/// What the frontend needs to know about `config.toml` -- read once at
/// startup, same as the TUI reads it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigView {
    /// Seconds to shift every lyric line by. Applied to the clock handed to
    /// the active-line search, never to the cached records.
    pub lyrics_offset: f64,
    /// The language `i` translates into, empty when translation is off.
    pub translate_to: String,
    /// Whether `I` -- the paid path -- is set up and has a key.
    pub ai_available: bool,
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // tauri::command requires State by value
pub fn get_config(state: State<'_, AppState>) -> ConfigView {
    ConfigView {
        lyrics_offset: state.config.lyrics.offset,
        translate_to: state.config.lyrics.translate_to.clone(),
        ai_available: state.config.lyrics.ai_available(),
    }
}

#[derive(Serialize)]
pub struct TranslationView {
    /// One entry per input line, empty where nothing could be translated.
    pub lines: Vec<String>,
    /// The model that answered, or empty when the free endpoint did.
    pub model: String,
}

/// Translates one record's lines, through the free endpoint or the AI model.
///
/// The caching rules are the TUI's, and all follow from `translations.json`
/// holding one translation per lrclib record: only the AI ones are saved (the
/// free endpoint costs nothing but a wait, so asking again each session lets
/// its answer improve), nothing is saved when the answering model came back
/// empty (an `I` request the free path ended up serving is not what `I`
/// bought), and `force` pushes past the cache for a redo -- the fresh answer
/// overwrites only when it actually lands, so a redo that hits a rate limit
/// leaves the paid translation where it was.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // tauri::command requires State by value
pub fn translate_lyrics(
    state: State<'_, AppState>,
    record_id: u64,
    lines: Vec<String>,
    use_ai: bool,
    force: bool,
) -> Result<TranslationView, String> {
    let backend = state.config.lyrics.backend(use_ai);
    if backend.to.is_empty() {
        return Err("no translation language is configured".to_string());
    }

    if use_ai && !force {
        let cached = state
            .translations
            .lock()
            .map_err(|e| e.to_string())?
            .get(record_id, &backend.to)
            .map(<[String]>::to_vec);
        if let Some(lines) = cached {
            return Ok(TranslationView { lines, model: String::new() });
        }
    }

    let rt_handle = tauri::async_runtime::handle().inner().clone();
    let done = rt_handle.block_on(translate::translate_lines(&lines, &backend))?;

    if !done.model.is_empty() {
        // Snapshot under the lock, write outside it -- same reasoning as
        // `lyrics::choose_lyrics`.
        let snapshot = {
            let mut saved = state.translations.lock().map_err(|e| e.to_string())?;
            saved.set(record_id, &backend.to, &done.model, done.lines.clone());
            saved.clone()
        };
        if let Err(e) = persistence::save_translations(&snapshot) {
            eprintln!("[gui] could not save the translation: {e}");
        }
    }

    Ok(TranslationView { lines: done.lines, model: done.model })
}
