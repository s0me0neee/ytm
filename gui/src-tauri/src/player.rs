use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use ytm_core::{Track, TrackRef};

use crate::state::AppState;

/// The snapshot the ticker emits, four times a second.
///
/// Deliberately small. It used to carry `queue: Vec<TrackRef>` -- the whole
/// queue -- which meant that every 250ms, for as long as the app was open,
/// hundreds of entries were cloned out of the player, compared field by field
/// against the previous tick, serialised to JSON, pushed across the IPC
/// boundary and parsed again in the webview. `elapsed` changes on every tick,
/// so the change check never suppressed any of it.
///
/// The frontend only ever read two things out of that vector: the entry
/// playing, and whether the queue had changed. Both are here directly, and
/// the panel that draws the queue asks for it by name (`get_queue`) when
/// `queue_revision` says there is something new to draw.
#[derive(Serialize, Clone, Default, PartialEq)]
pub struct PlaybackStateView {
    pub elapsed: f64,
    pub total: f64,
    pub paused: bool,
    pub loading: bool,
    pub error: Option<String>,
    pub track: Option<String>,
    /// The queue entry playing, rather than the queue.
    pub playing: Option<TrackRef>,
    pub queue_position: Option<usize>,
    pub queue_len: usize,
    /// Bumped by every queue edit, including a shuffle -- which reorders
    /// without changing the length, so length alone would miss it.
    pub queue_revision: u64,
    pub volume: u8,
    pub effective_volume: u8,
    pub muted: bool,
    /// `&'static str`: `PlayMode::label` already returns one, and building an
    /// owned `String` from it was an allocation per tick for a value drawn
    /// from a fixed set of three.
    pub mode: &'static str,
}

impl PlaybackStateView {
    /// Whether anything moved that the passage of time cannot explain.
    ///
    /// `elapsed` advances on every tick by definition, so comparing whole
    /// snapshots only ever answers "has time passed" -- it cannot tell that
    /// apart from the track having changed underneath. This is the question
    /// the emit schedule actually needs answered, and separating the two is
    /// what lets the loop run fast enough to catch a track change while still
    /// sending the progress bar its four updates a second.
    fn differs_beyond_elapsed(&self, other: &Self) -> bool {
        self.track != other.track
            || self.playing != other.playing
            || self.queue_position != other.queue_position
            || self.queue_revision != other.queue_revision
            || self.queue_len != other.queue_len
            || self.paused != other.paused
            || self.loading != other.loading
            || self.error != other.error
            // Bit-compared, not `!=`. The question is "did this value change
            // since the last snapshot", not "are these two lengths close" --
            // it is either the same f64 carried over or a new one mpv
            // reported, and a tolerance would only blur that. It also makes
            // the 0.0 `begin_track` writes differ from a real duration, and
            // stops a NaN comparing unequal to itself forever.
            || self.total.to_bits() != other.total.to_bits()
            || self.volume != other.volume
            || self.effective_volume != other.effective_volume
            || self.muted != other.muted
            || self.mode != other.mode
    }
}

/// Sends the current state to the frontend, unless it already has it.
///
/// Every command that changes playback calls this before returning, which is
/// the difference between the UI following the audio and the UI following the
/// *ticker*. `begin_track` makes the snapshot true on the caller's thread
/// before `Cmd::Play` is even queued, so by the time a command gets here the
/// answer is already correct -- waiting for the next poll to discover that put
/// the interface up to a quarter-second behind its own sound on every press.
pub fn push(app: &AppHandle, state: &AppState) {
    // The desktop's panel is told from the same place and by the same rule, so
    // Control Centre and the window agree about what is playing rather than
    // one of them being a poll behind the other. Its own diff decides whether
    // anything actually crosses to the main thread.
    crate::media::publish(app, state);

    let Some(view) = snapshot(state) else { return };
    // The one place that sees every song start, whichever of the dozen paths
    // to one was taken -- see `history::observe`.
    crate::history::observe(app, state, view.track.as_deref());

    let Ok(mut last) = state.last_emitted.lock() else {
        return;
    };
    if last.as_ref() == Some(&view) {
        return;
    }
    let _ = app.emit("playback-state", &view);
    *last = Some(view);
}

