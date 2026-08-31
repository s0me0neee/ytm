//! A client for the [lrclib.net](https://lrclib.net) lyrics API, plus a parser
//! for the LRC synced-lyrics format the API returns.
//!
//! [`api`] is the transport — it mirrors the HTTP endpoints and returns records
//! verbatim. [`lrc`] turns a record's `synced_lyrics` string into timestamped
//! [`LyricLine`]s and answers "which line is playing at time *t*".

/* Not a style exemption: `clippy::large_futures` ICEs on this crate under the
   workspace's feature unification -- "unexpected rigid alias in layout_of
   after normalization" against `api_error`'s own opaque future, on
   rustc 1.100.0-nightly (fb6531d55). Nothing was reported because the ICE
   aborts the run, and until `[lints] workspace = true` was added here the
   whole table was inert anyway, so the crash had never been reached.
   Revisit on a newer toolchain; the lint is worth having back. */
#![allow(clippy::large_futures)]

pub mod api;
pub mod lrc;

pub use api::{LrcError, LrcLib, Lyrics};
pub use lrc::{LyricLine, active_index, next_boundary, parse_lrc};
