//! Audio playback: an mpv instance embedded in-process via `libmpv2`, driven
//! from a dedicated thread through a command mailbox.
//!
//! Split into its own thread (rather than driven from the caller's event
//! loop) so a blocking mpv/yt-dlp call never stalls anything else — worst
//! case this thread hangs, not the whole process.

use std::collections::{HashMap, HashSet};
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use libmpv2::events::{Event, PropertyData};
use libmpv2::{Format, Mpv};

use crate::shutdown::is_shutdown_requested;

#[allow(dead_code)]
pub enum Cmd {
    Play(String),     // video_id
    Prefetch(String), // pre-resolve CDN URL before the user presses play
    Pause,
    Resume,
    Seek(f64),    // relative seconds
    SeekAbs(f64), // absolute position, seconds
    Volume(u8),   // 0-100
    Stop,
}

#[derive(Clone, Default)]
pub struct AudioState {
    pub elapsed: f64,
    pub total: f64,
    pub paused: bool,
    pub loading: bool,
    pub error: Option<String>,
    pub song_ended: bool, // set on natural eof; caller must reset after reading
    /// The video the rest of this snapshot describes.
    ///
    /// Without it there is no way to tell a figure that has arrived from one
    /// left over: `total` is the *previous* track's until mpv reports the new
    /// one, and a reader that can't see the difference will use it. See
    /// [`AudioEngine::begin_track`].
    pub track: Option<String>,
}

pub struct AudioEngine {
    cmd_tx: Option<std::sync::mpsc::Sender<Cmd>>,
    state: Arc<Mutex<AudioState>>,
    audio_thread: Option<thread::JoinHandle<()>>,
    changed: Arc<tokio::sync::Notify>,
}

impl AudioEngine {
    /// `rt` is the app's own runtime, which the resolve threads borrow to screen
    /// a URL before mpv is handed it (see [`serves_whole_file`]). Taking a handle
    /// rather than building a client of our own is what keeps this to one
    /// reactor for the process.
    ///
    /// # Panics
    /// If the OS refuses to start the audio thread. Called once, at startup,
    /// before either frontend has a UI to report a failure into — and there is
    /// no playback at all without that thread, so there is nothing a caller
    /// could do with a `Result` here but exit.
    #[allow(clippy::expect_used)] // see `# Panics`
    pub fn new(rt: tokio::runtime::Handle, audio: crate::config::Audio) -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        let state = Arc::new(Mutex::new(AudioState::default()));
        let state2 = Arc::clone(&state);
        let changed = Arc::new(tokio::sync::Notify::new());
        let changed2 = Arc::clone(&changed);
        let handle = thread::Builder::new()
            .name("audio".into())
            .spawn(move || run(rx, state2, rt, audio, changed2))
            // The OS refusing a thread at startup is not a condition any
            // caller could act on -- there is no audio without this thread,
            // and both frontends build the engine before they have a UI to
            // report into. Documented under `# Panics` above.
            .expect("spawn audio thread");
        Self {
            cmd_tx: Some(tx),
            state,
            audio_thread: Some(handle),
            changed,
        }
    }

    /// Waits until something happens to playback that a listener would want to
    /// redraw for — a track ending, a load finishing, an error arriving.
    ///
    /// The alternative is asking on a timer, and a timer has to be set for the
    /// worst case: a frontend that wants to know within 50ms that a song ended
    /// has to ask twenty times a second forever, and is told "nothing" almost
    /// every time. This is told once, when there is something to tell.
    ///
    /// Deliberately *not* signalled for the clock. `elapsed` moves constantly
    /// and by itself means only that time is passing, so waking a listener for
    /// it would put the timer back with extra steps. A progress bar is a clock
    /// and is entitled to one of its own; nothing else here is.
    #[must_use]
    pub fn changed(&self) -> Arc<tokio::sync::Notify> {
        Arc::clone(&self.changed)
    }

    pub fn send(&self, cmd: Cmd) {
        if let Some(tx) = &self.cmd_tx {
            let _ = tx.send(cmd);
        }
    }

    /// Snapshot of the current playback state.
    pub fn state(&self) -> AudioState {
        self.lock_state().clone()
    }

    /// Says the snapshot now describes `video_id`, before the audio thread has
    /// seen the [`Cmd::Play`] that follows.
    ///
    /// The commands are a mailbox and the audio thread reads it every 20 ms,
    /// but the caller carries on immediately — and the very next thing the
    /// event loop does is look at this state. For those few milliseconds every
    /// figure in it belongs to the track that was playing a moment ago, and a
    /// reader has no way to know: `total` is a plausible number for the wrong
    /// song. Stamping the id here, from the thread that decided to play it,
    /// closes the window rather than narrowing it.
    pub fn begin_track(&self, video_id: &str) {
        let mut s = self.lock_state();
        s.track = Some(video_id.to_string());
        s.elapsed = 0.0;
        s.total = 0.0;
        s.loading = true;
        s.song_ended = false;
        s.error = None;
    }

    /// Atomically reads and clears `song_ended`. `true` only once per natural
    /// end-of-track.
    pub fn take_song_ended(&self) -> bool {
        let mut s = self.lock_state();
        std::mem::take(&mut s.song_ended)
    }

    fn lock_state(&self) -> std::sync::MutexGuard<'_, AudioState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        // Dropping the sender ends the audio thread, which drops the embedded mpv.
        drop(self.cmd_tx.take());
        if let Some(h) = self.audio_thread.take() {
            let _ = h.join();
        }
    }
}

