//! The desktop's own media controls, for the windowed frontend.
//!
//! `ytm_core::media` already holds the whole protocol — Control Centre on
//! macOS, MPRIS on Linux, the SMTC flyout on Windows — and `tui/src/app.rs`
//! drives it from its event loop. This file is the same handle driven from a
//! GUI, which differs in exactly two ways.
//!
//! ## There is no tick
//!
//! The TUI publishes and drains once per loop iteration, so a media key costs
//! at most one tick. This app deliberately has no such loop left: the ticker in
//! `player.rs` waits on a notification and sleeps five seconds at a time when
//! nothing is playing, which is what makes it nearly free on battery. A press
//! would sit unread for as long as that, so commands are *pushed* instead —
//! `ytm_core::media::queued()` is signalled by whichever backend queued one,
//! and [`spawn_listener`] waits on it. Publishing keeps its own diff below, so
//! the outbound direction stays as quiet as the inbound one.
//!
//! ## The handle belongs to the main thread
//!
//! On macOS `MediaControls` owns `AppKit` objects and is `!Send` by construction,
//! and the run loop that delivers its handler blocks is the main one. So it
//! lives in a thread-local on the main thread and is reached through
//! `run_on_main_thread`, which is a plain dispatch on every platform. That is
//! also why nothing here is `cfg`-ed per OS: one path serves all three, exactly
//! as `ytm_core::media` intends.
//!
//! Under `Host::Windowed` the backend does not touch `NSApplication` and does
//! not turn the run loop itself — Tauri is already an app and already turning
//! it. See `ytm_core::media::Host`.

use std::cell::RefCell;
use std::time::{Duration, Instant};

use tauri::AppHandle;
use ytm_core::{Host, MediaCmd, MediaControls, NowPlaying, PlayState, Track, TrackInfo};

use crate::state::AppState;

/// How big a cover to ask the CDN for. The same figure the TUI sends, since
/// the panel it lands in is the operating system's either way and has nothing
/// to do with the window or the terminal in front of it.
const MEDIA_COVER_PX: u32 = 600;

/// How often the elapsed time is refreshed while nothing else has changed.
///
/// The system extrapolates position from the playback rate, so this only has to
/// be often enough that the two do not visibly drift. It applies while playing
/// and only then: paused, the position is frozen and already correct, and a
/// refresh would be a main-thread wakeup every five seconds for the life of the
/// process to re-send a number that has not moved.
const ELAPSED_INTERVAL: Duration = Duration::from_secs(5);

/// A position change larger than this cannot be the clock advancing, since the
/// ticker looks at least four times a second while a track is running. The same
/// rule `ytm_core::media::is_seek` applies, restated because it is crate-private
/// there — this is the one place a seek has to reach the panel without waiting
/// for [`ELAPSED_INTERVAL`].
const SEEK_JUMP_SECS: f64 = 1.0;

thread_local! {
    /// The handle, on the thread that is allowed to hold it. `None` on a
    /// machine with no media stack, or when `MediaControls::new` declined —
    /// which is logged there and never fatal.
    static CONTROLS: RefCell<Option<MediaControls>> = const { RefCell::new(None) };
}

/// Registers with the desktop. Must be called from the main thread, which is
/// where Tauri's `setup` hook runs.
pub fn init(rt: &tokio::runtime::Handle) {
    let controls = MediaControls::new(rt, Host::Windowed);
    CONTROLS.with_borrow_mut(|slot| *slot = controls);
}

/// Waits for the desktop to ask for something, and acts on it.
///
/// One wake can cover several commands — a run of key presses, a drag on a
/// scrubber — so [`drain`] takes everything queued rather than one.
pub fn spawn_listener(app: AppHandle, state: AppState) {
    let queued = ytm_core::media::queued();
    tauri::async_runtime::spawn(async move {
        loop {
            queued.notified().await;
            drain(&app, &state);
        }
    });
}

/// Publishes the current state, if it has moved somewhere worth reporting.
///
/// Called from [`crate::player::push`] and from the ticker, so the panel
/// follows the same events the interface does. The diff is here rather than
/// left to the backend's own because each call would otherwise cost a
/// main-thread dispatch — and a wakeup four times a second, forever, is the
/// cost this app spent its optimisation getting rid of.
pub fn publish(app: &AppHandle, state: &AppState) {
    let Some(now) = now_playing(state) else { return };

    let Ok(mut last) = state.last_published.lock() else {
        return;
    };
    let send = match last.as_ref() {
        None => true,
        Some((previous, sent_at)) => {
            beyond_position(previous, &now)
                || (now.position - previous.position).abs() > SEEK_JUMP_SECS
                || (now.state == PlayState::Playing && sent_at.elapsed() >= ELAPSED_INTERVAL)
        }
    };
    // The position is remembered whether or not it was sent, or the next
    // comparison is made against a figure several seconds old and every
    // refresh looks like a seek.
    let sent_at = match (send, last.as_ref()) {
        (false, Some((_, at))) => *at,
        _ => Instant::now(),
    };
    *last = Some((now.clone(), sent_at));
    drop(last);

    if !send {
        return;
    }
    let _ = app.run_on_main_thread(move || {
        CONTROLS.with_borrow_mut(|slot| {
            if let Some(controls) = slot.as_mut() {
                controls.update(&now);
            }
        });
    });
}

