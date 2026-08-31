mod auth;
mod history;
mod library;
mod lyrics;
mod media;
mod persist;
mod player;
mod profile;
mod search;
mod state;
mod translate;

use std::sync::{Arc, Mutex};

use state::AppState;
use tauri::{Emitter, Manager};

/// Opens `app.log` in the config directory — the same file, at the same level,
/// that `tui/src/main.rs` opens.
///
/// Truncating rather than appending, again as the TUI does: the file describes
/// the most recent start, and a log that only grows is one nobody reads. A GUI
/// needs this more than a terminal app does, having no console to have printed
/// to instead — without it every `log::` line in `ytm-core` is discarded, which
/// is most of what the app knows about its own auth, playback and media
/// backends.
///
/// Failure is not fatal and cannot be reported through the log: a read-only
/// home directory should cost the diagnostics, not the player.
fn init_logging() {
    let Ok(dir) = ytm_core::session::ensure_config_dir() else {
        eprintln!("[gui] no config directory; logging disabled");
        return;
    };
    let path = dir.join("app.log");
    match std::fs::File::create(&path) {
        Ok(file) => {
            if simplelog::WriteLogger::init(
                simplelog::LevelFilter::Debug,
                simplelog::Config::default(),
                file,
            )
            .is_ok()
            {
                log::info!("start up — config dir: {}", dir.display());
            }
        }
        Err(e) => eprintln!("[gui] could not open {}: {e}", path.display()),
    }
}

/// # Errors
/// Returns an error if the Tauri runtime fails to start.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() -> tauri::Result<()> {
    init_logging();
    tauri::Builder::default()
        .setup(|app| {
            let session = ytm_core::Session::new()?;
            let rt_handle = tauri::async_runtime::handle().inner().clone();
            // Hoisted out of the struct literal below: the player is built
            // from it too, so reading it there would mean loading the file
            // twice and logging every warning in it twice with it.
            let config = ytm_core::Config::load();
            let mut player = ytm_core::Player::new(rt_handle.clone(), config.audio);
            // Before the player is shared, so nothing can observe it at the
            // default and then see it jump. `settings.json` is the TUI's file,
            // read the same way — see `persist.rs`.
            player.set_volume(ytm_core::persistence::load_settings().volume);

            let state = AppState {
                session: session.clone(),
                library: Arc::new(Mutex::new(ytm_core::Library::default())),
                player: Arc::new(Mutex::new(player)),
                client: Arc::new(Mutex::new(None)),
                fetcher: Arc::new(Mutex::new(None)),
                config: Arc::new(config),
                lyrics_overrides: Arc::new(Mutex::new(
                    ytm_core::persistence::load_lyrics_overrides(),
                )),
                translations: Arc::new(Mutex::new(ytm_core::persistence::load_translations())),
                pending_refresh: Arc::new(Mutex::new(std::collections::HashMap::new())),
                last_emitted: Arc::new(Mutex::new(None)),
                // Resolved by `persist::try_restore_queue` as the playlists it
                // names arrive; nothing to do here but read the file.
                pending_queue_restore: Arc::new(Mutex::new(
                    ytm_core::persistence::load_queue(),
                )),
                last_published: Arc::new(Mutex::new(None)),
                history: Arc::new(Mutex::new(ytm_core::persistence::load_history())),
                last_noted: Arc::new(Mutex::new(None)),
                bootstrapping: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            };
            app.manage(state.clone());

            // On the main thread, which is the one requirement: macOS delivers
            // remote-command handlers through its run loop and will not hand
            // AppKit objects to anything else.
            media::init(&rt_handle);
            media::spawn_listener(app.handle().clone(), state.clone());
            player::spawn_ticker(app.handle().clone(), state.clone());

            if session.is_set_up() {
                let app_handle = app.handle().clone();
                tauri::async_runtime::spawn_blocking(move || {
                    if let Err(e) = library::bootstrap(&app_handle, &state) {
                        eprintln!("[gui] bootstrap failed: {e}");
                        let _ = app_handle.emit("bootstrap-error", e);
                    }
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            auth::auth_status,
            auth::list_browsers,
            auth::sign_in,
            library::get_playlists,
            library::get_songs,
            library::refetch_playlist,
            player::playback_state,
            player::current_track,
            player::play,
            player::play_pause,
            player::next,
            player::prev,
            player::seek,
            player::seek_to,
            player::set_volume,
            player::toggle_mute,
            player::cycle_mode,
            player::append_to_queue,
            player::remove_from_queue,
            player::jump_to,
            player::prefetch,
            player::get_queue,
            player::play_next,
            player::clear_queue,
            search::search,
            search::add_to_playlist,
            search::like_track,
            search::play_search_result,
            search::queue_search_result,
            lyrics::get_lyrics,
            lyrics::get_lyrics_choices,
            lyrics::choose_lyrics,
            lyrics::clear_lyrics_override,
            translate::get_config,
            translate::translate_lyrics,
            profile::log_render_timing,
            history::get_history,
            history::play_history_track,
        ])
        .build(tauri::generate_context!())?
        // `build` + `run` rather than `run` alone, purely to get here. This is
        // where the TUI's own save-on-exit block lives, at the end of
        // `App::run` once the terminal has been restored: the queue and the
        // volume are written on the way out, not on every change, so a volume
        // drag is not a file write per frame.
        .run(|handle, event| {
            if matches!(event, tauri::RunEvent::Exit)
                && let Some(state) = handle.try_state::<AppState>()
            {
                persist::save(&state);
            }
        });
    Ok(())
}
