//! Radio: an endless "up next" seeded from one track.
//!
//! `ytmusicapi 0.5` has no `get_watch_playlist`, so this is the same move
//! [`crate::search`] makes: YouTube Music's own `next` endpoint, through
//! [`YTMusicClient::send_request`], so there are no new cookies, context or
//! HTTP stack. The web client asks for a radio by naming the seed's
//! auto-generated mix, `RDAMVM<videoId>`, and gets back the first page of it
//! plus a continuation token for the next.
//!
//! The source of a station, and its state: [`Station`] and the rules over it
//! ([`begin_refill`], [`accept_page`]) decide when to ask for more, so both
//! frontends top the queue up the same way. Where the tracks go is
//! [`crate::Library::place_off_library`] and [`crate::Player::append_many`].
//!
//! Parsing walks for queue slots rather than pathing to them, for the reason
//! `search` gives: a continuation page nests them under a different root. A slot
//! is a bare `playlistPanelVideoRenderer`, or a `playlistPanelVideoWrapperRenderer`
//! holding the music video *and* its `counterpart`, the art track of the same
//! song — two video ids for one entry, which is the web client's song/video
//! toggle. Measured, most slots of a station are wrapped, so taking every
//! renderer queued nearly every song twice. [`parse`] takes one per slot.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use ytmusicapi::YTMusicClient;

use crate::error::Result;
use crate::library::Track;
use crate::search::{ResultKind, SearchResult, find_all, first_str, parse_duration, thumbnail};

/// The `params` blob the web client sends with a radio request. Without it
/// the endpoint answers with the seed's own watch page and no mix.
const RADIO_PARAMS: &str = "wAEB";

/// Ask for more once the queue has this few entries left after the one
/// playing. Two is enough headroom for a request to land before the last
/// track ends, even over a slow link, without fetching pages nobody reaches.
pub const REFILL_BELOW: usize = 2;

/// Whether a station with `remaining` entries left should fetch its next page.
#[must_use]
pub fn needs_refill(remaining: usize) -> bool {
    remaining < REFILL_BELOW
}

/// One page of a station.
#[derive(Debug, Clone, Default)]
pub struct Page {
    pub tracks: Vec<Track>,
    /// The versions of those tracks not taken — a wrapped slot's other video —
    /// so a later page serving one cannot queue the song again.
    pub alternates: Vec<String>,
    /// Where the next page starts. `None` once YouTube has nothing more, which
    /// in practice is never for a radio but is for a finite mix.
    pub continuation: Option<String>,
}

/// The mix id YouTube Music generates for a seed.
#[must_use]
pub fn mix_id(seed_video_id: &str) -> String {
    format!("RDAMVM{seed_video_id}")
}

/// The request body for the first page (`continuation: None`) or a later one.
#[must_use]
pub fn request_body(seed_video_id: &str, continuation: Option<&str>) -> Value {
    let mut body = json!({
        "enablePersistentPlaylistPanel": true,
        "isAudioOnly": true,
        "tunerSettingValue": "AUTOMIX_SETTING_NORMAL",
        "videoId": seed_video_id,
        "playlistId": mix_id(seed_video_id),
        "params": RADIO_PARAMS,
    });
    if let (Some(token), Value::Object(map)) = (continuation, &mut body) {
        map.insert("continuation".into(), Value::String(token.to_string()));
    }
    body
}

/// Concatenated `runs[*].text` of a text object, or its `simpleText`.
fn text_of(v: Option<&Value>) -> Option<String> {
    let v = v?;
    if let Some(s) = v.get("simpleText").and_then(Value::as_str) {
        return Some(s.trim().to_string());
    }
    let text: String = v
        .get("runs")?
        .as_array()?
        .iter()
        .filter_map(|r| r.get("text").and_then(Value::as_str))
        .collect();
    let text = text.trim().to_string();
    (!text.is_empty()).then_some(text)
}

/// A byline segment that is a year or a count rather than an album name.
fn is_not_album(part: &str) -> bool {
    part.chars().all(|c| c.is_ascii_digit())
        || part.ends_with("views")
        || part.ends_with("plays")
        || part.ends_with("likes")
        || parse_duration(part).is_some()
}

