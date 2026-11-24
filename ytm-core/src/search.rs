//! YouTube Music search — policy over the raw InnerTube endpoint.
//!
//! `ytmusicapi 0.4.2` has no search of its own, but [`YTMusicClient::send_request`]
//! is public, so this is built on the client already in the graph: same cookies,
//! same context, no second HTTP stack. The parsing is deliberately
//! shape-tolerant — YouTube renames renderers without notice, and the first
//! version of this was written against `musicShelfRenderer` a week before the
//! unfiltered response started using `musicCardShelfRenderer` instead. Rather
//! than walk a fixed path, [`parse`] finds every result row wherever it is
//! nested and reads each row's own fields.
//!
//! ## Songs and videos are both fetched, on purpose
//!
//! YouTube Music types every result through `musicVideoType`:
//!
//! - `MUSIC_VIDEO_TYPE_ATV` — an *art track*: the label's catalogue audio,
//!   which YouTube wraps in the album cover. This is what the UI calls a Song.
//! - `MUSIC_VIDEO_TYPE_OMV` — an official music video.
//! - `MUSIC_VIDEO_TYPE_UGC` — a user upload.
//! - `MUSIC_VIDEO_TYPE_OFFICIAL_SOURCE_MUSIC` — an official non-art-track source.
//!
//! The songs filter returns only ATV — measured at 198 of 198 rows across ten
//! queries, see `examples/search_verify.rs`. That is the cleaner result in
//! every way: it carries the album, the release duration, and metadata good
//! enough for the lyrics matcher to settle on the first request.
//!
//! But plenty of music exists on YouTube *only* as a video — a self-released
//! track that was never distributed, a doujin upload, anything where the
//! "video" is a still image and the audio is the whole point. Searching songs
//! alone silently cannot find those, so [`search`] asks for both and labels
//! each row with its [`ResultKind`]. The UI marks them differently; the user
//! decides. Playback is unaffected either way — mpv is given `bestaudio` and
//! never fetches a video stream.

use std::sync::Arc;
use std::sync::mpsc::Sender;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use ytmusicapi::YTMusicClient;

use crate::browse::{Card, list_card};
use crate::error::Result;
use crate::library::Track;

/// The opaque `params` blobs YouTube Music's own UI sends to filter a search.
/// Verified against the live API; see the module docs.
const SONGS_FILTER: &str = "EgWKAQIIAWoKEAkQBRAKEAMQBA%3D%3D";
const VIDEOS_FILTER: &str = "EgWKAQIQAWoKEAkQChAFEAMQBA%3D%3D";
/// The same blob with the filter field swapped: 3 is albums, 4 artists. Measured against the live API.
const ALBUMS_FILTER: &str = "EgWKAQIYAWoKEAkQBRAKEAMQBA%3D%3D";
const ARTISTS_FILTER: &str = "EgWKAQIgAWoKEAkQBRAKEAMQBA%3D%3D";

/// How many results to keep per filter. YouTube returns 20 a page and the list
/// is a picker, not a catalogue — past this it is quicker to refine the query.
const PER_FILTER: usize = 20;

/// What a result *is*, reduced from `musicVideoType` to the distinction that
/// matters when choosing one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResultKind {
    /// An art track: catalogue audio with an album and a release duration.
    Song,
    /// A video — official or user-uploaded. Often still the only copy of a
    /// track that was never distributed as a song.
    Video,
}

impl ResultKind {
    /// The classification, or `None` for a row that is neither — a podcast
    /// episode, or a type YouTube has added since.
    #[must_use]
    pub fn from_video_type(video_type: &str) -> Option<Self> {
        match video_type {
            "MUSIC_VIDEO_TYPE_ATV" => Some(Self::Song),
            "MUSIC_VIDEO_TYPE_OMV"
            | "MUSIC_VIDEO_TYPE_UGC"
            | "MUSIC_VIDEO_TYPE_OFFICIAL_SOURCE_MUSIC" => Some(Self::Video),
            _ => None,
        }
    }

    /// The word the UI shows against a row.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Song => "song",
            Self::Video => "video",
        }
    }
}

/// One search hit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub video_id: String,
    pub title: String,
    /// Credited artist, or the uploading channel for a user upload.
    pub artist: String,
    /// Album, where the row carries one. Videos rarely do.
    pub album: String,
    /// As displayed — `"3:12"`. Empty when the row carries no duration.
    pub duration: String,
    /// The same, in seconds, for the lyrics matcher.
    pub duration_seconds: Option<u32>,
    pub kind: ResultKind,
    /// The raw `musicVideoType`, kept so a surprising classification can be
    /// explained without another request.
    pub video_type: String,
    /// Cover art URL, largest first. Empty when the row carries none.
    pub thumbnail: Option<String>,
    /// The credited artists with their channel ids, where the row links them.
    #[serde(default)]
    pub artists: Vec<ytmusicapi::Artist>,
    /// The album's browse id (`MPREb_…`), where the row links it.
    #[serde(default)]
    pub album_id: Option<String>,
}

