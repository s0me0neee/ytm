//! `cover://`: artwork fetched here, a few at a time, because the webview's burst got 429'd.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use tauri::http::{Request, Response, StatusCode};
use tauri::{UriSchemeResponder, Url};
use tokio::sync::Semaphore;
use ytm_core::cover::{FetchError, fetch_raw};

/// Two lanes, because one queue put the cover somebody is looking at behind
/// every row thumbnail a playlist asked for: `WebKitGTK` does not defer
/// `loading="lazy"`, so opening a playlist queues ~170 small requests at once
/// and a Now Playing cover joined the back of them.
const ROW_PERMITS: usize = 6;
const LARGE_PERMITS: usize = 2;

/// Attempts per request. A large cover is the one on screen at full size, and
/// is worth waiting out a rate limit for -- the webview shows the small copy
/// underneath meanwhile, so a pending request costs nothing visible. A row
/// thumbnail that gives up just leaves its placeholder.
const LARGE_ATTEMPTS: usize = 6;
const ROW_ATTEMPTS: usize = 3;

/// A 429 with no `Retry-After` pauses every fetch for this long, doubling with
/// each 429 in a row up to [`MAX_PAUSE`]. Seconds, not milliseconds: a rate
/// limit is not a blip.
const BASE_PAUSE: Duration = Duration::from_secs(2);
const MAX_PAUSE: Duration = Duration::from_secs(30);

/// Backoff for a 5xx or a dropped connection -- a one-off, so per request.
const BLIP_BACKOFF: &[Duration] = &[Duration::from_secs(1), Duration::from_secs(3)];

/// Held in memory by total size rather than by count: rows are ~10KB and a Now
/// Playing cover ~300KB, so a count limit of 600 could mean anything from 6MB
/// to 180MB depending on which kind filled it.
const MAX_CACHED_BYTES: usize = 48 * 1024 * 1024;

// Anything else is refused, or this would be a proxy the page could point anywhere.
const ALLOWED_HOST_SUFFIXES: &[&str] = &[".googleusercontent.com", ".ytimg.com", ".ggpht.com"];

#[derive(Default)]
struct Cache {
    bytes: HashMap<String, Arc<Vec<u8>>>,
    order: VecDeque<String>,
    total: usize,
}

fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(Mutex::default)
}

fn lane(large: bool) -> &'static Semaphore {
    static ROWS: Semaphore = Semaphore::const_new(ROW_PERMITS);
    static LARGE: Semaphore = Semaphore::const_new(LARGE_PERMITS);
    if large { &LARGE } else { &ROWS }
}

/// Whether `url` asks for a picture big enough to belong in the large lane: a
/// named `YouTube` frame of 640px or more, or a Google image URL resized past
/// 400px. Everything the rows ask for is the API's own advertised URL, which
/// is neither.
fn is_large(url: &str) -> bool {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    if ["/sddefault.jpg", "/maxresdefault.jpg", "/hq720.jpg"]
        .iter()
        .any(|name| path.ends_with(name))
    {
        return true;
    }
    path.rsplit_once('=').is_some_and(|(_, params)| {
        params
            .split('-')
            .filter_map(|p| p.strip_prefix('w').or_else(|| p.strip_prefix('s')))
            .filter_map(|n| n.parse::<u32>().ok())
            .any(|px| px > 400)
    })
}

/// The one rate-limit state every fetch consults. A 429 is the CDN talking
/// about *us*, not about one URL, so the answer to it is that everything holds
/// off -- the old per-request backoff let the other hundred-odd queued fetches
/// keep hitting the limit while each one slept on its own.
#[derive(Default)]
struct Gate {
    /// Nothing is sent before this.
    until: Option<Instant>,
    /// 429s since the last success. Non-zero is also "recovering": requests go
    /// one at a time until something succeeds, rather than the whole queue
    /// stampeding the moment the pause ends and earning another 429.
    strikes: u32,
}