fn snapshot(state: &AppState) -> Option<PlaybackStateView> {
    // Library then player, the workspace's order. Every caller of `push` and
    // the ticker itself releases both before getting here, so this can take
    // them fresh.
    let library = state.library.lock().ok()?;
    let player = state.player.lock().ok()?;
    let audio = player.audio_state();
    let playing = player.playing();
    // mpv's duration is the accurate one but arrives a beat late, so YouTube's
    // rounded figure stands in until it does -- the same substitution the OS
    // panel makes in `media::now_playing`. A queue restored from disk never
    // reaches mpv at all until the user presses play, so without this its
    // progress bar reads 0:00 long of 0:00 over a track whose length is known.
    let total = if audio.total > 0.0 {
        audio.total
    } else {
        playing
            .and_then(|(pl, song)| library.track(pl, song))
            .and_then(|t| t.duration_seconds)
            .map_or(0.0, f64::from)
    };
    Some(PlaybackStateView {
        elapsed: audio.elapsed,
        total,
        // A queue restored from disk has a track selected but has never been
        // handed to mpv, and mpv's `paused` is false because nothing has been
        // asked of it yet. Reported raw, that draws a Pause button and a set of
        // bouncing equaliser bars over silence -- so `playback_started` is
        // folded in here, where it is the same answer `Player::play_pause`
        // gives that button when it is pressed.
        paused: audio.paused || !player.playback_started(),
        loading: audio.loading,
        error: audio.error,
        track: audio.track,
        playing,
        queue_position: player.queue_position(),
        queue_len: player.queue().len(),
        queue_revision: player.queue_revision(),
        volume: player.volume(),
        effective_volume: player.effective_volume(),
        muted: player.is_muted(),
        mode: player.mode().label(),
    })
}

/// How often the clock is worth re-sending while a track is running.
///
/// This is the *only* timer left in the loop, and it exists for one thing: the
/// progress bar. A progress bar is a clock, so something has to be, and four
/// times a second is what the eye reads. Everything else -- a song ending, a
/// load finishing, an error -- arrives on `Player::changed` the moment it
/// happens, so this interval no longer has to be short enough to catch it.
///
/// Which is the whole difference. Polling at 50ms to notice a song end meant
/// twenty wakeups a second for the life of the process, and being told
/// "nothing has happened" on nineteen of them.
const CLOCK_MS: u64 = 250;

/// And the same when nothing is playing, where even the clock has stopped.
///
/// Nothing can change here without a command, and commands report themselves
/// through [`push`], so this is a backstop against a missed notification
/// rather than a sample rate. On a machine left on for an evening, this is the
/// interval the app spends nearly all its life at.
const IDLE_MS: u64 = 5000;

