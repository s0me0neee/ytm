//! `cover://`: artwork fetched here, a few at a time, because the webview's burst got 429'd.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use tauri::http::{Request, Response, StatusCode};
use tauri::{UriSchemeResponder, Url};
use tokio::sync::Semaphore;
use ytm_core::cover::{FetchError, fetch_raw};

const MAX_IN_FLIGHT: usize = 6;

// Seconds, not milliseconds: a rate limit is not a blip.
const BACKOFF: &[Duration] = &[Duration::from_secs(1), Duration::from_secs(3)];

// A row cover is ~10KB and a Now Playing one ~300KB, so a few MB at most.
const MAX_CACHED: usize = 600;

// Anything else is refused, or this would be a proxy the page could point anywhere.
const ALLOWED_HOST_SUFFIXES: &[&str] = &[".googleusercontent.com", ".ytimg.com", ".ggpht.com"];

#[derive(Default)]
struct Cache {
    bytes: HashMap<String, Arc<Vec<u8>>>,
    order: VecDeque<String>,
}

fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(Mutex::default)
}

fn permits() -> &'static Semaphore {
    static PERMITS: Semaphore = Semaphore::const_new(MAX_IN_FLIGHT);
    &PERMITS
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
    if c.bytes.insert(url.to_string(), bytes).is_none() {
        c.order.push_back(url.to_string());
    }
    while c.order.len() > MAX_CACHED {
        if let Some(old) = c.order.pop_front() {
            c.bytes.remove(&old);
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

// A 404 is final (the frontend's size ladder steps down on it); 429 and 5xx may pass next time.
const fn worth_retrying(e: &FetchError) -> bool {
    match e {
        FetchError::Status(code) => *code == 429 || *code >= 500,
        FetchError::Other(_) => true,
    }
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
    // Held across the backoff too, so a rate limit slows every cover rather than just this one.
    let _permit = permits().acquire().await.map_err(|e| FetchError::Other(e.to_string()))?;
    let mut last = fetch_image(url).await;
    for wait in BACKOFF {
        match &last {
            Err(e) if worth_retrying(e) => {
                log::debug!("cover: {url} failed ({e:?}) — retrying in {wait:?}");
                tokio::time::sleep(*wait).await;
                last = fetch_image(url).await;
            }
            _ => break,
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
    fn a_truncated_jpeg_is_not_an_image() {
        assert_eq!(content_type(&[0xFF, 0xD8, 0x00, 0xFF, 0xD9]), Some("image/jpeg"));
        assert_eq!(content_type(&[0xFF, 0xD8, 0x00, 0x12]), None);
        assert_eq!(content_type(b"<html>429</html>"), None);
    }
}