/// The artists and the album a byline's runs link to.
pub(crate) fn links(runs: Option<&Value>) -> (Vec<ytmusicapi::Artist>, Option<String>) {
    let mut artists = Vec::new();
    let mut album = None;
    for run in runs.and_then(Value::as_array).into_iter().flatten() {
        let Some(id) = first_str(run, "browseId") else { continue };
        let name = run.get("text").and_then(Value::as_str).unwrap_or_default().trim();
        if id.starts_with("MPRE") {
            album = Some(id);
        } else if id.starts_with("UC") && !name.is_empty() {
            artists.push(ytmusicapi::Artist { name: name.to_string(), id: Some(id) });
        }
    }
    (artists, album)
}

impl SearchResult {
    /// A [`crate::library::Track`], so a search hit can be queued and played
    /// through exactly the same path as a library one.
    #[must_use]
    pub fn to_track(&self) -> crate::library::Track {
        let artists = if self.artists.is_empty() {
            self.artist
                .split(',')
                .map(|name| ytmusicapi::Artist {
                    name: name.trim().to_string(),
                    id: None,
                })
                .filter(|a| !a.name.is_empty())
                .collect()
        } else {
            self.artists.clone()
        };
        crate::library::Track {
            video_id: Some(self.video_id.clone()),
            title: Some(self.title.clone()),
            artists,
            album: (!self.album.is_empty()).then(|| ytmusicapi::Album {
                name: self.album.clone(),
                id: self.album_id.clone(),
            }),
            duration: (!self.duration.is_empty()).then(|| self.duration.clone()),
            duration_seconds: self.duration_seconds,
            thumbnail: self.thumbnail.clone(),
            set_video_id: None,
        }
    }
}

// ── JSON walking ─────────────────────────────────────────────────────────────

/// Every value under `key`, however deep.
///
/// Walking rather than pathing is what makes this survive YouTube's renderer
/// renames. Within a single result row it is also *unambiguous*: measured over
/// ~340 rows, no row ever carried two different `videoId`s or two different
/// `musicVideoType`s, so "the first hit inside this row" is always the row's
/// own. `examples/search_verify.rs` is the check.
pub(crate) fn find_all<'a>(v: &'a Value, key: &str, out: &mut Vec<&'a Value>) {
    match v {
        Value::Object(map) => {
            for (k, val) in map {
                if k == key {
                    out.push(val);
                }
                find_all(val, key, out);
            }
        }
        Value::Array(items) => items.iter().for_each(|i| find_all(i, key, out)),
        _ => {}
    }
}

pub(crate) fn first_str(v: &Value, key: &str) -> Option<String> {
    let mut hits = Vec::new();
    find_all(v, key, &mut hits);
    hits.first().and_then(|h| h.as_str()).map(str::to_string)
}

/// The text of each display column of a row, in order.
fn columns(item: &Value) -> Vec<String> {
    let mut arrays = Vec::new();
    find_all(item, "flexColumns", &mut arrays);
    arrays
        .iter()
        .filter_map(|a| a.as_array())
        .flatten()
        .filter_map(|column| {
            let mut runs = Vec::new();
            find_all(column, "runs", &mut runs);
            let text: String = runs
                .first()?
                .as_array()?
                .iter()
                .filter_map(|r| r.get("text").and_then(Value::as_str))
                .collect();
            let text = text.trim().to_string();
            (!text.is_empty()).then_some(text)
        })
        .collect()
}

/// The raw `runs` of a row's `n`th display column.
pub(crate) fn column_runs(item: &Value, n: usize) -> Option<&Value> {
    let mut arrays = Vec::new();
    find_all(item, "flexColumns", &mut arrays);
    let column = arrays.first()?.as_array()?.get(n)?;
    let mut runs = Vec::new();
    find_all(column, "runs", &mut runs);
    runs.first().copied()
}

/// `"3:12"` → 192. Also handles `"1:02:03"`.
#[must_use]
pub fn parse_duration(text: &str) -> Option<u32> {
    let parts: Vec<&str> = text.trim().split(':').collect();
    if !(2..=3).contains(&parts.len()) {
        return None;
    }
    let mut secs: u32 = 0;
    for part in &parts {
        let n: u32 = part.trim().parse().ok()?;
        secs = secs.checked_mul(60)?.checked_add(n)?;
    }
    Some(secs)
}

/// Whether a column segment looks like a duration rather than a view count or
/// an album name.
fn is_duration(text: &str) -> bool {
    parse_duration(text).is_some()
}