/// One queue row, or `None` for a row there is nothing to play in.
fn parse_row(row: &Value) -> Option<SearchResult> {
    // A greyed-out row (region-locked, taken down) still has a video id.
    if row.get("unplayableText").is_some() {
        return None;
    }
    let video_id = row
        .get("videoId")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| first_str(row, "videoId"))?;
    let title = text_of(row.get("title"))?;
    // `artist • album • year` for a song, `channel • N views` for a video.
    let byline = text_of(
        row.get("longBylineText")
            .or_else(|| row.get("shortBylineText")),
    )
    .unwrap_or_default();
    let detail: Vec<&str> = byline.split('•').map(str::trim).collect();
    let artist = detail.first().copied().unwrap_or_default().to_string();
    let album = detail
        .get(1)
        .filter(|p| !is_not_album(p))
        .map(|p| (*p).to_string())
        .unwrap_or_default();
    let duration = text_of(row.get("lengthText")).unwrap_or_default();
    let video_type = first_str(row, "musicVideoType").unwrap_or_default();

    Some(SearchResult {
        video_id,
        title,
        artist,
        album,
        duration_seconds: parse_duration(&duration),
        duration,
        // A radio is a list of things to *play*, so a type this doesn't
        // recognise is played rather than dropped; the kind only matters to a
        // picker, which a station never shows.
        kind: ResultKind::from_video_type(&video_type).unwrap_or(ResultKind::Video),
        video_type,
        thumbnail: thumbnail(row),
    })
}

