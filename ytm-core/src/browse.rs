//! Album and artist pages, from the web client's own `browse` endpoint.
//!
//! `ytmusicapi 0.5` has neither, so this is `search.rs`'s approach again: one
//! `send_request`, then a walk for renderers rather than a fixed path.

use serde::Serialize;
use serde_json::{Value, json};
use ytmusicapi::{Album, Artist, YTMusicClient};

use crate::error::{Error, Result};
use crate::library::{self, Track};
use crate::search::{column_runs, find_all, first_str, links, parse_duration, thumbnail};

/// Most top songs an artist page lists; the songs playlist behind it runs to hundreds.
const MAX_TOP_SONGS: usize = 50;

#[derive(Debug, Clone, Serialize)]
pub struct AlbumPage {
    pub browse_id: String,
    pub title: String,
    /// `Album`, `Single` or `EP`, as the page labels itself.
    pub kind: String,
    pub year: Option<String>,
    pub artists: Vec<Artist>,
    pub thumbnail: Option<String>,
    pub tracks: Vec<Track>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArtistPage {
    pub channel_id: String,
    pub name: String,
    pub thumbnail: Option<String>,
    pub description: Option<String>,
    pub songs: Vec<Track>,
    pub shelves: Vec<Shelf>,
}

/// A row of cards: albums, singles, or related artists.
#[derive(Debug, Clone, Serialize)]
pub struct Shelf {
    pub title: String,
    pub cards: Vec<Card>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum CardKind {
    Album,
    Artist,
}

#[derive(Debug, Clone, Serialize)]
pub struct Card {
    pub kind: CardKind,
    /// A browse id: `MPREb_…` for an album, `UC…` for an artist.
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub thumbnail: Option<String>,
}

fn text(v: Option<&Value>) -> String {
    v.and_then(|v| v.get("runs"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|r| r.get("text").and_then(Value::as_str))
        .collect::<String>()
        .trim()
        .to_string()
}

fn first<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    let mut hits = Vec::new();
    find_all(v, key, &mut hits);
    hits.first().copied()
}

async fn browse(yt: &YTMusicClient, browse_id: &str) -> Result<Value> {
    Ok(yt.send_request("browse", json!({ "browseId": browse_id })).await?)
}

pub async fn album(yt: &YTMusicClient, browse_id: &str) -> Result<AlbumPage> {
    parse_album(&browse(yt, browse_id).await?, browse_id)
}

fn parse_album(response: &Value, browse_id: &str) -> Result<AlbumPage> {
    let header = first(response, "musicResponsiveHeaderRenderer")
        .ok_or_else(|| Error::Other("that album page has no header".to_string()))?;
    let title = text(header.get("title"));
    let subtitle = text(header.get("subtitle"));
    let mut parts = subtitle.split('•').map(str::trim);
    let kind = parts.next().unwrap_or("Album").to_string();
    let year = parts.find(|p| p.len() == 4 && p.chars().all(|c| c.is_ascii_digit())).map(str::to_string);
    let (artists, _) = links(header.get("straplineTextOne").and_then(|s| s.get("runs")));
    let cover = thumbnail(header);
    let album = Album { name: title.clone(), id: Some(browse_id.to_string()) };

    let mut rows = Vec::new();
    if let Some(shelf) = first(response, "musicShelfRenderer") {
        find_all(shelf, "musicResponsiveListItemRenderer", &mut rows);
    }
    let tracks = rows
        .into_iter()
        .filter_map(|row| {
            let video_id = first_str(row.get("playlistItemData")?, "videoId")?;
            let name = text(column_runs(row, 0).map(|r| json!({ "runs": r })).as_ref());
            let (by, _) = links(column_runs(row, 1));
            let duration = row
                .get("fixedColumns")
                .and_then(Value::as_array)
                .and_then(|c| c.first())
                .map(|c| text(first(c, "text")))
                .filter(|d| !d.is_empty());
            Some(Track {
                video_id: Some(video_id),
                title: (!name.is_empty()).then_some(name),
                artists: if by.is_empty() { artists.clone() } else { by },
                album: Some(album.clone()),
                duration_seconds: duration.as_deref().and_then(parse_duration),
                duration,
                thumbnail: cover.clone(),
                set_video_id: None,
            })
        })
        .collect();

    Ok(AlbumPage {
        browse_id: browse_id.to_string(),
        title,
        kind,
        year,
        artists,
        thumbnail: cover,
        tracks,
    })
}

pub async fn artist(yt: &YTMusicClient, channel_id: &str) -> Result<ArtistPage> {
    let response = browse(yt, channel_id).await?;
    let (mut page, songs_playlist) = parse_artist(&response, channel_id)?;
    // The shelf shows five; its "more" link is a playlist with the rest, and durations.
    if let Some(id) = songs_playlist
        && let Some(mut songs) = library::get_songs(yt, id.trim_start_matches("VL")).await
        && !songs.is_empty()
    {
        songs.truncate(MAX_TOP_SONGS);
        page.songs = songs;
    }
    Ok(page)
}

fn parse_artist(response: &Value, channel_id: &str) -> Result<(ArtistPage, Option<String>)> {
    let header = response
        .get("header")
        .and_then(|h| h.as_object()?.values().next())
        .ok_or_else(|| Error::Other("that artist page has no header".to_string()))?;
    let name = text(header.get("title"));
    let mut description = text(header.get("description"));

    let sections = first(response, "sectionListRenderer")
        .and_then(|list| list.get("contents"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut songs = Vec::new();
    let mut songs_playlist = None;
    let mut shelves = Vec::new();
    for section in &sections {
        if let Some(shelf) = section.get("musicShelfRenderer") {
            if !songs.is_empty() {
                continue;
            }
            songs_playlist = first(shelf, "bottomEndpoint")
                .and_then(|e| first_str(e, "browseId"))
                .filter(|id| id.starts_with("VL"));
            let mut rows = Vec::new();
            find_all(shelf, "musicResponsiveListItemRenderer", &mut rows);
            songs = rows.into_iter().filter_map(top_song).collect();
        } else if let Some(carousel) = section.get("musicCarouselShelfRenderer") {
            let title = text(first(carousel, "musicCarouselShelfBasicHeaderRenderer").and_then(|h| h.get("title")));
            let mut items = Vec::new();
            find_all(carousel, "musicTwoRowItemRenderer", &mut items);
            let cards: Vec<Card> = items.into_iter().filter_map(card).collect();
            if !cards.is_empty() {
                shelves.push(Shelf { title, cards });
            }
        } else if description.is_empty()
            && let Some(about) = section.get("musicDescriptionShelfRenderer")
        {
            description = text(about.get("description"));
        }
    }

    Ok((
        ArtistPage {
            channel_id: channel_id.to_string(),
            name,
            thumbnail: thumbnail(header),
            description: (!description.is_empty()).then_some(description),
            songs,
            shelves,
        },
        songs_playlist,
    ))
}

fn top_song(row: &Value) -> Option<Track> {
    let video_id = first_str(row.get("playlistItemData")?, "videoId")?;
    let title = text(column_runs(row, 0).map(|r| json!({ "runs": r })).as_ref());
    let (artists, _) = links(column_runs(row, 1));
    // The album sits in the last column; the one before it is a play count.
    let album = (2..5).rev().find_map(|n| {
        let runs = column_runs(row, n)?;
        let (_, id) = links(Some(runs));
        let name = text(Some(&json!({ "runs": runs })));
        id.map(|id| Album { name, id: Some(id) })
    });
    Some(Track {
        video_id: Some(video_id),
        title: (!title.is_empty()).then_some(title),
        artists,
        album,
        duration: None,
        duration_seconds: None,
        thumbnail: thumbnail(row),
        set_video_id: None,
    })
}

fn card(item: &Value) -> Option<Card> {
    let endpoint = item.get("navigationEndpoint")?.get("browseEndpoint")?;
    let id = endpoint.get("browseId")?.as_str()?.to_string();
    let kind = if id.starts_with("MPRE") {
        CardKind::Album
    } else if id.starts_with("UC") {
        CardKind::Artist
    } else {
        return None;
    };
    Some(Card {
        kind,
        id,
        title: text(item.get("title")),
        subtitle: text(item.get("subtitle")),
        thumbnail: thumbnail(item.get("thumbnailRenderer")?),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runs(parts: &[(&str, Option<&str>)]) -> Value {
        json!(parts
            .iter()
            .map(|(text, id)| match id {
                Some(id) => json!({ "text": text, "navigationEndpoint": { "browseEndpoint": { "browseId": id } } }),
                None => json!({ "text": text }),
            })
            .collect::<Vec<_>>())
    }

    fn column(r: &Value) -> Value {
        json!({ "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": r } } })
    }

    fn thumbs(url: &str) -> Value {
        json!({ "musicThumbnailRenderer": { "thumbnail": { "thumbnails": [ { "url": url, "width": 544 } ] } } })
    }

    #[test]
    fn an_album_page_yields_its_header_and_tracks() {
        let page = json!({
            "contents": { "twoColumnBrowseResultsRenderer": {
                "tabs": [ { "tabRenderer": { "content": { "sectionListRenderer": { "contents": [
                    { "musicResponsiveHeaderRenderer": {
                        "title": { "runs": [ { "text": "Random Access Memories" } ] },
                        "subtitle": { "runs": [ { "text": "Album" }, { "text": " • " }, { "text": "2013" } ] },
                        "straplineTextOne": { "runs": runs(&[("Daft Punk", Some("UCdaft"))]) },
                        "thumbnail": thumbs("https://cover")
                    } } ] } } } } ],
                "secondaryContents": { "sectionListRenderer": { "contents": [ { "musicShelfRenderer": { "contents": [
                    { "musicResponsiveListItemRenderer": {
                        "playlistItemData": { "videoId": "v1" },
                        "flexColumns": [
                            column(&runs(&[("Give Life Back to Music", None)])),
                            column(&runs(&[("Daft Punk", Some("UCdaft"))]))
                        ],
                        "fixedColumns": [ { "musicResponsiveListItemFixedColumnRenderer": {
                            "text": { "runs": [ { "text": "4:36" } ] } } } ]
                    } },
                    { "musicResponsiveListItemRenderer": {
                        "playlistItemData": { "videoId": "v2" },
                        "flexColumns": [
                            column(&runs(&[("Get Lucky", None)])),
                            column(&runs(&[("Daft Punk", Some("UCdaft")), (", ", None), ("Pharrell Williams", Some("UCpharrell"))]))
                        ]
                    } }
                ] } } ] } }
            } }
        });
        let album = parse_album(&page, "MPREb_ram").unwrap();
        assert_eq!(album.title, "Random Access Memories");
        assert_eq!(album.kind, "Album");
        assert_eq!(album.year.as_deref(), Some("2013"));
        assert_eq!(album.artists[0].id.as_deref(), Some("UCdaft"));
        assert_eq!(album.tracks.len(), 2);
        let first = &album.tracks[0];
        assert_eq!(first.duration_seconds, Some(276));
        assert_eq!(first.thumbnail.as_deref(), Some("https://cover"));
        assert_eq!(first.album.as_ref().and_then(|a| a.id.as_deref()), Some("MPREb_ram"));
        assert_eq!(album.tracks[1].artist_names(), "Daft Punk, Pharrell Williams");
    }

    #[test]
    fn an_artist_page_yields_songs_and_card_shelves() {
        let song = json!({ "musicResponsiveListItemRenderer": {
            "playlistItemData": { "videoId": "v1" },
            "thumbnail": thumbs("https://song"),
            "flexColumns": [
                column(&runs(&[("Instant Crush", None)])),
                column(&runs(&[("Daft Punk", Some("UCdaft"))])),
                column(&runs(&[("1.2B plays", None)])),
                column(&runs(&[("Random Access Memories", Some("MPREb_ram"))]))
            ]
        } });
        let two_row = |id: &str, title: &str| json!({ "musicTwoRowItemRenderer": {
            "title": { "runs": [ { "text": title } ] },
            "subtitle": { "runs": [ { "text": "2013" } ] },
            "navigationEndpoint": { "browseEndpoint": { "browseId": id } },
            "thumbnailRenderer": thumbs("https://card")
        } });
        let page = json!({
            "header": { "musicImmersiveHeaderRenderer": {
                "title": { "runs": [ { "text": "Daft Punk" } ] },
                "description": { "runs": [ { "text": "A French duo." } ] },
                "thumbnail": thumbs("https://artist")
            } },
            "contents": { "singleColumnBrowseResultsRenderer": { "tabs": [ { "tabRenderer": { "content": {
                "sectionListRenderer": { "contents": [
                    { "musicShelfRenderer": {
                        "contents": [ song ],
                        "bottomEndpoint": { "browseEndpoint": { "browseId": "VLOLAK5uy_songs" } }
                    } },
                    { "musicCarouselShelfRenderer": {
                        "header": { "musicCarouselShelfBasicHeaderRenderer": { "title": { "runs": [ { "text": "Albums" } ] } } },
                        "contents": [ two_row("MPREb_ram", "Random Access Memories"), two_row("VLPLplaylist", "A playlist") ]
                    } },
                    { "musicCarouselShelfRenderer": {
                        "header": { "musicCarouselShelfBasicHeaderRenderer": { "title": { "runs": [ { "text": "Fans might also like" } ] } } },
                        "contents": [ two_row("UCjustice", "Justice") ]
                    } }
                ] }
            } } } ] } }
        });
        let (artist, more) = parse_artist(&page, "UCdaft").unwrap();
        assert_eq!(artist.name, "Daft Punk");
        assert_eq!(artist.description.as_deref(), Some("A French duo."));
        assert_eq!(more.as_deref(), Some("VLOLAK5uy_songs"));
        assert_eq!(artist.songs.len(), 1);
        let album = artist.songs[0].album.as_ref().unwrap();
        assert_eq!(album.id.as_deref(), Some("MPREb_ram"));
        // The playlist card is something there is no page for, so it is left out.
        assert_eq!(artist.shelves.len(), 2);
        assert_eq!(artist.shelves[0].cards.len(), 1);
        assert_eq!(artist.shelves[0].cards[0].kind, CardKind::Album);
        assert_eq!(artist.shelves[1].cards[0].kind, CardKind::Artist);
    }

    /// `cargo test -p ytm-core browse -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "network + a set-up session"]
    async fn live_an_album_and_its_artist_load() {
        let yt = crate::Session::new().unwrap().build_client().unwrap();
        let album = album(&yt, "MPREb_K8qWMWVqXGi").await.unwrap();
        eprintln!("{} · {} · {:?} · {} tracks", album.title, album.kind, album.year, album.tracks.len());
        assert_eq!(album.tracks.len(), 13);
        assert!(album.tracks.iter().all(|t| t.duration_seconds.is_some()));
        let id = album.artists[0].id.clone().unwrap();
        let artist = artist(&yt, &id).await.unwrap();
        eprintln!("{} · {} songs · {} shelves", artist.name, artist.songs.len(), artist.shelves.len());
        assert!(artist.songs.len() > 5, "the songs playlist should have been followed");
        assert!(artist.shelves.iter().any(|s| s.cards.iter().any(|c| c.kind == CardKind::Album)));
    }
}