/// The largest cover URL on a row.
///
/// YouTube lists thumbnails smallest-first and every size is the same image, so
/// the last is the one worth fetching — a 60px crop looks like porridge once a
/// terminal scales it into a cell block.
pub(crate) fn thumbnail(item: &Value) -> Option<String> {
    let mut lists = Vec::new();
    find_all(item, "thumbnails", &mut lists);
    lists
        .iter()
        .filter_map(|l| l.as_array())
        .flatten()
        .filter_map(|t| {
            let url = t.get("url")?.as_str()?.to_string();
            let width = t.get("width").and_then(Value::as_u64).unwrap_or(0);
            Some((width, url))
        })
        .max_by_key(|(w, _)| *w)
        .map(|(_, url)| url)
}

/// Every playable row in a response, in the order YouTube returned them.
fn parse(response: &Value, limit: usize) -> Vec<SearchResult> {
    let mut items = Vec::new();
    find_all(response, "musicResponsiveListItemRenderer", &mut items);

    let mut out = Vec::new();
    for item in items {
        // No video id ⇒ an artist, album or playlist row: nothing to play.
        let Some(video_id) = first_str(item, "videoId") else {
            continue;
        };
        let video_type = first_str(item, "musicVideoType").unwrap_or_default();
        // A podcast episode is neither a song nor a video, and a type we don't
        // know is not one to guess at.
        let Some(kind) = ResultKind::from_video_type(&video_type) else {
            continue;
        };

        let cols = columns(item);
        let Some(title) = cols.first().cloned() else {
            continue;
        };

        // The second column is `artist • album • duration` for a song and
        // `channel • N views • duration` for a video — same shape, different
        // middle, and the duration is only sometimes present.
        let detail: Vec<String> = cols
            .get(1)
            .map(|s| s.split('•').map(|p| p.trim().to_string()).collect())
            .unwrap_or_default();
        let duration = detail
            .iter()
            .rev()
            .find(|p| is_duration(p))
            .cloned()
            .unwrap_or_default();
        // Whatever sits between the artist and the duration, when it isn't a
        // view count. Videos say "1.2M views"; songs name the album.
        let album = detail
            .get(1)
            .filter(|p| !is_duration(p) && !p.ends_with("views") && !p.ends_with("plays"))
            .cloned()
            .unwrap_or_default();
        let (artists, album_id) = links(column_runs(item, 1));

        out.push(SearchResult {
            video_id,
            title,
            artist: detail.first().cloned().unwrap_or_default(),
            album,
            duration_seconds: parse_duration(&duration),
            duration,
            kind,
            video_type,
            thumbnail: thumbnail(item),
            artists,
            album_id,
        });
        if out.len() >= limit {
            break;
        }
    }
    out
}

// ── the request ──────────────────────────────────────────────────────────────

async fn run_filter(yt: &YTMusicClient, query: &str, params: &str) -> Result<Vec<SearchResult>> {
    let body = json!({ "query": query, "params": params });
    let response = yt.send_request("search", body).await?;
    Ok(parse(&response, PER_FILTER))
}

/// Songs first, then any video that isn't already listed.
///
/// Two requests rather than one unfiltered search: the filtered responses are
/// uniform and carry the album and duration, while an unfiltered one mixes in
/// artists, playlists, podcasts and profiles — 15 of 32 rows on a measured
/// query had no video id at all.
pub async fn search(yt: &YTMusicClient, query: &str) -> Result<Vec<SearchResult>> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }

    let songs = run_filter(yt, query, SONGS_FILTER).await?;
    // A failed video search should not cost the songs that already arrived.
    let videos = match run_filter(yt, query, VIDEOS_FILTER).await {
        Ok(found) => found,
        Err(e) => {
            log::warn!("search: the video results failed ({e}) — songs only");
            Vec::new()
        }
    };

    let mut seen: std::collections::HashSet<String> =
        songs.iter().map(|r| r.video_id.clone()).collect();
    let mut out = songs;
    for video in videos {
        if seen.insert(video.video_id.clone()) {
            out.push(video);
        }
    }
    log::info!("search: {query:?} → {} results", out.len());
    Ok(out)
}

/// Everything a query finds, by kind: what a results page lays out as sections.
#[derive(Debug, Clone, Default, Serialize)]
pub struct SearchPage {
    pub songs: Vec<SearchResult>,
    pub videos: Vec<SearchResult>,
    pub albums: Vec<Card>,
    pub artists: Vec<Card>,
}

async fn run_cards(yt: &YTMusicClient, query: &str, params: &str) -> Result<Vec<Card>> {
    let body = json!({ "query": query, "params": params });
    let response = yt.send_request("search", body).await?;
    let mut rows = Vec::new();
    find_all(&response, "musicResponsiveListItemRenderer", &mut rows);
    Ok(rows.into_iter().filter_map(list_card).take(PER_FILTER).collect())
}

