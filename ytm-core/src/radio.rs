//! Radio: an endless "up next" seeded from one track.
//!
//! `ytmusicapi 0.5` has no `get_watch_playlist`, so this is the same move
//! [`crate::search`] makes: YouTube Music's own `next` endpoint, through
//! [`YTMusicClient::send_request`], so there are no new cookies, context or
//! HTTP stack. The web client asks for a radio by naming the seed's
//! auto-generated mix, `RDAMVM<videoId>`, and gets back the first page of it
//! plus a continuation token for the next.
//!
//! This module is only the *source* of a station. What a frontend does with the
//! tracks — which synthetic playlist they are filed under, when the queue asks
//! for more — goes through [`crate::Library::place_off_library`] and
//! [`crate::Player::remaining`], which both frontends already share.
//!
//! Parsing walks for `playlistPanelVideoRenderer` rows rather than pathing to
//! them, for the reason `search` gives: the queue panel has been served under
//! at least two wrappers (`playlistPanelVideoWrapperRenderer` around some rows)
//! and a continuation page nests it under a different root altogether.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::mpsc::Sender;

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

/// One queue row as a track, or `None` for a row there is nothing to play in.
fn parse_row(row: &Value) -> Option<Track> {
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

    let hit = SearchResult {
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
    };
    Some(hit.to_track())
}

/// Every playable row in a `next` response, in order, without repeats, and the
/// continuation for the page after it.
///
/// `skip` answers for what the caller already has — the seed, and whatever
/// earlier pages delivered. YouTube's first page leads with the seed itself, and a
/// continuation sometimes re-serves the row it broke on.
#[must_use]
pub fn parse(response: &Value, skip: impl Fn(&str) -> bool) -> Page {
    let mut rows = Vec::new();
    find_all(response, "playlistPanelVideoRenderer", &mut rows);

    let mut seen = HashSet::new();
    let tracks = rows
        .into_iter()
        .filter_map(parse_row)
        .filter(|t| {
            t.video_id
                .as_deref()
                .is_some_and(|id| !skip(id) && seen.insert(id.to_string()))
        })
        .collect();

    let mut tokens = Vec::new();
    find_all(response, "nextRadioContinuationData", &mut tokens);
    find_all(response, "nextContinuationData", &mut tokens);
    let continuation = tokens
        .iter()
        .find_map(|t| t.get("continuation").and_then(Value::as_str))
        .map(str::to_string);

    Page {
        tracks,
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
        assert!(page.continuation.is_some());
    }
}
