//! Core engine for yt-music-tui: session/auth, library fetching, playback,
//! queue orchestration, and persistence — extracted from the ratatui TUI so
//! it can be driven by something else too (e.g. a headless daemon).

/* This crate is held to a `deny`-level `nursery`/`pedantic` plus a list of
   panic-shaped lints, and it has never actually passed them. In the monorepo
   the table was `[workspace.lints.clippy]`, which a member only inherits by
   saying `[lints] workspace = true` — and only `gui/src-tauri` ever did, so a
   `#[deny]`-level `indexing_slicing` sat next to `self.queue[pos]` in
   `player.rs` while `cargo clippy --workspace` reported a clean tree. In the
   standalone split the table is inline and was always in force, so that repo
   has simply been red since it was created. Either way the backlog is the
   same 395 findings.

   They are listed rather than quietly dropped: each line is a category still
   to burn down, with what it costs, and removing a line is the unit of that
   work. Everything *not* named here is denied, which is the part that matters
   — the gate is real for new code even while the backlog is open.

   Ordered by how much they are worth fixing rather than by count. The first
   group can hide a panic and should go first; the second is API polish; the
   third is style. `expect_used` is not here at all — the crate's one `expect`
   is allowed at its own line, with a `# Panics` section saying why. */
/* Not part of that backlog: `clippy::large_futures` ICEs the toolchain rather
   than reporting anything — "unexpected rigid alias in layout_of after
   normalization" against this crate's own async fns (`cover::fetch`,
   `library::get_songs`, `lyrics::with_retry`, …) on rustc 1.100.0-nightly
   (fb6531d55). `lrclib` needs the same line for the same reason; see the note
   in its `lib.rs`. The crash aborts the run, so it only became visible once
   the errors below stopped stopping it first. Revisit on a newer toolchain. */
#![allow(clippy::large_futures)]

#![allow(clippy::arithmetic_side_effects)] // 61 — overflow; the ones that could bite are fixed
#![allow(clippy::indexing_slicing)] // 22 — panics on a bad index
#![allow(clippy::string_slice)] // 25 — panics mid-UTF-8 character
#![allow(clippy::as_conversions)] // 34 — silent truncation
#![allow(clippy::cast_possible_truncation)] // 12
#![allow(clippy::cast_possible_wrap)] // 4
#![allow(clippy::cast_precision_loss)] // 4
#![allow(clippy::cast_sign_loss)] // 4
#![allow(clippy::missing_errors_doc)] // 26 — `# Errors` sections
#![allow(clippy::must_use_candidate)] // 71
#![allow(clippy::missing_const_for_fn)] // 25
#![allow(clippy::needless_pass_by_value)] // 5
#![allow(clippy::unnecessary_wraps)] // 1
#![allow(clippy::doc_markdown)] // 50 — backticks in prose
#![allow(clippy::too_long_first_doc_paragraph)] // 6
#![allow(clippy::use_self)] // 6
#![allow(clippy::redundant_closure_for_method_calls)] // 8
#![allow(clippy::single_match_else)] // 4
#![allow(clippy::option_if_let_else)] // 4
#![allow(clippy::items_after_statements)] // 3
#![allow(clippy::match_same_arms)] // 2
#![allow(clippy::semicolon_if_nothing_returned)] // 2
#![allow(clippy::format_push_string)] // 2
#![allow(clippy::literal_string_with_formatting_args)] // 2
#![allow(clippy::assert_is_empty)] // 5 — all in tests
#![allow(clippy::many_single_char_names)] // 1
#![allow(clippy::struct_excessive_bools)] // 1
#![allow(clippy::too_many_lines)] // 1
#![allow(clippy::unused_self)] // 1
#![allow(clippy::duration_suboptimal_units)] // 1
#![allow(clippy::case_sensitive_file_extension_comparisons)] // 1
pub mod config;
pub mod cover;
pub mod error;
pub mod library;
pub mod lyrics;
pub mod media;
pub mod persistence;
pub mod playback;
pub mod player;
pub mod search;
pub mod session;
pub mod shutdown;
pub mod translate;

pub use config::Config;
pub use cover::{Cover, CoverMsg};
pub use error::{Error, Result};
pub use library::{Album, Artist, Library, Playlist, PlaylistEntry, Track};
pub use lyrics::{LyricsKind, LyricsMsg, LyricsQuery, LyricsService, TrackLyrics};
pub use media::{Host, MediaCmd, MediaControls, NowPlaying, PlayState, TrackInfo};
pub use playback::AudioState;
pub use player::{AppendOutcome, PlayMode, Player, RemoveOutcome, TrackRef};
pub use search::{ResultKind, SearchMsg, SearchResult};
pub use session::{Browser, Reauth, Session};
pub use translate::TranslateMsg;

/// Re-exported so consumers don't need `ytmusicapi` as a direct dependency.
pub use ytmusicapi::YTMusicClient;