/// Songs, videos, albums and artists, sent together; one failing costs only its own section.
pub async fn search_page(yt: &YTMusicClient, query: &str) -> Result<SearchPage> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(SearchPage::default());
    }
    let (songs, videos, albums, artists) = tokio::join!(
        run_filter(yt, query, SONGS_FILTER),
        run_filter(yt, query, VIDEOS_FILTER),
        run_cards(yt, query, ALBUMS_FILTER),
        run_cards(yt, query, ARTISTS_FILTER),
    );
    if songs.is_err() && videos.is_err() && albums.is_err() && artists.is_err() {
        return songs.map(|_| SearchPage::default());
    }
    fn or_empty<T>(section: &str, r: Result<Vec<T>>) -> Vec<T> {
        r.unwrap_or_else(|e| {
            log::warn!("search: the {section} results failed ({e})");
            Vec::new()
        })
    }

    // As in `search`: a video the songs already list is the same track, and the type decides the section.
    let mut seen = std::collections::HashSet::new();
    let (songs, videos): (Vec<_>, Vec<_>) = or_empty("song", songs)
        .into_iter()
        .chain(or_empty("video", videos))
        .filter(|r| seen.insert(r.video_id.clone()))
        .partition(|r| r.kind == ResultKind::Song);
    let page = SearchPage {
        songs,
        videos,
        albums: or_empty("album", albums),
        artists: or_empty("artist", artists),
    };
    log::info!(
        "search: {query:?} → {} songs, {} videos, {} albums, {} artists",
        page.songs.len(),
        page.videos.len(),
        page.albums.len(),
        page.artists.len()
    );
    Ok(page)
}

/// What the search box offers while a query is being typed.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Suggestions {
    /// Completions of the query, to search for as they are.
    pub queries: Vec<String>,
    /// Artists and albums to go straight to.
    pub cards: Vec<Card>,
    /// Songs to play straight away.
    pub songs: Vec<SearchResult>,
}

/// A suggested song: its byline is `Song • artist • plays`, which `parse` would read "Song" out of as the artist.
fn suggested_song(item: &Value) -> Option<SearchResult> {
    let video_id = first_str(item, "videoId")?;
    let video_type = first_str(item, "musicVideoType").unwrap_or_default();
    let kind = ResultKind::from_video_type(&video_type)?;
    let cols = columns(item);
    let title = cols.first()?.clone();
    let (artists, _) = links(column_runs(item, 1));
    let (_, album_id) = links(column_runs(item, 2));
    let artist = if artists.is_empty() {
        cols.get(1)
            .and_then(|byline| byline.split('•').nth(1))
            .map(|a| a.trim().to_string())
            .unwrap_or_default()
    } else {
        artists.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ")
    };
    Some(SearchResult {
        video_id,
        title,
        artist,
        album: cols.get(2).cloned().unwrap_or_default(),
        duration: String::new(),
        duration_seconds: None,
        kind,
        video_type,
        thumbnail: thumbnail(item),
        artists,
        album_id,
    })
}

fn parse_suggestions(response: &Value) -> Suggestions {
    let mut texts = Vec::new();
    find_all(response, "searchSuggestionRenderer", &mut texts);
    let queries = texts
        .iter()
        .filter_map(|s| s.get("navigationEndpoint")?.get("searchEndpoint")?.get("query")?.as_str())
        .map(str::to_string)
        .collect();

    let mut rows = Vec::new();
    find_all(response, "musicResponsiveListItemRenderer", &mut rows);
    let mut out = Suggestions { queries, ..Suggestions::default() };
    for row in rows {
        if let Some(card) = list_card(row) {
            out.cards.push(card);
        } else if let Some(song) = suggested_song(row) {
            out.songs.push(song);
        }
    }
    out
}

/// YouTube Music's own suggestions for a partly typed query.
pub async fn suggestions(yt: &YTMusicClient, input: &str) -> Result<Suggestions> {
    let input = input.trim();
    if input.is_empty() {
        return Ok(Suggestions::default());
    }
    let response = yt.send_request("music/get_search_suggestions", json!({ "input": input })).await?;
    Ok(parse_suggestions(&response))
}

/// Adds a track to one of the user's playlists.
pub async fn add_to_playlist(yt: &YTMusicClient, playlist_id: &str, video_id: &str) -> Result<()> {
    yt.add_playlist_items(playlist_id, &[video_id.to_string()], false)
        .await?;
    Ok(())
}

/// Likes a track, which is what "add to Liked Music" means to the API.
pub async fn like(yt: &YTMusicClient, video_id: &str) -> Result<()> {
    yt.like_song(video_id).await?;
    Ok(())
}

/// Takes a like back, which also takes the track out of Liked Music.
pub async fn unlike(yt: &YTMusicClient, video_id: &str) -> Result<()> {
    yt.unlike_song(video_id).await?;
    Ok(())
}