// ── URL resolution ──────────────────────────────────────────────────────────────

/// How many times a resolve is worth repeating when what came back is a URL mpv
/// will not be able to play.
///
/// Each yt-dlp run draws afresh (see [`serves_whole_file`]), so the odds
/// compound: at the refusal rate measured, four attempts leave a small fraction
/// of plays to the load-failure retry that was the only defence before.
const MAX_RESOLVE_ATTEMPTS: usize = 4;

/// How long the screen may take. It is one round trip to the host that is about
/// to serve the song, sitting in front of playback starting — either it answers
/// at once or it is not worth waiting for.
const SCREEN_TIMEOUT: Duration = Duration::from_secs(4);

/// The one client the screen uses.
///
/// Every song is on a different `rr*.googlevideo` host, so unlike
/// [`crate::cover`]'s there is no connection pool here worth sharing — but
/// there is no reason to rebuild the client per request either.
fn screen_client() -> Option<&'static reqwest::Client> {
    static CLIENT: std::sync::OnceLock<Option<reqwest::Client>> = std::sync::OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .timeout(SCREEN_TIMEOUT)
                .build()
                .inspect_err(|e| {
                    log::warn!("[audio] no HTTP client ({e}) — resolved URLs go unscreened");
                })
                .ok()
        })
        .as_ref()
}

/// Whether `url` will serve the file in one response — the only way mpv ever
/// asks for it.
///
/// YouTube runs a server-side experiment, carried in the URL's own `fexp`, that
/// refuses whole-file requests and serves bounded chunks only. yt-dlp never
/// notices, because it downloads in chunks anyway; mpv opens with
/// `Range: bytes=0-` and is answered **403**, which arrives as
/// `MPV_ERROR_LOADING_FAILED` and is indistinguishable from the stale URL that
/// error usually means. Which bucket a URL lands in is drawn per resolve rather
/// than being a property of the song — measured over 24 resolves of one video,
/// between a third and two thirds refused, and the same video played on the
/// next attempt. So this asks the question mpv is about to ask.
///
/// A request that doesn't complete answers `true`. The point is to catch a
/// refusal, and condemning a URL over a network hiccup would spend three more
/// yt-dlp runs to arrive back where it started. Only headers are read: the
/// response is dropped without touching the body, so nothing of the song is
/// downloaded twice.
fn serves_whole_file(rt: &tokio::runtime::Handle, url: &str) -> bool {
    let Some(client) = screen_client() else {
        return true;
    };
    rt.block_on(async {
        match client.get(url).header("Range", "bytes=0-").send().await {
            Ok(resp) => {
                let status = resp.status();
                drop(resp);
                if status.is_success() {
                    return true;
                }
                log::debug!("[audio] screen: {status} for a whole-file request");
                false
            }
            Err(e) => {
                log::debug!("[audio] screen: could not ask ({e}) — using the URL anyway");
                true
            }
        }
    })
}

/// How long the audio thread waits between passes while a track is running.
///
/// It bounds how stale `AudioState::elapsed` can be, and nothing reads that
/// finer: the GUI's ticker samples at 250ms and the TUI's lyric scheduler at
/// 33ms and up, against lrclib timings that are themselves only good to about
/// a tenth of a second. Commands do not wait for it at all — they wake the
/// thread directly.
const ACTIVE_POLL: Duration = Duration::from_millis(50);

/// And while a track is loaded but paused.
///
/// Half a second because nothing anyone is waiting for can arrive in this
/// state. Playback cannot end when none is running, and every other change
/// comes down the command channel, which this waits on rather than polls. It
/// is a backstop, not a sample rate.
const IDLE_POLL: Duration = Duration::from_millis(500);

/// And with nothing loaded at all — a fresh start, or a queue played out.
///
/// This is the state the app spends whole evenings in, so it gets the longest
/// wait: five seconds is a fifth of a wakeup per second, which rounds to
/// nothing against everything else on the machine. It is not `recv()` with no
/// timeout, which would be truly free, only because that would depend on mpv
/// having *nothing* to say while idle — true as far as it is known here, and
/// not worth a hang if it ever stops being. A timeout that long costs
/// essentially what blocking forever costs and cannot wedge.
const STOPPED_POLL: Duration = Duration::from_secs(5);