/// Every queue slot in `v`, in order, each as the renderers of its versions.
fn find_slots<'a>(v: &'a Value, out: &mut Vec<Vec<&'a Value>>) {
    match v {
        Value::Object(map) => {
            for (k, val) in map {
                match k.as_str() {
                    "playlistPanelVideoRenderer" => out.push(vec![val]),
                    "playlistPanelVideoWrapperRenderer" => {
                        let mut versions = Vec::new();
                        find_all(val, "playlistPanelVideoRenderer", &mut versions);
                        out.push(versions);
                    }
                    _ => find_slots(val, out),
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|i| find_slots(i, out)),
        _ => {}
    }
}

/// The version of a slot to queue: the song where there is one, since an art
/// track has a real album and the release length lyrics are matched against.
fn pick(versions: &[SearchResult]) -> Option<&SearchResult> {
    versions
        .iter()
        .find(|v| v.kind == ResultKind::Song)
        .or_else(|| versions.first())
}

/// One playable track per slot in a `next` response, in order, without
/// repeats, and the continuation for the page after it.
///
/// `skip` answers for what the caller already has — the seed, and whatever
/// earlier pages delivered. A slot is dropped when *any* of its versions is
/// skipped: YouTube's first page leads with the seed, which may be served as
/// the other version of the one that was played, and a continuation sometimes
/// re-serves the row it broke on.
#[must_use]
pub fn parse(response: &Value, skip: impl Fn(&str) -> bool) -> Page {
    let mut slots = Vec::new();
    find_slots(response, &mut slots);

    let mut seen = HashSet::new();
    let mut tracks = Vec::new();
    let mut alternates = Vec::new();
    for slot in slots {
        let versions: Vec<SearchResult> = slot.into_iter().filter_map(parse_row).collect();
        if versions
            .iter()
            .any(|v| skip(&v.video_id) || seen.contains(&v.video_id))
        {
            continue;
        }
        let Some(chosen) = pick(&versions) else {
            continue;
        };
        for v in &versions {
            seen.insert(v.video_id.clone());
            if v.video_id != chosen.video_id {
                alternates.push(v.video_id.clone());
            }
        }
        tracks.push(chosen.to_track());
    }

    let mut tokens = Vec::new();
    find_all(response, "nextRadioContinuationData", &mut tokens);
    find_all(response, "nextContinuationData", &mut tokens);
    let continuation = tokens
        .iter()
        .find_map(|t| t.get("continuation").and_then(Value::as_str))
        .map(str::to_string);

    Page {
        tracks,
        alternates,
        continuation,
    }
}

/// Fetches one page of `seed`'s station.
pub async fn fetch(
    yt: &YTMusicClient,
    seed_video_id: &str,
    continuation: Option<&str>,
    skip: impl Fn(&str) -> bool,
) -> Result<Page> {
    let body = request_body(seed_video_id, continuation);
    let response = yt.send_request("next", body).await?;
    let page = parse(&response, |id| id == seed_video_id || skip(id));
    log::info!(
        "radio: {seed_video_id} → {} tracks (more: {})",
        page.tracks.len(),
        page.continuation.is_some()
    );
    Ok(page)
}

/// A page that has landed, for the frontend's drain loop.
pub struct RadioMsg {
    pub seed: String,
    pub result: std::result::Result<Page, String>,
}

/// [`fetch`] on the runtime, answered over `tx` — the shape every other
/// background job here has, so a frontend drains it beside the rest. `skip` is
/// the video ids already queued, owned so the task can outlive the caller.
pub fn spawn_fetch(
    handle: &tokio::runtime::Handle,
    yt: Arc<YTMusicClient>,
    seed: String,
    continuation: Option<String>,
    skip: HashSet<String, impl std::hash::BuildHasher + Send + Sync + 'static>,
    tx: Sender<RadioMsg>,
) {
    handle.spawn(async move {
        let result = fetch(&yt, &seed, continuation.as_deref(), |id| skip.contains(id))
            .await
            .map_err(|e| e.to_string());
        let _ = tx.send(RadioMsg { seed, result });
    });
}

/// After a page fails, how long to wait before asking again.
const RETRY_AFTER: Duration = Duration::from_secs(10);

/// Failures in a row that end a station, so a dead link is not asked forever.
const MAX_FAILURES: u32 = 3;

/// A station in progress, shared in shape by both frontends. Plain data: the
/// rules over it are the free functions below, so neither frontend has its own
/// copy of when to ask for more.
#[derive(Debug, Clone)]
pub struct Station {
    pub seed: String,
    pub seed_title: String,
    pub continuation: Option<String>,
    /// Every video id the station has queued, the seed included — the `skip`
    /// for the next page, and the test for whether the station still plays.
    pub seen: HashSet<String>,
    pub fetching: bool,
    /// YouTube had no next page, or stopped answering.
    pub ended: bool,
    pub pages: u32,
    failures: u32,
    retry_at: Option<Instant>,
}

/// What to ask [`fetch`] for, taken out of a [`Station`] by [`begin_refill`].
pub struct Request {
    pub seed: String,
    pub continuation: Option<String>,
    pub skip: HashSet<String>,
}

#[must_use]
pub fn new_station(seed_video_id: &str, seed_title: &str) -> Station {
    Station {
        seed: seed_video_id.to_string(),
        seed_title: seed_title.to_string(),
        continuation: None,
        seen: HashSet::from([seed_video_id.to_string()]),
        fetching: false,
        ended: false,
        pages: 0,
        failures: 0,
        retry_at: None,
    }
}

/// Whether the station is still what is playing. Anything played from
/// elsewhere replaces the queue, and the station stops with it.
#[must_use]
pub fn is_live(station: &Station, playing_video_id: Option<&str>) -> bool {
    playing_video_id.is_some_and(|id| station.seen.contains(id))
}

/// The next page to fetch, if one is due — marking it in flight, so a caller
/// that asks every tick sends one request rather than one per tick.
pub fn begin_refill(
    station: &mut Station,
    playing_video_id: Option<&str>,
    remaining: usize,
    now: Instant,
) -> Option<Request> {
    let due = !station.fetching
        && !station.ended
        && station.retry_at.is_none_or(|at| now >= at)
        && is_live(station, playing_video_id)
        && needs_refill(remaining);
    if !due {
        return None;
    }
    station.fetching = true;
    Some(Request {
        seed: station.seed.clone(),
        continuation: station.continuation.clone(),
        skip: station.seen.clone(),
    })
}

/// Records a page that landed and returns the tracks in it the station has
/// not queued yet. A page with none ends the station: asking again past it
/// would only page through repeats.
pub fn accept_page(station: &mut Station, page: Page) -> Vec<Track> {
    station.fetching = false;
    station.failures = 0;
    station.retry_at = None;
    station.pages += 1;
    station.seen.extend(page.alternates);
    let fresh: Vec<Track> = page
        .tracks
        .into_iter()
        .filter(|t| {
            t.video_id
                .as_ref()
                .is_some_and(|id| station.seen.insert(id.clone()))
        })
        .collect();
    station.ended = page.continuation.is_none() || fresh.is_empty();
    station.continuation = page.continuation;
    fresh
}

/// Records a page that failed. Answers whether that ended the station.
pub fn page_failed(station: &mut Station, now: Instant) -> bool {
    station.fetching = false;
    station.failures += 1;
    station.retry_at = Some(now + RETRY_AFTER);
    station.ended = station.failures >= MAX_FAILURES;
    station.ended
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, title: &str, byline: &str, len: &str, kind: &str) -> Value {
        json!({ "playlistPanelVideoRenderer": {
            "videoId": id,
            "title": { "runs": [{ "text": title }] },
            "longBylineText": { "runs": byline.split('|').map(|t| json!({ "text": t })).collect::<Vec<_>>() },
            "lengthText": { "runs": [{ "text": len }] },
            "thumbnail": { "thumbnails": [
                { "url": format!("https://i.ytimg.com/{id}/small"), "width": 60 },
                { "url": format!("https://i.ytimg.com/{id}/big"), "width": 544 },
            ]},
            "navigationEndpoint": { "watchEndpoint": { "videoId": id,
                "watchEndpointMusicSupportedConfigs": { "watchEndpointMusicConfig": {
                    "musicVideoType": kind }}}},
        }})
    }

    /// The first page's shape, as the web client receives it.
    fn first_page(rows: Vec<Value>) -> Value {
        json!({ "contents": { "singleColumnMusicWatchNextResultsRenderer": { "tabbedRenderer": {
        "watchNextTabbedResultsRenderer": { "tabs": [{ "tabRenderer": { "content": {
            "musicQueueRenderer": { "content": { "playlistPanelRenderer": {
                "contents": rows,
                "continuations": [{ "nextRadioContinuationData": { "continuation": "TOKEN2" } }],
            }}}}}}]}}}}})
    }

    fn ids(page: &Page) -> Vec<&str> {
        page.tracks
            .iter()
            .filter_map(|t| t.video_id.as_deref())
            .collect()
    }

    #[test]
    fn rows_become_tracks_with_their_metadata() {
        let resp = first_page(vec![row(
            "aaa",
            "Resonance",
            "HOME| • |Odyssey| • |2014",
            "3:32",
            "MUSIC_VIDEO_TYPE_ATV",
        )]);
        let page = parse(&resp, |_| false);
        let t = &page.tracks[0];
        assert_eq!(t.title.as_deref(), Some("Resonance"));
        assert_eq!(t.artist_names(), "HOME");
        assert_eq!(t.album.as_ref().map(|a| a.name.as_str()), Some("Odyssey"));
        assert_eq!(t.duration_seconds, Some(212));
        assert_eq!(t.thumbnail.as_deref(), Some("https://i.ytimg.com/aaa/big"));
        assert_eq!(page.continuation.as_deref(), Some("TOKEN2"));
    }

    #[test]
    fn the_seed_and_repeats_are_dropped() {
        let resp = first_page(vec![
            row("seed", "Seed", "A", "3:00", "MUSIC_VIDEO_TYPE_ATV"),
            row("bbb", "B", "A", "3:00", "MUSIC_VIDEO_TYPE_ATV"),
            row("bbb", "B", "A", "3:00", "MUSIC_VIDEO_TYPE_ATV"),
            row("ccc", "C", "A", "3:00", "MUSIC_VIDEO_TYPE_OMV"),
        ]);
        assert_eq!(ids(&parse(&resp, |id| id == "seed")), ["bbb", "ccc"]);
    }

    #[test]
    fn a_video_byline_names_no_album() {
        let resp = first_page(vec![row(
            "v",
            "Live",
            "Channel| • |1.2M views",
            "4:00",
            "MUSIC_VIDEO_TYPE_UGC",
        )]);
        let page = parse(&resp, |_| false);
        assert!(page.tracks[0].album.is_none());
        assert_eq!(page.tracks[0].artist_names(), "Channel");
    }

    #[test]
    fn an_unplayable_row_is_skipped() {
        let mut dead = row("dead", "Gone", "A", "3:00", "MUSIC_VIDEO_TYPE_ATV");
        dead["playlistPanelVideoRenderer"]["unplayableText"] = json!({ "runs": [{ "text": "x" }] });
        let resp = first_page(vec![
            dead,
            row("ok", "Ok", "A", "3:00", "MUSIC_VIDEO_TYPE_ATV"),
        ]);
        assert_eq!(ids(&parse(&resp, |_| false)), ["ok"]);
    }

    #[test]
    fn a_wrapped_row_and_a_continuation_page_are_both_found() {
        let resp = json!({ "continuationContents": { "playlistPanelContinuation": {
            "contents": [{ "playlistPanelVideoWrapperRenderer": { "primaryRenderer":
                row("deep", "Deep", "A", "3:00", "MUSIC_VIDEO_TYPE_ATV") } }],
            "continuations": [{ "nextContinuationData": { "continuation": "TOKEN3" } }],
        }}});
        let page = parse(&resp, |_| false);
        assert_eq!(ids(&page), ["deep"]);
        assert_eq!(page.continuation.as_deref(), Some("TOKEN3"));
    }

    #[test]
    fn the_request_names_the_mix_and_carries_a_continuation_only_when_given() {
        let first = request_body("abc", None);
        assert_eq!(first["playlistId"], "RDAMVMabc");
        assert!(first.get("continuation").is_none());
        assert_eq!(request_body("abc", Some("T"))["continuation"], "T");
    }

    #[test]
    fn a_refill_is_asked_for_before_the_queue_runs_dry() {
        assert!(needs_refill(0));
        assert!(needs_refill(REFILL_BELOW - 1));
        assert!(!needs_refill(REFILL_BELOW));
    }

    fn station_with(seed: &str) -> Station {
        new_station(seed, "Seed")
    }

    fn page_of(ids: &[&str], more: bool) -> Page {
        Page {
            tracks: ids
                .iter()
                .map(|id| Track {
                    video_id: Some((*id).to_string()),
                    title: None,
                    artists: Vec::new(),
                    album: None,
                    duration: None,
                    duration_seconds: None,
                    thumbnail: None,
                })
                .collect(),
            alternates: Vec::new(),
            continuation: more.then(|| "NEXT".to_string()),
        }
    }

    #[test]
    fn a_station_asks_once_and_only_while_it_is_playing() {
        let now = Instant::now();
        let mut st = station_with("seed");
        assert!(begin_refill(&mut st, Some("other"), 0, now).is_none());
        let req = begin_refill(&mut st, Some("seed"), 0, now).expect("first page");
        assert!(req.continuation.is_none() && req.skip.contains("seed"));
        // In flight: the next tick must not send a second request.
        assert!(begin_refill(&mut st, Some("seed"), 0, now).is_none());
    }

    #[test]
    fn a_page_queues_only_what_is_new_and_carries_its_continuation() {
        let now = Instant::now();
        let mut st = station_with("seed");
        let _ = begin_refill(&mut st, Some("seed"), 0, now);
        let fresh = accept_page(&mut st, page_of(&["seed", "a", "b"], true));
        assert_eq!(fresh.len(), 2);
        assert!(!st.ended && st.continuation.is_some());
        assert!(begin_refill(&mut st, Some("a"), REFILL_BELOW, now).is_none());
        let req = begin_refill(&mut st, Some("a"), 1, now).expect("refill");
        assert_eq!(req.continuation.as_deref(), Some("NEXT"));
    }

    #[test]
    fn a_station_ends_on_its_last_page_or_a_page_of_repeats() {
        let mut st = station_with("seed");
        let _ = accept_page(&mut st, page_of(&["a"], false));
        assert!(st.ended);
        let mut st = station_with("seed");
        let _ = accept_page(&mut st, page_of(&["seed"], true));
        assert!(st.ended);
    }

    #[test]
    fn failures_wait_before_retrying_and_end_the_station_eventually() {
        let now = Instant::now();
        let mut st = station_with("seed");
        let _ = begin_refill(&mut st, Some("seed"), 0, now);
        assert!(!page_failed(&mut st, now));
        assert!(begin_refill(&mut st, Some("seed"), 0, now).is_none());
        assert!(begin_refill(&mut st, Some("seed"), 0, now + RETRY_AFTER).is_some());
        assert!(!page_failed(&mut st, now));
        assert!(page_failed(&mut st, now));
    }

    /// A wrapped slot as the web client receives it: the video, and the song
    /// version of it as its counterpart.
    fn wrapped(video: Value, song: Value) -> Value {
        json!({ "playlistPanelVideoWrapperRenderer": {
            "primaryRenderer": video,
            "counterpart": [{ "counterpartRenderer": song }],
        }})
    }

    #[test]
    fn a_wrapped_slot_queues_its_song_once() {
        let resp = first_page(vec![
            wrapped(
                row("vid", "Sunflower (MV)", "A", "4:00", "MUSIC_VIDEO_TYPE_OMV"),
                row("song", "Sunflower", "A", "3:58", "MUSIC_VIDEO_TYPE_ATV"),
            ),
            row("bare", "Bare", "A", "3:00", "MUSIC_VIDEO_TYPE_OMV"),
        ]);
        let page = parse(&resp, |_| false);
        assert_eq!(ids(&page), ["song", "bare"]);
        assert_eq!(page.alternates, ["vid"]);
    }

    #[test]
    fn a_slot_is_skipped_when_either_version_is_already_had() {
        // The seed was played as the video; the mix leads with its song.
        let resp = first_page(vec![
            wrapped(
                row("seed", "Seed (MV)", "A", "4:00", "MUSIC_VIDEO_TYPE_OMV"),
                row("seed-song", "Seed", "A", "3:58", "MUSIC_VIDEO_TYPE_ATV"),
            ),
            row("next", "Next", "A", "3:00", "MUSIC_VIDEO_TYPE_ATV"),
        ]);
        assert_eq!(ids(&parse(&resp, |id| id == "seed")), ["next"]);
    }

    #[test]
    fn an_alternate_from_one_page_keeps_the_song_off_the_next() {
        let mut st = station_with("seed");
        let first = first_page(vec![wrapped(
            row("vid", "Song (MV)", "A", "4:00", "MUSIC_VIDEO_TYPE_OMV"),
            row("song", "Song", "A", "3:58", "MUSIC_VIDEO_TYPE_ATV"),
        )]);
        let page = parse(&first, |id| st.seen.contains(id));
        let _ = accept_page(&mut st, page);
        let again = first_page(vec![row(
            "vid",
            "Song (MV)",
            "A",
            "4:00",
            "MUSIC_VIDEO_TYPE_OMV",
        )]);
        assert!(parse(&again, |id| st.seen.contains(id)).tracks.is_empty());
    }

    /// The live endpoint. Needs a signed-in `browser.json`.
    #[tokio::test]
    #[ignore = "network: needs a live session"]
    async fn a_live_station_has_tracks_and_a_next_page() {
        let yt = crate::Session::new()
            .and_then(|s| s.build_client())
            .expect("session");
        // "Blinding Lights", an art track that will not be taken down.
        let page = fetch(&yt, "fHI8X4OXluQ", None, |_| false)
            .await
            .expect("fetch");
        assert!(page.tracks.len() > 5, "{} tracks", page.tracks.len());
        // Most slots are wrapped as video + song; one of each is queued.
        assert!(!page.alternates.is_empty());
        assert!(
            page.tracks
                .iter()
                .all(|t| !page.alternates.contains(t.video_id.as_ref().expect("id")))
        );
        let titles: Vec<_> = page
            .tracks
            .iter()
            .filter_map(|t| t.title.as_deref())
            .collect();
        eprintln!(
            "{} tracks, {} alternates: {titles:?}",
            titles.len(),
            page.alternates.len()
        );

        // The refill path: the next page, past everything the first delivered.
        let mut station = new_station("fHI8X4OXluQ", "Blinding Lights");
        let fresh = accept_page(&mut station, page);
        assert!(!station.ended);
        let next = fetch(&yt, "fHI8X4OXluQ", station.continuation.as_deref(), |id| {
            station.seen.contains(id)
        })
        .await
        .expect("second page");
        assert!(!next.tracks.is_empty(), "first page had {}", fresh.len());
    }
}