/// Removes one row of a playlist, named by its `set_video_id`.
pub async fn remove_from_playlist(yt: &YTMusicClient, playlist_id: &str, track: &Track) -> Result<()> {
    let row = ytmusicapi::PlaylistTrack {
        video_id: track.video_id.clone(),
        title: track.title.clone(),
        artists: Vec::new(),
        album: None,
        duration: None,
        duration_seconds: None,
        thumbnails: Vec::new(),
        is_available: true,
        is_explicit: false,
        set_video_id: track.set_video_id.clone(),
        video_type: None,
    };
    yt.remove_playlist_items(playlist_id, &[row]).await?;
    Ok(())
}

/// Creates a private playlist and answers its id.
pub async fn create_playlist(yt: &YTMusicClient, title: &str) -> Result<String> {
    let created = yt
        .create_playlist(title, None, ytmusicapi::Privacy::Private)
        .await?;
    Ok(created.playlist_id)
}

// ── background work ──────────────────────────────────────────────────────────

impl SearchPage {
    /// The artist and the album the query names, if it names one — what a results list puts first.
    ///
    /// The GUI's `topResults` applies the same rule.
    #[must_use]
    pub fn named(&self, query: &str) -> Vec<&Card> {
        let q = normalise(query);
        let mut out = Vec::new();
        if let Some(artist) = self.artists.first() {
            let name = normalise(&artist.title);
            if q.chars().count() >= 3 && !name.is_empty() && (name.contains(&q) || q.contains(&name)) {
                out.push(artist);
            }
        }
        // Four letters at least, or every query containing "a" would name an album called "A".
        if let Some(album) = self.albums.iter().find(|a| {
            let title = normalise(&a.title);
            title.chars().count() >= 4 && q.contains(&title)
        }) {
            out.push(album);
        }
        out
    }
}