fn gate() -> &'static Mutex<Gate> {
    static GATE: OnceLock<Mutex<Gate>> = OnceLock::new();
    GATE.get_or_init(Mutex::default)
}

/// Waits out any pause in force. Loops because a strike can extend the pause
/// while this is asleep.
async fn pass_gate() {
    loop {
        let until = gate().lock().ok().and_then(|g| g.until);
        match until {
            Some(t) if t > Instant::now() => tokio::time::sleep_until(t.into()).await,
            _ => return,
        }
    }
}

fn probe() -> &'static Semaphore {
    static PROBE: Semaphore = Semaphore::const_new(1);
    &PROBE
}

fn recovering() -> bool {
    gate().lock().is_ok_and(|g| g.strikes > 0)
}

/// How long the `strikes`th 429 in a row pauses for, absent a `Retry-After`.
fn pause_for(strikes: u32) -> Duration {
    BASE_PAUSE
        .saturating_mul(1u32 << strikes.saturating_sub(1).min(5))
        .min(MAX_PAUSE)
}

fn strike(retry_after: Option<Duration>) {
    let Ok(mut g) = gate().lock() else { return };
    g.strikes = g.strikes.saturating_add(1);
    let pause = retry_after.unwrap_or_else(|| pause_for(g.strikes)).min(MAX_PAUSE);
    let Some(until) = Instant::now().checked_add(pause) else { return };
    g.until = Some(g.until.map_or(until, |t| t.max(until)));
    log::info!("cover: rate limited ({} in a row) — pausing all covers for {pause:?}", g.strikes);
}

fn all_clear() {
    if let Ok(mut g) = gate().lock()
        && g.strikes > 0
    {
        log::info!("cover: rate limit lifted");
        g.strikes = 0;
    }
}

// One lock per URL being fetched, so parallel requests for it wait and then hit the cache.
fn in_flight(url: &str) -> Arc<tokio::sync::Mutex<()>> {
    static IN_FLIGHT: OnceLock<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> = OnceLock::new();
    let map = IN_FLIGHT.get_or_init(Mutex::default);
    let Ok(mut map) = map.lock() else { return Arc::default() };
    map.retain(|_, lock| Arc::strong_count(lock) > 1);
    Arc::clone(map.entry(url.to_string()).or_default())
}

fn cached(url: &str) -> Option<Arc<Vec<u8>>> {
    cache().lock().ok()?.bytes.get(url).cloned()
}

fn remember(url: &str, bytes: Arc<Vec<u8>>) {
    let Ok(mut c) = cache().lock() else { return };
    let size = bytes.len();
    if let Some(old) = c.bytes.insert(url.to_string(), bytes) {
        c.total = c.total.saturating_sub(old.len());
    } else {
        c.order.push_back(url.to_string());
    }
    c.total = c.total.saturating_add(size);
    while c.total > MAX_CACHED_BYTES {
        let Some(old) = c.order.pop_front() else { break };
        if let Some(gone) = c.bytes.remove(&old) {
            c.total = c.total.saturating_sub(gone.len());
        }
    }
}

/// The cover URL a `cover://` request names — `convertFileSrc` percent-encodes it into the path.
fn target(request: &Request<Vec<u8>>) -> Option<String> {
    let path = request.uri().path().trim_start_matches('/');
    let raw = percent_encoding::percent_decode_str(path).decode_utf8().ok()?;
    // Parsed, not split: a `#` or `\` would otherwise hide the real host from the check.
    let url = Url::parse(&raw).ok()?;
    let host = url.host_str()?;
    (url.scheme() == "https" && ALLOWED_HOST_SUFFIXES.iter().any(|s| host.ends_with(s))).then(|| url.to_string())
}

