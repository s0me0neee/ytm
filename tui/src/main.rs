/* The workspace's clippy table reaches this crate for the first time — a
   member only inherits `[workspace.lints]` by saying `[lints] workspace =
   true`, and only `gui/src-tauri` ever did, so the three crates the table was
   written for were held to nothing. See the longer note in `ytm-core`'s
   `lib.rs`.

   158 findings here, listed rather than quietly dropped: each line is a
   category still to burn down, with what it costs, and deleting a line is the
   unit of that work. Everything not named stays denied, so the gate is real
   for new code while the backlog is open. The one `expect` this surfaced is
   gone rather than exempted — it is now an ordinary mutable borrow. */
#![allow(clippy::arithmetic_side_effects)] // 58 — overflow
#![allow(clippy::indexing_slicing)] // 8 — panics on a bad index
#![allow(clippy::as_conversions)] // 45 — silent truncation
#![allow(clippy::cast_possible_truncation)] // 10
#![allow(clippy::cast_possible_wrap)] // 1
#![allow(clippy::cast_sign_loss)] // 7
#![allow(clippy::missing_const_for_fn)] // 4
#![allow(clippy::needless_pass_by_value)] // 1
#![allow(clippy::needless_pass_by_ref_mut)] // 3
#![allow(clippy::doc_markdown)] // 7 — backticks in prose
#![allow(clippy::option_if_let_else)] // 2
#![allow(clippy::items_after_statements)] // 1
#![allow(clippy::format_push_string)] // 1
#![allow(clippy::assigning_clones)] // 1
#![allow(clippy::useless_let_if_seq)] // 1
#![allow(clippy::struct_excessive_bools)] // 1
#![allow(clippy::too_many_lines)] // 2
#![allow(clippy::unused_self)] // 4
#![allow(clippy::assert_is_empty)] // 1 — in tests
#![allow(clippy::redundant_closure_for_method_calls)] // 1
#![allow(clippy::unnested_or_patterns)] // 1

mod app;
mod kitty;

use simplelog::{Config, LevelFilter, WriteLogger};
use std::fs::File;
use std::sync::Arc;

use ytm_core::{Session, library, persistence, session, shutdown};

#[hotpath::main]
fn main() -> anyhow::Result<()> {
    // Ensure config dir exists before anything else touches it.
    let config_dir = session::ensure_config_dir()?;

    WriteLogger::init(
        LevelFilter::Debug,
        Config::default(),
        File::create(config_dir.join("app.log"))?,
    )?;
    log::info!("Start up — config dir: {}", config_dir.display());

    ctrlc::set_handler(shutdown::request_shutdown)?;

    let session = Session::new()?;

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    // Expired cookies are renewed and everything built again from the top, in
    // this same process: `reauth` returns with a working session either way, so
    // there is never an app to run again by hand. Only the *first* start may
    // renew on its own, though — an account whose library really is empty must
    // not have us renewing it once a run forever, so after one attempt an empty
    // library is reported and `r` is what asks for another.
    let mut renewed = false;

    // Setting up leaves a working session, so there is nothing to run again
    // for: the prompts finish and the TUI opens, the same way a renewal
    // mid-session carries straight on rather than ending with an instruction.
    // It counts as the one renewal — the cookies are seconds old, so an empty
    // library after it is an empty library, not a session to fetch again.
    if !session.is_set_up() {
        session.run_setup()?;
        eprintln!("\nSetup complete.\n");
        renewed = true;
    }

    loop {
        match start(&session, &rt, !renewed)? {
            app::Exit::Quit => return Ok(()),
            app::Exit::Reauth => {
                session.reauth()?;
                renewed = true;
            }
        }
    }
}

/// One full start: client, playlists, background fetches, TUI. Returns when the
/// TUI does, saying whether it wants the session renewed and another go.
fn start(
    session: &Session,
    rt: &tokio::runtime::Runtime,
    auto_reauth: bool,
) -> anyhow::Result<app::Exit> {
    // Build the API client immediately with cached cookies, then kick off a
    // background refresh so yt-dlp's 2-5 s run doesn't block startup.
    let mut yt = match session.build_client() {
        Ok(c) => c,
        Err(e) => {
            log::error!("build_client failed: {e:#}");
            eprintln!("\nFailed to load session: {e}");
            session.reauth()?;
            session.build_client()?
        }
    };

    let cookie_refresh = {
        let session = session.clone();
        std::thread::spawn(move || {
            if let Err(e) = session.refresh_cookies() {
                log::warn!("cookie refresh failed (using cached): {e}");
            }
        })
    };

    let playlists = match rt.block_on(library::get_playlists(&yt)) {
        Ok(p) => p,
        Err(ytm_core::Error::SessionExpired) => {
            session.reauth()?;
            yt = session.build_client()?;
            rt.block_on(library::get_playlists(&yt))?
        }
        Err(e) => return Err(e.into()),
    };

    let yt = Arc::new(yt);

    // Spawn per-playlist song fetches in the background so the TUI starts
    // immediately rather than waiting for all network calls to complete. The
    // fetcher outlives them: `r` on a playlist whose fetch failed asks again.
    let (fetcher, songs_rx) =
        library::LibraryFetcher::new(rt.handle(), Arc::clone(&yt), &playlists);

    let saved_queue = persistence::load_queue();
    let lib = library::Library::new(playlists);
    // Read after `Session::new` has ensured the file exists.
    let config = ytm_core::Config::load();

    // Renewing on our own is only ever the silent path — the fallback is a set
    // of prompts, and those belong to a keypress that asked for them.
    let result = app::App::new(
        lib,
        saved_queue,
        songs_rx,
        fetcher,
        rt.handle().clone(),
        config,
        auto_reauth && session::can_auto_reauth(),
    )
    .run();

    // Wait for cookie refresh before exiting so browser.json is never partial.
    let _ = cookie_refresh.join();

    result
}