/// Lowercase letters and digits with single spaces, so "AC/DC" and "ac dc" compare equal.
fn normalise(text: &str) -> String {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// A change to the user's library.
#[derive(Debug, Clone)]
pub enum Edit {
    /// An empty `playlist_id` is Liked Music, which is the like button rather than a playlist.
    Add { playlist_id: String, video_id: String },
    Unlike { video_id: String },
    Remove { playlist_id: String, track: Track },
    /// A private playlist, with `video_id` in it when given.
    Create { title: String, video_id: Option<String> },
}

/// One [`Edit`], with what the caller needs to act on its answer.
pub struct EditRequest {
    pub edit: Edit,
    /// The library playlist the edit changes, so the answer says which one to fetch again.
    pub playlist: Option<usize>,
    /// The track's title and the playlist's, for what the answer has to say.
    pub title: String,
    pub where_to: String,
}

/// A finished search, or a finished edit.
pub enum SearchMsg {
    Results {
        query: String,
        result: std::result::Result<SearchPage, String>,
    },
    /// An edit that has landed or failed; `Ok(Some(id))` is a created playlist's id.
    Edited {
        edit: Edit,
        playlist: Option<usize>,
        title: String,
        where_to: String,
        result: std::result::Result<Option<String>, String>,
    },
}

pub fn spawn_search(
    handle: &tokio::runtime::Handle,
    yt: Arc<YTMusicClient>,
    query: String,
    tx: Sender<SearchMsg>,
) {
    handle.spawn(async move {
        let result = search_page(&yt, &query).await.map_err(|e| e.to_string());
        let _ = tx.send(SearchMsg::Results { query, result });
    });
}

async fn run_edit(yt: &YTMusicClient, edit: &Edit) -> Result<Option<String>> {
    match edit {
        Edit::Add { playlist_id, video_id } if playlist_id.is_empty() => like(yt, video_id).await?,
        Edit::Add { playlist_id, video_id } => add_to_playlist(yt, playlist_id, video_id).await?,
        Edit::Unlike { video_id } => unlike(yt, video_id).await?,
        Edit::Remove { playlist_id, track } => remove_from_playlist(yt, playlist_id, track).await?,
        Edit::Create { title, video_id } => {
            let id = create_playlist(yt, title).await?;
            if let Some(video_id) = video_id {
                add_to_playlist(yt, &id, video_id).await?;
            }
            return Ok(Some(id));
        }
    }
    Ok(None)
}

pub fn spawn_edit(
    handle: &tokio::runtime::Handle,
    yt: Arc<YTMusicClient>,
    request: EditRequest,
    tx: Sender<SearchMsg>,
) {
    handle.spawn(async move {
        let EditRequest { edit, playlist, title, where_to } = request;
        let result = run_edit(&yt, &edit).await.map_err(|e| e.to_string());
        let _ = tx.send(SearchMsg::Edited { edit, playlist, title, where_to, result });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_type_becomes_the_distinction_that_matters() {
        assert_eq!(
            ResultKind::from_video_type("MUSIC_VIDEO_TYPE_ATV"),
            Some(ResultKind::Song)
        );
        for video in [
            "MUSIC_VIDEO_TYPE_OMV",
            "MUSIC_VIDEO_TYPE_UGC",
            "MUSIC_VIDEO_TYPE_OFFICIAL_SOURCE_MUSIC",
        ] {
            assert_eq!(ResultKind::from_video_type(video), Some(ResultKind::Video));
        }
        // Neither, and not to be guessed at.
        assert_eq!(
            ResultKind::from_video_type("MUSIC_VIDEO_TYPE_PODCAST_EPISODE"),
            None
        );
        assert_eq!(ResultKind::from_video_type(""), None);
        assert_eq!(
            ResultKind::from_video_type("MUSIC_VIDEO_TYPE_SOMETHING_NEW"),
            None
        );
    }

    #[test]
    fn durations_parse_and_view_counts_do_not() {
        assert_eq!(parse_duration("3:12"), Some(192));
        assert_eq!(parse_duration("0:45"), Some(45));
        assert_eq!(parse_duration("1:02:03"), Some(3723));
        assert_eq!(parse_duration(" 5:55 "), Some(355));
        // The things that share the column with it.
        assert_eq!(parse_duration("815K plays"), None);
        assert_eq!(parse_duration("2B views"), None);
        assert_eq!(parse_duration("AREEL"), None);
        assert_eq!(parse_duration(""), None);
        assert_eq!(parse_duration("3:12:00:00"), None);
    }

    /// One row, shaped like the real thing: the fields this parser needs, at
    /// the depths YouTube puts them.
    fn column(runs: &Value) -> Value {
        json!({ "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": runs } } })
    }

    fn browse_row(id: &str, title: &str, detail: &Value) -> Value {
        json!({ "musicResponsiveListItemRenderer": {
            "navigationEndpoint": { "browseEndpoint": { "browseId": id } },
            "flexColumns": [ column(&json!([{ "text": title }])), column(detail) ]
        } })
    }

    // Shaped after a live `get_search_suggestions` reply for "the weekn".
    #[test]
    fn suggestions_split_into_queries_pages_and_songs() {
        let response = json!({ "contents": [
            { "searchSuggestionsSectionRenderer": { "contents": [
                { "searchSuggestionRenderer": {
                    "suggestion": { "runs": [ { "text": "the weekn" }, { "text": "d" } ] },
                    "navigationEndpoint": { "searchEndpoint": { "query": "the weeknd" } } } },
                { "searchSuggestionRenderer": {
                    "navigationEndpoint": { "searchEndpoint": { "query": "the weeknd the hills" } } } }
            ] } },
            { "searchSuggestionsSectionRenderer": { "contents": [
                browse_row("UCweeknd", "The Weeknd", &json!([
                    { "text": "Artist" }, { "text": " • " }, { "text": "238M monthly audience" } ])),
                { "musicResponsiveListItemRenderer": {
                    "playlistItemData": { "videoId": "GfiJowcJiVw" },
                    "flexColumns": [
                        column(&json!([{ "text": "The Hills" }])),
                        column(&json!([
                            { "text": "Song" }, { "text": " • " },
                            { "text": "The Weeknd", "navigationEndpoint": { "browseEndpoint": { "browseId": "UCweeknd" } } },
                            { "text": " • " }, { "text": "2.8B plays" } ])),
                        column(&json!([{ "text": "Beauty Behind The Madness",
                            "navigationEndpoint": { "browseEndpoint": { "browseId": "MPREbbtm" } } }]))
                    ],
                    "menu": { "musicVideoType": "MUSIC_VIDEO_TYPE_ATV" } } }
            ] } }
        ] });
        let s = parse_suggestions(&response);
        assert_eq!(s.queries, ["the weeknd", "the weeknd the hills"]);
        assert_eq!(s.cards.len(), 1);
        assert_eq!((s.cards[0].id.as_str(), s.cards[0].subtitle.as_str()), ("UCweeknd", "238M monthly audience"));
        assert_eq!(s.songs.len(), 1);
        let song = &s.songs[0];
        assert_eq!((song.title.as_str(), song.artist.as_str()), ("The Hills", "The Weeknd"));
        assert_eq!((song.album.as_str(), song.album_id.as_deref()), ("Beauty Behind The Madness", Some("MPREbbtm")));
    }

    #[test]
    fn an_album_row_keeps_its_byline_and_an_unknown_page_is_dropped() {
        let album = browse_row("MPREstarboy", "Starboy", &json!([
            { "text": "Album" }, { "text": " • " }, { "text": "The Weeknd" }, { "text": " • " }, { "text": "2016" } ]));
        let card = list_card(&album["musicResponsiveListItemRenderer"]).unwrap();
        assert_eq!(card.kind, crate::browse::CardKind::Album);
        assert_eq!(card.subtitle, "Album • The Weeknd • 2016");
        let playlist = browse_row("VLPL123", "Some playlist", &json!([{ "text": "Playlist" }]));
        assert!(list_card(&playlist["musicResponsiveListItemRenderer"]).is_none());
    }

    fn row(video_type: &str, title: &str, detail: &str) -> Value {
        json!({
            "musicResponsiveListItemRenderer": {
                "playlistItemData": { "videoId": "abc12345678" },
                "flexColumns": [
                    { "musicResponsiveListItemFlexColumnRenderer": {
                        "text": { "runs": [{ "text": title }] } } },
                    { "musicResponsiveListItemFlexColumnRenderer": {
                        "text": { "runs": [{ "text": detail }] } } }
                ],
                "thumbnail": { "musicThumbnailRenderer": { "thumbnail": { "thumbnails": [
                    { "url": "https://small", "width": 60, "height": 60 },
                    { "url": "https://large", "width": 544, "height": 544 }
                ] } } },
                "menu": { "menuRenderer": { "items": [ { "menuNavigationItemRenderer": {
                    "navigationEndpoint": { "watchEndpoint": {
                        "watchEndpointMusicSupportedConfigs": {
                            "watchEndpointMusicConfig": { "musicVideoType": video_type } } } } } } ] } }
            }
        })
    }

    #[test]
    fn a_song_row_yields_album_and_duration() {
        let found = parse(
            &row("MUSIC_VIDEO_TYPE_ATV", "typing", "ariiol • AREEL • 3:12"),
            20,
        );
        assert_eq!(found.len(), 1);
        let hit = &found[0];
        assert_eq!(hit.kind, ResultKind::Song);
        assert_eq!(hit.title, "typing");
        assert_eq!(hit.artist, "ariiol");
        assert_eq!(hit.album, "AREEL");
        assert_eq!(hit.duration, "3:12");
        assert_eq!(hit.duration_seconds, Some(192));
        // Largest, not first — a 60px crop is porridge once scaled up.
        assert_eq!(hit.thumbnail.as_deref(), Some("https://large"));
    }

    #[test]
    fn a_video_rows_view_count_is_not_mistaken_for_an_album() {
        let found = parse(
            &row(
                "MUSIC_VIDEO_TYPE_UGC",
                "typing",
                "some channel • 1.2M views • 3:14",
            ),
            20,
        );
        let hit = &found[0];
        assert_eq!(hit.kind, ResultKind::Video);
        assert_eq!(hit.artist, "some channel");
        assert_eq!(hit.album, "", "a view count is not an album");
        assert_eq!(hit.duration, "3:14");
    }

    #[test]
    fn a_row_with_no_duration_is_still_a_result() {
        let found = parse(&row("MUSIC_VIDEO_TYPE_OMV", "live set", "a channel"), 20);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].duration, "");
        assert_eq!(found[0].duration_seconds, None);
    }

    #[test]
    fn rows_that_are_not_music_are_dropped() {
        // A podcast episode, and an artist row with no video id at all.
        let episode = row("MUSIC_VIDEO_TYPE_PODCAST_EPISODE", "an episode", "a show");
        assert!(parse(&episode, 20).is_empty());

        let artist = json!({ "musicResponsiveListItemRenderer": {
            "flexColumns": [ { "musicResponsiveListItemFlexColumnRenderer": {
                "text": { "runs": [{ "text": "Queen" }] } } } ] } });
        assert!(parse(&artist, 20).is_empty());
    }

    #[test]
    fn the_result_count_is_bounded() {
        let many = json!(vec![
            row("MUSIC_VIDEO_TYPE_ATV", "x", "y • z • 1:00");
            50
        ]);
        assert_eq!(parse(&many, 20).len(), 20);
    }

    use crate::browse::CardKind;

    fn card(kind: CardKind, title: &str) -> Card {
        Card { kind, id: format!("id-{title}"), title: title.to_string(), subtitle: String::new(), thumbnail: None }
    }

    fn page(artists: &[&str], albums: &[&str]) -> SearchPage {
        SearchPage {
            artists: artists.iter().map(|t| card(CardKind::Artist, t)).collect(),
            albums: albums.iter().map(|t| card(CardKind::Album, t)).collect(),
            ..SearchPage::default()
        }
    }

    fn titles(cards: &[&Card]) -> Vec<String> {
        cards.iter().map(|c| c.title.clone()).collect()
    }

    #[test]
    fn a_query_naming_the_top_artist_puts_it_first() {
        let p = page(&["AC/DC"], &[]);
        assert_eq!(titles(&p.named("ac dc")), ["AC/DC"]);
        assert_eq!(titles(&p.named("acdc thunderstruck")), Vec::<String>::new());
        assert_eq!(titles(&p.named("ac/dc thunderstruck")), ["AC/DC"]);
    }

    #[test]
    fn a_short_query_names_no_artist() {
        assert_eq!(titles(&page(&["Abba"], &[]).named("ab")), Vec::<String>::new());
    }

    #[test]
    fn an_album_is_named_only_by_a_title_the_query_contains_whole() {
        let p = page(&[], &["A", "OK Computer", "Kid A"]);
        assert_eq!(titles(&p.named("radiohead ok computer")), ["OK Computer"]);
        // "A" is in every query with an "a" in it, and "Kid A" isn't in this one.
        assert_eq!(titles(&p.named("a day in the life")), Vec::<String>::new());
    }

    /// Hits the real API with the user's session.
    #[tokio::test]
    #[ignore = "network + a set-up session"]
    async fn live_a_search_page_finds_every_kind_and_suggests() {
        let yt = crate::Session::new().expect("session").build_client().expect("client");
        let page = search_page(&yt, "the weeknd").await.expect("searched");
        assert!(!page.songs.is_empty() && !page.videos.is_empty());
        assert!(page.artists.iter().any(|a| a.title == "The Weeknd"), "{:?}", page.artists);
        assert!(page.albums.iter().all(|a| a.kind == crate::browse::CardKind::Album));
        let s = suggestions(&yt, "the weekn").await.expect("suggested");
        assert!(s.queries.iter().any(|q| q == "the weeknd"), "{:?}", s.queries);
    }

    /// `cargo test -p ytm-core search -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "network + a set-up session"]
    async fn live_a_search_returns_both_kinds() {
        let yt = crate::Session::new()
            .expect("session")
            .build_client()
            .expect("client");
        let found = search(&yt, "ariiol typing").await.expect("searched");
        for hit in &found {
            eprintln!(
                "{:<6} {:<12} {}  ·  {}  ·  {}  ·  {}",
                hit.kind.label(),
                hit.video_id,
                hit.title,
                hit.artist,
                hit.album,
                hit.duration
            );
        }
        assert!(!found.is_empty());
        // Both kinds, since a track that only exists as a video is the reason
        // the video filter is asked for at all.
        assert!(found.iter().any(|h| h.kind == ResultKind::Song));
        assert!(found.iter().any(|h| h.kind == ResultKind::Video));
        // Every row is playable and captioned.
        for hit in &found {
            assert!(!hit.video_id.is_empty(), "{hit:?}");
            assert!(!hit.title.is_empty(), "{hit:?}");
        }
        // Songs carry a duration; that is what the lyrics matcher needs.
        let songs: Vec<_> = found
            .iter()
            .filter(|h| h.kind == ResultKind::Song)
            .collect();
        assert!(
            songs.iter().all(|h| h.duration_seconds.is_some()),
            "a song came back with no duration"
        );
        // And a cover to show.
        assert!(songs.iter().all(|h| h.thumbnail.is_some()));
    }

    #[test]
    fn a_rows_links_carry_artist_and_album_ids() {
        let mut item = row("MUSIC_VIDEO_TYPE_ATV", "Get Lucky", "");
        let link = |text: &str, id: &str| json!({ "text": text,
            "navigationEndpoint": { "browseEndpoint": { "browseId": id } } });
        item["musicResponsiveListItemRenderer"]["flexColumns"][1] = json!({
            "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [
                link("Daft Punk", "UCdaft"), { "text": " & " }, link("Nile Rodgers", "UCnile"),
                { "text": " • " }, link("Random Access Memories", "MPREb_ram"),
                { "text": " • " }, { "text": "6:10" }
            ] } }
        });
        let hit = &parse(&item, 20)[0];
        assert_eq!(hit.artists.len(), 2);
        assert_eq!(hit.artists[1].id.as_deref(), Some("UCnile"));
        assert_eq!(hit.album_id.as_deref(), Some("MPREb_ram"));
        let track = hit.to_track();
        assert_eq!(track.artist_names(), "Daft Punk, Nile Rodgers");
        assert_eq!(track.album.and_then(|a| a.id).as_deref(), Some("MPREb_ram"));
    }

    #[test]
    fn a_result_converts_to_a_playable_track() {
        let found = parse(
            &row(
                "MUSIC_VIDEO_TYPE_ATV",
                "typing",
                "ariiol, Kaai Yuki • AREEL • 3:12",
            ),
            20,
        );
        let track = found[0].to_track();
        assert_eq!(track.video_id.as_deref(), Some("abc12345678"));
        assert_eq!(track.title.as_deref(), Some("typing"));
        assert_eq!(track.artist_names(), "ariiol, Kaai Yuki");
        assert_eq!(track.duration_seconds, Some(192));
        assert_eq!(track.album.map(|a| a.name).as_deref(), Some("AREEL"));
    }
}