/// One yt-dlp run. The URL it gives back still has to get past
/// [`serves_whole_file`] before mpv is handed it.
fn yt_dlp_url(video_id: &str) -> Option<String> {
    let yt_url = format!("https://music.youtube.com/watch?v={video_id}");
    let out = Command::new("yt-dlp")
        .args([
            "-f",
            "bestaudio[ext=webm]/bestaudio[ext=m4a]/bestaudio",
            "--get-url",
            "--no-playlist",
            &yt_url,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;

    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8(out.stdout).ok()?;
    let url = stdout.trim().lines().next()?.to_string();
    if url.starts_with("http") {
        Some(url)
    } else {
        None
    }
}

#[hotpath::measure]
fn resolve_url(rt: &tokio::runtime::Handle, video_id: &str) -> Option<String> {
    let mut last = None;
    for attempt in 1..=MAX_RESOLVE_ATTEMPTS {
        // Don't start new yt-dlp work if the app is shutting down. Checked each
        // time round, not once: four attempts is several seconds of runs.
        if is_shutdown_requested() {
            return None;
        }
        log::debug!("[audio] yt-dlp resolving {video_id}");
        let Some(url) = yt_dlp_url(video_id) else {
            log::warn!("[audio] yt-dlp failed for {video_id}");
            break;
        };
        if serves_whole_file(rt, &url) {
            if attempt > 1 {
                log::info!("[audio] {video_id}: a playable URL on attempt {attempt}");
            }
            return Some(url);
        }
        log::debug!("[audio] {video_id}: attempt {attempt} came back chunk-only — resolving again");
        last = Some(url);
    }
    // Out of attempts, so hand the last one over rather than nothing. `None`
    // sends the caller to mpv's own ytdl_hook, which draws from the same
    // lottery, and the screen itself can be wrong — a captive portal answering
    // 403 to everything would otherwise take playback down with it. Trying is
    // what this did before the screen existed, and the load-failure retry is
    // still behind it.
    if last.is_some() {
        log::warn!(
            "[audio] {video_id}: {MAX_RESOLVE_ATTEMPTS} resolves all came back chunk-only — trying the last anyway"
        );
    }
    last
}

// ── audio thread ──────────────────────────────────────────────────────────────

fn lock_state(state: &Mutex<AudioState>) -> std::sync::MutexGuard<'_, AudioState> {
    // Recover from a poisoned lock rather than panicking the audio thread.
    state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Set an mpv property, logging (but not propagating) any error.
fn set_prop<V: libmpv2::SetData>(mpv: &Mpv, name: &str, value: V) {
    if let Err(e) = mpv.set_property(name, value) {
        log::warn!("[audio] set_property {name} failed: {e}");
    }
}

/// Load `url` into mpv (replacing whatever is playing) and unpause.
fn load_url(mpv: &Mpv, url: &str) {
    if let Err(e) = mpv.command("loadfile", &[url, "replace"]) {
        log::warn!("[audio] loadfile failed: {e}");
    }
    set_prop(mpv, "pause", false);
}

/// mpv errors that mean "that URL did not play", as opposed to something being
/// wrong with the player itself.
///
/// `LOADING_FAILED` is the everyday one — a CDN URL that has gone stale. The
/// other two are how a *fallback* fails once mpv has opened something that
/// isn't media at all: an expiry page parses as an unknown format, or as a
/// container with no playable stream. Matching only `LOADING_FAILED` meant the
/// fallback's own failure fell through to the catch-all warn arm, so the retry
/// could neither escalate nor give up and the player sat "loading" forever.
///
/// `UNSUPPORTED` (-18) is deliberately absent: it means the system can't play
/// this kind of stream, which a different URL for the same song won't change.
const LOAD_FAILED: &[libmpv2::MpvError] = &[
    -13, // MPV_ERROR_LOADING_FAILED
    -16, // MPV_ERROR_NOTHING_TO_PLAY
    -17, // MPV_ERROR_UNKNOWN_FORMAT
];

// ── the resolved-URL cache ────────────────────────────────────────────────────

/// How long a resolved CDN URL is worth keeping.
///
/// YouTube's expire in a few hours, and a stale one costs a failed load plus a
/// re-resolve — the exact work the cache exists to avoid. An hour is well
/// inside the window and long enough to cover any listening session.
const URL_TTL: Duration = Duration::from_secs(3600);

/// How many to hold. Prefetch warms two per j/k keystroke, so without a bound
/// this grows for as long as the app runs, keeping URLs that expired hours ago.
const MAX_CACHED_URLS: usize = 64;

/// Resolved CDN URLs, bounded in both size and age.
#[derive(Default)]
struct UrlCache {
    entries: HashMap<String, (Instant, String)>,
}

impl UrlCache {
    /// The URL for `id`, if one was resolved recently enough to still work.
    fn get(&mut self, id: &str) -> Option<String> {
        let (at, url) = self.entries.get(id)?;
        if at.elapsed() < URL_TTL {
            return Some(url.clone());
        }
        log::debug!("[audio] cached URL for {id} has aged out");
        self.entries.remove(id);
        None
    }

    /// Whether a *usable* URL is held — the check `Prefetch` makes before
    /// spending a yt-dlp run.
    fn has(&mut self, id: &str) -> bool {
        self.get(id).is_some()
    }

    fn insert(&mut self, id: String, url: String) {
        self.entries.insert(id, (Instant::now(), url));
        if self.entries.len() <= MAX_CACHED_URLS {
            return;
        }
        // Age out whatever has expired first; only if that isn't enough does
        // the oldest survivor go, which is the least likely to be played next.
        self.entries.retain(|_, (at, _)| at.elapsed() < URL_TTL);
        while self.entries.len() > MAX_CACHED_URLS {
            let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(id, (at, _))| (*at, (*id).clone()))
                .map(|(id, _)| id.clone())
            else {
                return;
            };
            self.entries.remove(&oldest);
        }
    }

    fn remove(&mut self, id: &str) {
        self.entries.remove(id);
    }
}

/// Starts a yt-dlp resolve for `id` on its own thread, unless one is already
/// running. `what` only names the thread, for debuggers and panic messages.
fn spawn_resolve(
    id: &str,
    what: &str,
    fetching: &mut HashSet<String>,
    tx: &std::sync::mpsc::Sender<(String, Option<String>)>,
    rt: &tokio::runtime::Handle,
) {
    if !fetching.insert(id.to_string()) {
        return; // already in flight; its result will be delivered to us anyway
    }
    let tx = tx.clone();
    let id = id.to_string();
    let rt = rt.clone();
    thread::Builder::new()
        .name(format!("{what}-{id}"))
        .spawn(move || {
            let _ = tx.send((id.clone(), resolve_url(&rt, &id)));
        })
        .ok();
}

/// Everything about the playback state except the clock, in a form two passes
/// of the loop can be compared by.
///
/// Derived once per pass rather than signalled from each of the dozen places
/// that write to `AudioState`: a notification that has to be remembered at
/// every call site is one that will eventually be forgotten at a new one, and
/// the symptom — an interface that stops updating for one particular kind of
/// event — is the sort that survives a long time unnoticed.
fn material(s: &AudioState) -> (Option<String>, bool, bool, bool, bool, u64) {
    (
        s.track.clone(),
        s.paused,
        s.loading,
        s.song_ended,
        s.error.is_some(),
        s.total.to_bits(),
    )
}

fn run(
    rx: Receiver<Cmd>,
    state: Arc<Mutex<AudioState>>,
    rt: tokio::runtime::Handle,
    audio: crate::config::Audio,
    changed: Arc<tokio::sync::Notify>,
) {
    #[cfg(windows)]
    let which_cmd = "where";
    #[cfg(not(windows))]
    let which_cmd = "which";
    match Command::new(which_cmd).arg("yt-dlp").output() {
        Ok(o) if o.status.success() => {
            log::info!(
                "[audio] yt-dlp found at {}",
                String::from_utf8_lossy(&o.stdout).trim()
            );
        }
        _ => {
            let msg = "yt-dlp not found — install with: brew install yt-dlp".to_string();
            log::error!("[audio] {msg}");
            lock_state(&state).error = Some(msg);
        }
    }

    // ── create embedded mpv (options must be set before init) ──────────────────
    let mpv = match Mpv::with_initializer(|init| {
        init.set_property("vid", "no")?;
        init.set_property("vo", "null")?;
        init.set_property("ytdl", "yes")?;
        init.set_property(
            "ytdl-format",
            "bestaudio[ext=webm]/bestaudio[ext=m4a]/bestaudio",
        )?;
        init.set_property("script-opts", "ytdl_hook-ytdl_path=yt-dlp")?;
        init.set_property("gapless-audio", "yes")?;
        init.set_property("audio-display", "no")?;

        /* mpv's built-in Lua scripts, off. Each is a *thread* — a `sample` of
           the running app found seven of them (`stats`, `console`, `select`,
           `positioning`, `commands`, `context_menu`, plus the OSC), every one
           an on-screen or interactive feature of the mpv *player*. This mpv
           has no window, no video output and no keyboard: there is nothing any
           of them could draw on or respond to, so they were pure inventory.

           `load-scripts` is deliberately left alone. It would take `ytdl_hook`
           with it, which is the fallback `Cmd::Play` drops to when a direct
           resolve fails — the path that keeps playback working when yt-dlp
           and mpv disagree about a URL. */
        for script in [
            "osc",
            "load-stats-overlay",
            "load-console",
            "load-select",
            "load-positioning",
            "load-commands",
            "load-context-menu",
            "load-auto-profiles",
        ] {
            init.set_property(script, "no")?;
        }

        /* And mpv's terminal handling. It is a library here, not a program:
           there is no terminal of its own to read keys from or print status
           to, and under the TUI the terminal it would find belongs to
           crossterm in raw mode. Same reasoning for the key bindings — every
           command reaches this mpv through `Cmd`, never a keypress. */
        init.set_property("terminal", "no")?;
        init.set_property("input-terminal", "no")?;
        init.set_property("input-default-bindings", "no")?;
        init.set_property("input-builtin-bindings", "no")?;
        // What PipeWire/PulseAudio calls this stream, so the system mixer
        // lists the app under its own name and slider rather than "mpv".
        init.set_property("audio-client-name", "ytm")?;
        init.set_property("cache", "yes")?;
        init.set_property("demuxer-readahead-secs", 30i64)?;
        init.set_property("cache-pause-initial", "no")?; // start ASAP, don't pre-fill
        // Probe only a little before starting — YouTube audio is detected from the
        // first packets, so this avoids ffmpeg reading megabytes up front. The
        // LOADING_FAILED retry re-resolves if a stream ever needs more.
        init.set_property("demuxer-lavf-probesize", 65536i64)?;
        init.set_property("demuxer-lavf-analyzeduration", 0.1f64)?;
        init.set_property("idle", "yes")?;
        init.set_property("keep-open", "yes")?;

        /* Hold the signal under full scale. Nothing upstream does: YouTube's
           CDN hands back the master as it was cut, and four of five tracks
           measured off it decode to peaks above 0 dBFS — the loudest at
           +2.0 dBTP. mpv has no headroom to absorb that, so the peaks are
           flattened by whatever converts to the sound card's format, which is
           clipping in the one place nothing can be done about it afterwards.

           `level=no` is the load-bearing half. `alimiter`'s `level` defaults
           to *true*, which makes it an auto-gain stage that lifts quiet
           material up to the ceiling — the opposite of what is wanted here,
           and silent about it. Off, the filter only ever attenuates, so a
           track that never reaches the ceiling passes through untouched.

           Attack and release are libavfilter's defaults, named rather than
           left implicit because they are what makes this transparent: 5ms is
           short enough to catch a peak without pulling the note down with
           it, and 50ms long enough not to modulate the bass. */
        if let Some(limit) = audio.limit_amplitude() {
            init.set_property(
                "af",
                format!("alimiter=limit={limit:.6}:level=no:attack=5:release=50").as_str(),
            )?;
        }
        Ok(())
    }) {
        Ok(m) => m,
        Err(e) => {
            let msg = format!("libmpv init failed: {e}");
            log::error!("[audio] {msg}");
            lock_state(&state).error = Some(msg);
            return;
        }
    };

    for prop in ["time-pos", "duration"] {
        if let Err(e) = mpv.observe_property(prop, Format::Double, 0) {
            log::error!("[audio] observe {prop} failed: {e}");
        }
    }
    for prop in ["pause", "eof-reached"] {
        if let Err(e) = mpv.observe_property(prop, Format::Flag, 0) {
            log::error!("[audio] observe {prop} failed: {e}");
        }
    }
    log::info!("[audio] embedded mpv ready, entering event loop");

    // ── background URL resolution ─────────────────────────────────────────────
    let (fetch_tx, fetch_rx) = std::sync::mpsc::channel::<(String, Option<String>)>();
    let mut url_cache = UrlCache::default();
    let mut fetching: HashSet<String> = HashSet::new();
    let mut pending_resolve: Option<String> = None;
    // Track of the song mpv is loading, so a load failure can be retried once.
    let mut current_id: Option<String> = None;
    // A command taken by the wait at the foot of the loop, waiting for the
    // drain at the top of the next pass to handle it.
    let mut held: Option<Cmd> = None;
    // What the state looked like at the end of the previous pass, so this one
    // can tell whether anything happened worth waking a listener for.
    let mut was = material(&lock_state(&state));
    let mut load_retried = false;

    // Max concurrent yt-dlp resolves (play-resolve + prefetches share `fetching`).
    const MAX_PREFETCH: usize = 3;

    // ── main loop ─────────────────────────────────────────────────────────────
    loop {
        if is_shutdown_requested() {
            log::info!("[audio] QUIT signal — dropping mpv");
            return;
        }

        // ── drain background resolution results ──────────────────────────────
        while let Ok((id, maybe_url)) = fetch_rx.try_recv() {
            fetching.remove(&id);
            match maybe_url {
                Some(url) => {
                    // If the user is waiting on this song, load it now.
                    if pending_resolve.as_deref() == Some(id.as_str()) {
                        log::info!("[audio] loading resolved CDN URL for {id}");
                        load_url(&mpv, &url);
                        pending_resolve = None;
                    }
                    log::info!("[audio] cached CDN URL for {id}");
                    url_cache.insert(id, url);
                }
                None => {
                    if pending_resolve.as_deref() == Some(id.as_str()) {
                        // Fall back to mpv's own ytdl_hook so playback still works.
                        log::warn!("[audio] resolve failed for {id} — falling back to ytdl_hook");
                        load_url(&mpv, &format!("https://music.youtube.com/watch?v={id}"));
                        pending_resolve = None;
                    } else {
                        log::warn!("[audio] prefetch resolve failed for {id}");
                    }
                }
            }
        }

        // ── commands from UI ─────────────────────────────────────────────────
        let mut disconnected = false;
        loop {
            // `held` is the command the wait at the foot of the loop already
            // took off the channel. There is no way to put one back, and it
            // has to be handled *here* rather than there, because this is
            // where the handling lives.
            match held.take().map_or_else(|| rx.try_recv(), Ok) {
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    log::info!("[audio] channel disconnected — dropping mpv");
                    disconnected = true;
                    break;
                }
                Ok(cmd) => match cmd {
                    Cmd::Play(id) => {
                        pending_resolve = None;
                        current_id = Some(id.clone());
                        load_retried = false;
                        {
                            let mut s = lock_state(&state);
                            s.track = Some(id.clone());
                            s.loading = true;
                            s.paused = false;
                            s.elapsed = 0.0;
                            s.total = 0.0;
                            s.error = None;
                            s.song_ended = false;
                        }

                        if let Some(cached) = url_cache.get(&id) {
                            log::info!("[audio] Play {id}: cache HIT — instant CDN URL");
                            load_url(&mpv, &cached);
                        } else {
                            // Cache miss: resolve ourselves and load when it lands.
                            // Don't hand mpv the watch URL — that spawns a second,
                            // competing yt-dlp via its ytdl_hook.
                            log::info!("[audio] Play {id}: cache miss — resolving (single yt-dlp)");
                            pending_resolve = Some(id.clone());
                            spawn_resolve(&id, "resolve", &mut fetching, &fetch_tx, &rt);
                        }
                    }

                    Cmd::Prefetch(id) => {
                        if url_cache.has(&id) || fetching.contains(&id) {
                            continue;
                        }
                        if fetching.len() >= MAX_PREFETCH {
                            continue;
                        }
                        spawn_resolve(&id, "prefetch", &mut fetching, &fetch_tx, &rt);
                    }

                    Cmd::Pause => {
                        log::debug!("[audio] Pause");
                        set_prop(&mpv, "pause", true);
                    }
                    Cmd::Resume => {
                        log::debug!("[audio] Resume");
                        set_prop(&mpv, "pause", false);
                    }
                    Cmd::Seek(d) => {
                        log::debug!("[audio] Seek {d:+}s");
                        let ds = d.to_string();
                        if let Err(e) = mpv.command("seek", &[ds.as_str(), "relative"]) {
                            log::warn!("[audio] seek failed: {e}");
                        }
                    }
                    Cmd::SeekAbs(t) => {
                        log::debug!("[audio] SeekAbs {t:.1}s");
                        let ts = t.to_string();
                        if let Err(e) = mpv.command("seek", &[ts.as_str(), "absolute"]) {
                            log::warn!("[audio] absolute seek failed: {e}");
                        }
                    }
                    Cmd::Volume(v) => {
                        log::debug!("[audio] Volume {v}");
                        set_prop(&mpv, "volume", i64::from(v));
                    }
                    Cmd::Stop => {
                        log::debug!("[audio] Stop");
                        pending_resolve = None; // don't auto-play a late resolve
                        current_id = None;
                        lock_state(&state).track = None;
                        if let Err(e) = mpv.command("stop", &[]) {
                            log::warn!("[audio] stop failed: {e}");
                        }
                    }
                },
            }
        }
        if disconnected {
            return;
        }

        // ── events from mpv ──────────────────────────────────────────────────
        while let Some(ev) = mpv.wait_event(0.0) {
            match ev {
                Ok(Event::PropertyChange { name, change, .. }) => {
                    let mut s = lock_state(&state);
                    match (name, change) {
                        ("time-pos", PropertyData::Double(v)) => s.elapsed = v,
                        ("pause", PropertyData::Flag(b)) => s.paused = b,
                        // Deliberately *not* clearing `pending_resolve`: this
                        // event belongs to whatever mpv has open, which during
                        // a track change is still the previous song. Clearing
                        // it here threw away the load the user is waiting for,
                        // so the new track's URL resolved, was cached, and was
                        // never handed to mpv — audio carrying on with the old
                        // song while the UI showed the new one. A resolve for a
                        // song since superseded is already ignored below, by
                        // the id it is compared against.
                        ("duration", PropertyData::Double(v)) => {
                            log::info!("[audio] duration: {v:.1}s");
                            s.total = v;
                            s.loading = false;
                        }
                        ("eof-reached", PropertyData::Flag(true)) => {
                            log::info!("[audio] eof-reached → song_ended");
                            s.song_ended = true;
                        }
                        _ => {}
                    }
                }
                Ok(Event::StartFile) => {
                    log::info!("[audio] start-file");
                    let mut s = lock_state(&state);
                    s.loading = true;
                    s.elapsed = 0.0;
                }
                Ok(Event::EndFile(reason)) => {
                    log::info!("[audio] end-file: reason={reason}");
                    lock_state(&state).loading = false;
                }
                Ok(Event::FileLoaded) => log::info!("[audio] file-loaded"),
                Ok(_) => {}
                // The URL didn't play. Drop it and resolve the song again, once.
                Err(libmpv2::Error::Raw(code)) if LOAD_FAILED.contains(&code) => {
                    match current_id.clone() {
                        Some(id) if !load_retried => {
                            log::warn!("[audio] load failed for {id} (mpv {code}) — re-resolving");
                            load_retried = true;
                            url_cache.remove(&id);
                            // Resolve it ourselves rather than handing mpv the
                            // watch URL. Its ytdl_hook is the path that failed
                            // in the field, on a video a fresh single yt-dlp
                            // resolve then played without complaint. If this
                            // resolve comes back empty we still fall back to
                            // ytdl_hook, where the results are drained.
                            pending_resolve = Some(id.clone());
                            spawn_resolve(&id, "re-resolve", &mut fetching, &fetch_tx, &rt);
                        }
                        Some(id) => {
                            log::error!(
                                "[audio] load failed for {id} (mpv {code}) after retry — giving up"
                            );
                            // Forget it here too. The re-resolve above cached
                            // its URL on the way past — that is where every
                            // resolve is cached — so giving up without this
                            // left a URL known not to play sitting in the cache
                            // as a hit, and the next `p`/`n` back onto the song
                            // failed on it instantly without even re-resolving.
                            url_cache.remove(&id);
                            let mut s = lock_state(&state);
                            s.loading = false;
                            s.error = Some("playback failed".into());
                        }
                        None => log::warn!("[audio] load failed (mpv {code}), no current track"),
                    }
                }
                Err(e) => log::warn!("[audio] mpv event error: {e}"),
            }
        }

        /* Wait, rather than sleep. This used to be an unconditional
           `sleep(20ms)`, which meant the thread woke 50 times a second for
           the whole life of the process -- playing, paused, or sitting on an
           empty queue overnight. Idle wakeups are what keep a laptop's SoC
           out of its deep sleep states, and this was the largest source of
           them in the app by some way.

           Blocking on the command channel is better on *both* axes at once.
           A command no longer waits for the current sleep to expire, so a
           keypress is acted on the moment it arrives instead of up to 20ms
           later; and with nothing to do, the timeout is all that is left, so
           the cost of being idle is the cost of the timeout.

           Which is why it is chosen by what is actually happening. While a
           track runs there are mpv events worth collecting promptly -- the
           clock the progress bar and the lyric timing read. Paused or
           stopped, nothing arrives that anybody is waiting for: playback
           cannot end on its own, and anything the user does comes down this
           very channel and wakes it immediately. */
        // Anything a listener would redraw for, told once rather than
        // discovered by asking. See `AudioEngine::changed`.
        let interval = {
            let s = lock_state(&state);
            let now = material(&s);
            if now != was {
                was = now;
                changed.notify_waiters();
            }
            match (s.loading || (s.track.is_some() && !s.paused), s.track.is_some()) {
                (true, _) => ACTIVE_POLL,
                (false, true) => IDLE_POLL,
                (false, false) => STOPPED_POLL,
            }
        };
        match rx.recv_timeout(interval) {
            Ok(cmd) => held = Some(cmd),
            Err(RecvTimeoutError::Timeout) => {}
            // The drain at the top of the next pass sees this too and takes
            // the shutdown path, which is where that logic belongs.
            Err(RecvTimeoutError::Disconnected) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cache_of(n: usize) -> UrlCache {
        let mut cache = UrlCache::default();
        for i in 0..n {
            cache.insert(format!("id{i}"), format!("https://cdn/{i}"));
        }
        cache
    }

    /// No audio thread, no mpv — `begin_track`/`state` only ever touch the
    /// shared `Mutex`, so this is enough to test them in isolation.
    fn engine_without_a_thread() -> AudioEngine {
        AudioEngine {
            cmd_tx: None,
            state: Arc::new(Mutex::new(AudioState::default())),
            audio_thread: None,
            changed: Arc::new(tokio::sync::Notify::new()),
        }
    }

    #[test]
    fn begin_track_is_true_the_instant_it_returns() {
        let engine = engine_without_a_thread();

        engine.begin_track("abc123");

        // No Cmd::Play has been sent (there is no thread to read it), and yet
        // the snapshot already names the new track and clears the old one's
        // figures -- that immediacy is the whole point.
        let s = engine.state();
        assert_eq!(s.track.as_deref(), Some("abc123"));
        assert_eq!(s.elapsed, 0.0);
        assert_eq!(s.total, 0.0);
        assert!(s.loading);
        assert!(!s.song_ended);
        assert!(s.error.is_none());
    }

    #[test]
    fn begin_track_clears_the_previous_tracks_leftovers() {
        let engine = engine_without_a_thread();
        {
            let mut s = engine.lock_state();
            s.track = Some("old".to_string());
            s.elapsed = 123.0;
            s.total = 200.0;
            s.song_ended = true;
            s.error = Some("previous failure".to_string());
        }

        engine.begin_track("new");

        let s = engine.state();
        assert_eq!(s.track.as_deref(), Some("new"));
        assert_eq!(s.total, 0.0, "the old song's length must not survive");
        assert!(!s.song_ended);
        assert!(s.error.is_none());
    }

    /// The screen, against the live CDN.
    ///
    /// Resolves one track several times and asks of each URL handed back the
    /// question mpv would ask. Without the screen this is the coin toss that
    /// made playback fail so often — at the refusal rate measured, all four
    /// coming back playable happens about one run in sixteen — so a pass says
    /// the screen is doing its job rather than that YouTube was in a good mood.
    /// Three of four rather than four, because [`resolve_url`] deliberately
    /// hands over an unscreenable URL rather than nothing once it runs out of
    /// attempts.
    #[test]
    #[ignore = "hits YouTube and spends several yt-dlp runs"]
    fn resolving_gives_back_urls_that_actually_stream() {
        // Deliberately multi-threaded rather than `new_current_thread`: a
        // current-thread runtime only drives its own I/O and timer reactor
        // while a call is inside that *specific* `Runtime`'s own `block_on` —
        // driven instead through a cloned `Handle`, as `resolve_url` and
        // `serves_whole_file` do (they carry a `&Handle`, matching
        // `AudioEngine`'s real one, built by `tui/src/main.rs` as
        // multi-threaded), it falls back to a bare `CachedParkThread` park
        // with nothing left actively polling that reactor. `SCREEN_TIMEOUT`
        // never fires then, so a stalled connection — which live YouTube
        // traffic hits often enough to be the whole reason this test exists —
        // hangs the test forever instead of failing it. One worker thread is
        // enough to keep a real reactor running underneath the `Handle`.
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("runtime");
        let handle = rt.handle().clone();
        let played = (0..4)
            .filter_map(|_| resolve_url(&handle, "0JqnlBedvvQ"))
            .filter(|url| serves_whole_file(&handle, url))
            .count();
        assert!(played >= 3, "only {played} of 4 resolves gave a playable URL");
    }

    #[test]
    fn a_resolved_url_comes_back() {
        let mut cache = cache_of(1);
        assert_eq!(cache.get("id0").as_deref(), Some("https://cdn/0"));
        assert!(cache.get("nothing").is_none());
        assert!(cache.has("id0"));
    }

    #[test]
    fn the_cache_stops_growing() {
        // j/k warms two URLs a keystroke, so an afternoon of browsing used to
        // leave thousands here — most of them expired hours earlier.
        let cache = cache_of(MAX_CACHED_URLS + 50);
        assert_eq!(cache.entries.len(), MAX_CACHED_URLS);
    }

    #[test]
    fn the_oldest_goes_first() {
        let mut cache = cache_of(MAX_CACHED_URLS);
        cache.insert("newest".to_string(), "https://cdn/new".to_string());
        assert_eq!(cache.entries.len(), MAX_CACHED_URLS);
        assert!(cache.has("newest"), "just resolved, and evicted anyway");
        assert!(!cache.has("id0"), "the oldest should have gone");
    }

    #[test]
    fn a_url_past_its_life_is_not_offered() {
        // A YouTube CDN URL expires on its own; handing an expired one to mpv
        // costs a failed load and a re-resolve, which is the work the cache is
        // supposed to save.
        let mut cache = UrlCache::default();
        cache.entries.insert(
            "stale".to_string(),
            (
                Instant::now()
                    .checked_sub(URL_TTL + Duration::from_secs(1))
                    .expect("a monotonic clock older than the TTL"),
                "https://cdn/stale".to_string(),
            ),
        );
        assert!(cache.get("stale").is_none());
        assert!(!cache.has("stale"));
        // And it is dropped rather than asked about again.
        assert!(cache.entries.is_empty());
    }

    #[test]
    fn a_url_that_failed_to_play_is_forgotten() {
        let mut cache = cache_of(2);
        cache.remove("id0");
        assert!(!cache.has("id0"));
        assert!(cache.has("id1"));
    }
}