/// Whether anything moved that the passage of time cannot explain — the same
/// question `PlaybackStateView::differs_beyond_elapsed` asks of the frontend's
/// snapshot, against the fields this panel actually draws.
fn beyond_position(a: &NowPlaying, b: &NowPlaying) -> bool {
    a.state != b.state
        || a.track != b.track
        || a.mode != b.mode
        || a.volume != b.volume
        || a.can_go_next != b.can_go_next
        || a.can_go_previous != b.can_go_previous
        || a.can_play != b.can_play
        || a.can_seek != b.can_seek
}

/// Takes everything the desktop has queued and acts on it, on the main thread.
fn drain(app: &AppHandle, state: &AppState) {
    let handle = app.clone();
    let state = state.clone();
    let _ = app.run_on_main_thread(move || {
        // Collected before anything is acted on: acting re-enters this module
        // through `publish`, and the borrow must be gone by then.
        let mut cmds = Vec::new();
        CONTROLS.with_borrow(|slot| {
            if let Some(controls) = slot.as_ref() {
                while let Some(cmd) = controls.try_recv() {
                    cmds.push(cmd);
                }
            }
        });
        for cmd in cmds {
            act(&handle, &state, cmd);
        }
    });
}

/// One command, mapped onto the same `Player` methods the interface's own
/// buttons call — so a media key and a click are the same press.
fn act(app: &AppHandle, state: &AppState, cmd: MediaCmd) {
    if cmd == MediaCmd::Quit {
        // MPRIS only, and the ordinary way out: `RunEvent::Exit` still fires,
        // so the queue and the volume are saved exactly as on any other quit.
        app.exit(0);
        return;
    }

    {
        let (Ok(library), Ok(mut player)) = (state.library.lock(), state.player.lock()) else {
            return;
        };
        match cmd {
            MediaCmd::Play => player.resume(&library),
            MediaCmd::Pause => {
                player.set_paused(true);
            }
            MediaCmd::PlayPause => player.play_pause(&library),
            MediaCmd::Stop => player.stop(),
            MediaCmd::Next => player.next(&library),
            // The same double-press gesture the TUI gives `p`: past a few
            // seconds this restarts the track, and at the start it steps back.
            // A media key is the same button, and that is what it does
            // everywhere else.
            MediaCmd::Previous => {
                player.restart_or_previous(&library);
            }
            MediaCmd::Seek(secs) => player.seek(secs),
            MediaCmd::SeekTo(secs) => player.seek_to(secs),
            MediaCmd::Volume(v) => player.set_volume(v),
            MediaCmd::Mode(mode) => player.set_mode(mode),
            MediaCmd::Quit => {}
        }
    }
    crate::player::push(app, state);
}

/// The playing track as the panels want it — the mirror of the TUI's own
/// `now_playing`, over the same `Player` and the same `Library`.
fn now_playing(state: &AppState) -> Option<NowPlaying> {
    let (Ok(library), Ok(player)) = (state.library.lock(), state.player.lock()) else {
        return None;
    };
    let audio = player.audio_state();
    let playing = player.playing();

    let track = playing
        .and_then(|(pl, song)| library.track(pl, song))
        .map(|t: &Track| TrackInfo {
            id: t.video_id.clone().unwrap_or_default(),
            title: t.title.clone().unwrap_or_default(),
            artists: t.artists.iter().map(|a| a.name.clone()).collect(),
            album: t.album.as_ref().map(|a| a.name.clone()).unwrap_or_default(),
            // mpv's duration is the accurate one but arrives a beat late, so
            // YouTube's rounded figure stands in until it does.
            length: if audio.total > 0.0 {
                audio.total
            } else {
                t.duration_seconds.unwrap_or(0).into()
            },
            art_url: t
                .thumbnail
                .as_deref()
                .map(|url| ytm_core::cover::at_size(url, MEDIA_COVER_PX))
                .unwrap_or_default(),
        });

    // A queue restored from disk has a track but has never been handed to mpv,
    // so Stopped — not Paused — is the honest answer: `Play` starts it from the
    // beginning, which is exactly what `Player::resume` does there.
    let state = if playing.is_none() || !player.playback_started() {
        PlayState::Stopped
    } else if audio.paused {
        PlayState::Paused
    } else {
        PlayState::Playing
    };

    let queued = !player.queue().is_empty();
    Some(NowPlaying {
        state,
        track,
        mode: player.mode(),
        volume: player.volume(),
        // The queue wraps, so there is always a next once it is non-empty.
        can_go_next: queued,
        can_go_previous: queued,
        can_play: queued || playing.is_some(),
        can_seek: !audio.loading && audio.total > 0.0,
        position: audio.elapsed,
    })
}
