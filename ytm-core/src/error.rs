//! Unified error type for the crate's public API.

use crate::session::Browser;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The YouTube Music session has expired and re-authentication is required.
    #[error("YouTube Music session expired — re-authenticate")]
    SessionExpired,

    #[error(transparent)]
    Ytmusicapi(#[from] ytmusicapi::Error),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Prompt(#[from] inquire::InquireError),

    /// `browser`'s cookie store couldn't be read at all. Usually the browser
    /// isn't installed, its profile isn't where expected, or (Safari) the
    /// process lacks Full Disk Access — `diagnosis` carries rookie's own
    /// error chain, which already names the specific cause and fix.
    #[error("couldn't read cookies from {browser}: {diagnosis}")]
    BrowserNotSignedIn { browser: Browser, diagnosis: String },

    /// The cookie store was read, but none of its cookies were for `*.youtube.com`.
    #[error("no youtube.com cookies found in {browser} — are you signed in to YouTube Music?")]
    NoCookiesFound { browser: Browser },

    /// The pasted cURL command had no `-H`/`-b` flags at all.
    #[error("no headers found — make sure the input is a 'Copy as cURL (bash)' export")]
    CurlEmpty,

    /// The pasted cURL command was missing one or more required headers.
    #[error(
        "required headers missing: {0:?} — copy a request from music.youtube.com while logged in"
    )]
    CurlMissingHeaders(Vec<&'static str>),

    #[error("libmpv init failed: {0}")]
    Mpv(String),

    #[error("lyrics lookup failed: {0}")]
    Lyrics(#[from] lrclib::LrcError),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
