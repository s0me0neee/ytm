//! The queue and the volume, across restarts — the same two files the TUI
//! writes, in the same format, in the same directory.
//!
//! Deliberately not a second scheme. `queue.json` and `settings.json` are read
//! and written through `ytm_core::persistence` exactly as `tui/src/app.rs` does
//! it, so the two frontends are two views of one saved state: quit the TUI
//! mid-album, open the GUI, and the queue is where it was left. That is only
//! true because nothing here decides anything about the format — every rule
//! about what a position means after entries drop out lives in `persistence`,
//! and both callers get it by using the same functions.
//!
//! `lyrics.json` and `translations.json` were already shared this way (see
//! `lyrics.rs` and `translate.rs`), which is what the file-per-concern split in
//! `persistence` was for.

use tauri::AppHandle;
use ytm_core::persistence::{self, QueueState, RestoreOutcome};

use crate::state::AppState;

/// Resolves the saved queue against the library, once enough of it has loaded.
///
/// Called after every song batch, because that is the only thing that can
/// change the answer: a saved entry names a playlist and a video id, and until
/// the playlist holding it has arrived there is no position to resolve it to.
/// [`RestoreOutcome::Pending`] means exactly that and is not a failure — the
/// TUI's `try_restore_queue` waits the same way, and for the same reason a
/// playlist whose fetch *failed* keeps the queue waiting rather than
/// discarding it.
pub fn try_restore_queue(app: &AppHandle, state: &AppState) {
    // Cloned and released before the library and the player are touched. This
    // is a leaf lock taken nowhere else, but holding it across the pair below
    // would put it into the lock order for no gain.
    let saved: Option<QueueState> = state
        .pending_queue_restore
        .lock()
        .ok()
        .and_then(|s| s.clone());
    let Some(saved) = saved else { return };

    // Library then player, matching every command and the ticker.
    let (Ok(library), Ok(mut player)) = (state.library.lock(), state.player.lock()) else {
        return;
    };
    let outcome = persistence::try_restore(&library, &saved);
    let restored = match outcome {
        RestoreOutcome::Pending => return,
        RestoreOutcome::Abandoned => {
            log::info!("saved queue abandoned: nothing in it still exists");
            None
        }
        RestoreOutcome::Ready { queue, position } => {
            player.restore(&library, queue, position);
            Some((player.queue().len(), position))
        }
    };
    drop(player);
    drop(library);

    if let Ok(mut pending) = state.pending_queue_restore.lock() {
        *pending = None;
    }
    if let Some((len, position)) = restored {
        log::info!("restored queue: len={len} pos={position:?}");
        // The queue arrived without anyone pressing anything, so nothing else
        // would tell the frontend about it.
        crate::player::push(app, state);
    }
}

/// Writes the queue and the volume out. Called once, on the way out.
///
/// Both go through `persistence`, so both land via `write_private` — a private
/// temporary renamed over the target. That matters most here: this runs while
/// the process is being torn down, which is when a half-written file is most
/// likely and least recoverable.
pub fn save(state: &AppState) {
    let (Ok(library), Ok(player)) = (state.library.lock(), state.player.lock()) else {
        return;
    };

    let built = persistence::build_queue_state(&library, player.queue(), player.queue_position());
    // `None` has two causes and they want opposite things. An *empty* queue is
    // the user having cleared it, and must overwrite the file or the next
    // launch restores what they cleared — the GUI has a "clear queue" button
    // where the TUI has no way to empty a queue at all, so this case is new
    // here. `None` from a queue that is *not* empty means nothing in it
    // resolved to a video id, which is what a quit during loading looks like,
    // and the right answer to that is to leave the saved queue alone.
    let state_to_save = match built {
        Some(built) => Some(built),
        None if player.queue().is_empty() => Some(QueueState {
            entries: Vec::new(),
            position: None,
        }),
        None => None,
    };
    if let Some(queue) = state_to_save
        && let Err(e) = persistence::save_queue(&queue)
    {
        log::warn!("failed to save queue: {e}");
    }

    // The *effective* volume, so quitting while muted doesn't come back at
    // zero with the mute forgotten.
    if let Err(e) = persistence::save_settings(&persistence::Settings {
        volume: player.effective_volume(),
    }) {
        log::warn!("failed to save settings: {e}");
    }
}