const fn content_type(bytes: &[u8]) -> Option<&'static str> {
    match bytes {
        [0xFF, 0xD8, .., 0xFF, 0xD9] => Some("image/jpeg"),
        [0x89, b'P', b'N', b'G', ..] => Some("image/png"),
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => Some("image/webp"),
        _ => None,
    }
}

// A 200 whose body is cut short or isn't an image is retried, never cached.
async fn fetch_image(url: &str) -> Result<Vec<u8>, FetchError> {
    let bytes = fetch_raw(url).await?;
    if content_type(&bytes).is_none() {
        return Err(FetchError::Other("not a complete image".into()));
    }
    Ok(bytes)
}

async fn fetch(url: &str) -> Result<Arc<Vec<u8>>, FetchError> {
    if let Some(hit) = cached(url) {
        return Ok(hit);
    }
    let lock = in_flight(url);
    let _same_url = lock.lock().await;
    if let Some(hit) = cached(url) {
        return Ok(hit);
    }

    let large = is_large(url);
    let attempts = if large { LARGE_ATTEMPTS } else { ROW_ATTEMPTS };
    let mut blips = BLIP_BACKOFF.iter();
    let mut last = Err(FetchError::Other("never attempted".into()));
    for _ in 0..attempts {
        // Outside the permit: a paused request should not hold a slot another
        // lane's request could use the moment the pause ends.
        pass_gate().await;
        last = {
            let _permit = lane(large).acquire().await.map_err(|e| FetchError::Other(e.to_string()))?;
            // While recovering, one request at a time across both lanes -- the
            // probe that tells us the limit has lifted.
            let _probe = if recovering() { probe().acquire().await.ok() } else { None };
            // The pause may have been extended while queued for the permit.
            pass_gate().await;
            fetch_image(url).await
        };
        match &last {
            Ok(_) => {
                all_clear();
                break;
            }
            Err(FetchError::RateLimited(retry_after)) => strike(*retry_after),
            Err(FetchError::Status(code)) if *code >= 500 => match blips.next() {
                Some(wait) => tokio::time::sleep(*wait).await,
                None => break,
            },
            Err(FetchError::Other(e)) => {
                log::debug!("cover: {url} failed ({e})");
                match blips.next() {
                    Some(wait) => tokio::time::sleep(*wait).await,
                    None => break,
                }
            }
            // A 404 is final: the frontend's size ladder steps down on it.
            Err(FetchError::Status(_)) => break,
        }
    }
    let bytes = Arc::new(last?);
    remember(url, Arc::clone(&bytes));
    Ok(bytes)
}

fn status_only(status: StatusCode) -> Response<Vec<u8>> {
    let mut response = Response::new(Vec::new());
    *response.status_mut() = status;
    response
}