/// Advances the queue when a song ends, and keeps the frontend's copy of the
/// playback state current. There is no render loop here to drive that the way
/// the TUI's `handle_song_end`-per-tick does, so this plays that role.
///
/// It is neither the only thing that emits nor, any longer, a poller. A
/// command that changes playback pushes its own result (see [`push`]), and
/// anything that happens without one — a track ending, a load finishing, an
/// error — wakes this through `Player::changed` at the moment it happens. The
/// timer that remains is for the progress bar, which is a clock and cannot be
/// anything else.
pub fn spawn_ticker(app: AppHandle, state: AppState) {
    tauri::async_runtime::spawn(async move {
        let Ok(changed) = state.player.lock().map(|p| p.changed()) else {
            return;
        };
        let mut clock_sent = std::time::Instant::now();
        let mut active = false;
        loop {
            // Whichever comes first: something happened, or the clock is due
            // another look. With nothing playing the clock is barely running,
            // so this waits on the notification almost exclusively.
            let wait = std::time::Duration::from_millis(if active { CLOCK_MS } else { IDLE_MS });
            let _ = tokio::time::timeout(wait, changed.notified()).await;

            // Lock order must match every command below (library, then player) --
            // acquiring them in the opposite order here would be a lock-order
            // inversion against e.g. `play`/`next`/`append_to_queue`, deadlocking
            // this ticker against whichever command loses the race.
            //
            // Only worth taking both when something is actually playing: a song
            // cannot end when none is running, and an idle app was otherwise
            // taking a lock pair four times a second forever, contending with
            // every command the UI issues for no possible result.
            let playing = state.player.lock().is_ok_and(|p| p.playing().is_some());
            if playing
                && let (Ok(mut library), Ok(mut player)) =
                    (state.library.lock(), state.player.lock())
            {
                player.handle_song_end(&library);
                // Nothing here ever took a track back out of the synthetic
                // search playlist, so it grew for the life of the process --
                // this frontend being the one likely to be left open all day.
                // Folded into the branch that already holds both locks rather
                // than given a pass of its own: the rule declines while
                // anything points into that playlist, so it wants to be asked
                // repeatedly and cheaply, and gating it on something playing
                // costs nothing (nothing is being added to it either way).
                player.prune_search_history(&mut library);
            }

            /* Two schedules, because the snapshot carries two kinds of news.
               A track change, a pause, a load finishing or an error goes out
               at once -- that is what the wait above was woken for. The clock
               going round waits its turn, since it moves continuously and
               nothing is learned from hearing about it more than four times a
               second. Without the split, one interval has to serve both and
               whichever is chosen is wrong for the other. */
            // Before the frontend's own bookkeeping and outside its lock: this
            // is where a song ending or a load finishing first becomes visible,
            // and the OS panel has no other way to hear about it.
            crate::media::publish(&app, &state);

            let Some(view) = snapshot(&state) else { continue };
            // A track that ended and advanced the queue arrives here rather
            // than through a command, so this is the other half of the pair.
            crate::history::observe(&app, &state, view.track.as_deref());
            /* What the *next* pass waits for. Read from the snapshot rather
               than from `playing` above, since a track that is loading or
               paused is still one whose state can move under the interface --
               it is having no track at all that makes the fast loop pointless. */
            active = view.loading || (view.track.is_some() && !view.paused);
            let due = clock_sent.elapsed().as_millis() >= u128::from(CLOCK_MS);
            let Ok(mut last) = state.last_emitted.lock() else {
                continue;
            };
            let news = last
                .as_ref()
                .is_none_or(|prev| prev.differs_beyond_elapsed(&view));
            if !news && !due {
                continue;
            }
            if last.as_ref() == Some(&view) {
                continue;
            }
            let _ = app.emit("playback-state", &view);
            *last = Some(view);
            if due {
                clock_sent = std::time::Instant::now();
            }
        }
    });
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // tauri::command requires State by value
pub fn playback_state(state: State<'_, AppState>) -> Option<PlaybackStateView> {
    snapshot(&state)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // tauri::command requires State by value
pub fn current_track(state: State<'_, AppState>) -> Option<Track> {
    let (pl, song) = state.player.lock().ok()?.playing()?;
    state.library.lock().ok()?.track(pl, song).cloned()
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // tauri::command requires State by value
pub fn play(app: AppHandle, state: State<'_, AppState>, playlist: usize, song: usize) -> Result<(), String> {
    {
        let library = state.library.lock().map_err(|e| e.to_string())?;
        state.player.lock().map_err(|e| e.to_string())?.play(&library, playlist, song);
    }
    push(&app, &state);
    Ok(())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // tauri::command requires State by value
pub fn play_pause(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    {
        let library = state.library.lock().map_err(|e| e.to_string())?;
        state.player.lock().map_err(|e| e.to_string())?.play_pause(&library);
    }
    push(&app, &state);
    Ok(())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // tauri::command requires State by value
pub fn next(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    {
        let library = state.library.lock().map_err(|e| e.to_string())?;
        state.player.lock().map_err(|e| e.to_string())?.next(&library);
    }
    push(&app, &state);
    Ok(())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // tauri::command requires State by value
pub fn prev(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    {
        let library = state.library.lock().map_err(|e| e.to_string())?;
        state.player.lock().map_err(|e| e.to_string())?.prev(&library);
    }
    push(&app, &state);
    Ok(())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // tauri::command requires State by value
pub fn seek(app: AppHandle, state: State<'_, AppState>, delta_secs: f64) -> Result<(), String> {
    state.player.lock().map_err(|e| e.to_string())?.seek(delta_secs);
    push(&app, &state);
    Ok(())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // tauri::command requires State by value
pub fn seek_to(app: AppHandle, state: State<'_, AppState>, secs: f64) -> Result<(), String> {
    state.player.lock().map_err(|e| e.to_string())?.seek_to(secs);
    push(&app, &state);
    Ok(())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // tauri::command requires State by value
pub fn set_volume(app: AppHandle, state: State<'_, AppState>, volume: u8) -> Result<(), String> {
    state.player.lock().map_err(|e| e.to_string())?.set_volume(volume);
    push(&app, &state);
    Ok(())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // tauri::command requires State by value
pub fn toggle_mute(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    state.player.lock().map_err(|e| e.to_string())?.toggle_mute();
    push(&app, &state);
    Ok(())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // tauri::command requires State by value
pub fn cycle_mode(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    state.player.lock().map_err(|e| e.to_string())?.cycle_mode();
    push(&app, &state);
    Ok(())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // tauri::command requires State by value
pub fn append_to_queue(app: AppHandle, state: State<'_, AppState>, playlist: usize, song: usize) -> Result<(), String> {
    {
        let library = state.library.lock().map_err(|e| e.to_string())?;
        state
            .player
            .lock()
            .map_err(|e| e.to_string())?
            .append_to_queue(&library, playlist, song);
    }
    push(&app, &state);
    Ok(())
}

/// One row of the Up Next panel: a queue entry resolved to the track it
/// names, since the queue itself holds only `(playlist, song)` positions.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueEntryView {
    /// Position in the queue -- what `jump_to` and `remove_from_queue` take.
    pub q_pos: usize,
    pub playlist: usize,
    pub song: usize,
    pub title: String,
    pub artist: String,
    pub duration: String,
    pub thumbnail: Option<String>,
    pub video_id: Option<String>,
    /// Whether this is the entry currently playing.
    pub current: bool,
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // tauri::command requires State by value
pub fn get_queue(state: State<'_, AppState>) -> Vec<QueueEntryView> {
    let (Ok(library), Ok(player)) = (state.library.lock(), state.player.lock()) else {
        return Vec::new();
    };
    let playing = player.queue_position();
    player
        .queue()
        .iter()
        .enumerate()
        .map(|(q_pos, &(playlist, song))| {
            let track = library.track(playlist, song);
            QueueEntryView {
                q_pos,
                playlist,
                song,
                title: track
                    .and_then(|t| t.title.clone())
                    .unwrap_or_else(|| "Untitled".to_string()),
                artist: track.map(Track::artist_names).unwrap_or_default(),
                duration: track.and_then(|t| t.duration.clone()).unwrap_or_default(),
                thumbnail: track.and_then(|t| t.thumbnail.clone()),
                video_id: track.and_then(|t| t.video_id.clone()),
                current: Some(q_pos) == playing,
            }
        })
        .collect()
}

/// Inserts directly after what is playing -- "Play Next" -- against
/// `append_to_queue`'s "Play Last".
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // tauri::command requires State by value
pub fn play_next(app: AppHandle, state: State<'_, AppState>, playlist: usize, song: usize) -> Result<(), String> {
    {
        let library = state.library.lock().map_err(|e| e.to_string())?;
        state
            .player
            .lock()
            .map_err(|e| e.to_string())?
            .insert_next(&library, playlist, song);
    }
    push(&app, &state);
    Ok(())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // tauri::command requires State by value
pub fn clear_queue(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    state.player.lock().map_err(|e| e.to_string())?.clear_queue();
    push(&app, &state);
    Ok(())
}

/// Warms the CDN URL for a track the user is likely to play next, so pressing
/// play doesn't wait on a yt-dlp resolve. The TUI does this from j/k in the
/// songs list; here the trigger is hovering a row.
///
/// Deliberately infallible past the lock: a prefetch is a guess, and a guess
/// that can't be made (no video id, track gone) is not worth reporting.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // tauri::command requires State by value
pub fn prefetch(state: State<'_, AppState>, playlist: usize, song: usize) {
    let Ok(library) = state.library.lock() else { return };
    let Some(video_id) = library.track(playlist, song).and_then(|t| t.video_id.as_deref()) else {
        return;
    };
    if let Ok(player) = state.player.lock() {
        player.prefetch(video_id);
    }
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // tauri::command requires State by value
pub fn remove_from_queue(app: AppHandle, state: State<'_, AppState>, q_pos: usize) -> Result<(), String> {
    {
        let library = state.library.lock().map_err(|e| e.to_string())?;
        state.player.lock().map_err(|e| e.to_string())?.remove_from_queue(&library, q_pos);
    }
    push(&app, &state);
    Ok(())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // tauri::command requires State by value
pub fn jump_to(app: AppHandle, state: State<'_, AppState>, q_pos: usize) -> Result<(), String> {
    {
        let library = state.library.lock().map_err(|e| e.to_string())?;
        state.player.lock().map_err(|e| e.to_string())?.jump_to(&library, q_pos);
    }
    push(&app, &state);
    Ok(())
}