pub fn handle(request: &Request<Vec<u8>>, responder: UriSchemeResponder) {
    let Some(url) = target(request) else {
        responder.respond(status_only(StatusCode::FORBIDDEN));
        return;
    };
    tauri::async_runtime::spawn(async move {
        let response = match fetch(&url).await {
            Ok(bytes) => Response::builder()
                .header("Content-Type", content_type(&bytes).unwrap_or("image/jpeg"))
                // One URL names one picture at one size, so it never goes stale.
                .header("Cache-Control", "max-age=86400")
                .body(bytes.to_vec())
                .unwrap_or_else(|_| status_only(StatusCode::INTERNAL_SERVER_ERROR)),
            Err(FetchError::Status(code)) => status_only(StatusCode::from_u16(code).unwrap_or(StatusCode::BAD_GATEWAY)),
            Err(FetchError::RateLimited(_)) => status_only(StatusCode::TOO_MANY_REQUESTS),
            Err(FetchError::Other(e)) => {
                log::debug!("cover: {url} failed ({e})");
                status_only(StatusCode::BAD_GATEWAY)
            }
        };
        responder.respond(response);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(uri: &str) -> Request<Vec<u8>> {
        Request::builder().uri(uri).body(Vec::new()).unwrap()
    }

    #[test]
    fn a_cover_url_is_read_back_out_of_the_path() {
        let encoded = "https%3A%2F%2Fyt3.googleusercontent.com%2Fabc%3Dw120-h120-l90-rj";
        let want = Some("https://yt3.googleusercontent.com/abc=w120-h120-l90-rj".to_string());
        assert_eq!(target(&request(&format!("cover://localhost/{encoded}"))), want);
        assert_eq!(target(&request(&format!("http://cover.localhost/{encoded}"))), want);
    }

    #[test]
    fn only_artwork_hosts_are_fetched() {
        let yt = "cover://localhost/https%3A%2F%2Fi.ytimg.com%2Fvi%2Fx%2Fhqdefault.jpg%3Fsqp%3Dabc";
        assert!(target(&request(yt)).is_some());
        for bad in [
            "cover://localhost/https%3A%2F%2Fevil.example%2Fa.jpg",
            "cover://localhost/https%3A%2F%2Fytimg.com.evil.example%2Fa.jpg",
            "cover://localhost/http%3A%2F%2Fi.ytimg.com%2Fa.jpg",
            "cover://localhost/file%3A%2F%2F%2Fetc%2Fpasswd",
            // A fragment or a backslash must not hide the real host.
            "cover://localhost/https%3A%2F%2Fattacker.example%23.ytimg.com",
            "cover://localhost/https%3A%2F%2Fattacker.example%5C.ytimg.com%2Fa.jpg",
            "cover://localhost/https%3A%2F%2Fattacker.example%3F.ytimg.com",
        ] {
            assert_eq!(target(&request(bad)), None, "{bad}");
        }
    }

    #[test]
    fn a_now_playing_cover_rides_the_large_lane_and_a_row_does_not() {
        assert!(is_large("https://i.ytimg.com/vi/x/maxresdefault.jpg"));
        assert!(is_large("https://i.ytimg.com/vi/x/sddefault.jpg"));
        assert!(is_large("https://lh3.googleusercontent.com/abc=w1200-h1200-l90-rj"));
        assert!(!is_large("https://lh3.googleusercontent.com/abc=w120-h120-l90-rj"));
        assert!(!is_large("https://lh3.googleusercontent.com/abc=w320-h320-l90-rj"));
        assert!(!is_large("https://i.ytimg.com/vi/x/hqdefault.jpg?sqp=abc=w9999"));
        assert!(!is_large("https://yt3.ggpht.com/abc"));
    }

    #[test]
    fn a_pause_doubles_per_strike_and_stops_at_the_ceiling() {
        assert_eq!(pause_for(1), BASE_PAUSE);
        assert_eq!(pause_for(2), BASE_PAUSE * 2);
        assert_eq!(pause_for(3), BASE_PAUSE * 4);
        assert_eq!(pause_for(40), MAX_PAUSE);
    }

    #[test]
    fn the_cache_is_bounded_by_bytes_not_entries() {
        let big = Arc::new(vec![0u8; MAX_CACHED_BYTES / 4 + 1]);
        for i in 0..8 {
            remember(&format!("test://big/{i}"), Arc::clone(&big));
        }
        let (total, entries, order, newest) = {
            let c = cache().lock().unwrap();
            (c.total, c.bytes.len(), c.order.len(), c.bytes.contains_key("test://big/7"))
        };
        assert!(total <= MAX_CACHED_BYTES, "{total} bytes held");
        assert_eq!(entries, order);
        assert!(newest, "the newest is kept");
    }

    #[test]
    fn a_truncated_jpeg_is_not_an_image() {
        assert_eq!(content_type(&[0xFF, 0xD8, 0x00, 0xFF, 0xD9]), Some("image/jpeg"));
        assert_eq!(content_type(&[0xFF, 0xD8, 0x00, 0x12]), None);
        assert_eq!(content_type(b"<html>429</html>"), None);
    }
}
