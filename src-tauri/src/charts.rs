//! "Billboard": live music charts from public, key-less endpoints.
//! Apple Music top charts per country (with 30s previews via the iTunes lookup API),
//! YouTube Music weekly top songs per country (unofficial charts.youtube.com API), and the Deezer global chart.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartEntry {
    pub rank: usize,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub artwork: Option<String>,
    pub preview: Option<String>,
    pub url: Option<String>,
    pub release_date: Option<String>,
    pub genre: Option<String>,
    pub explicit: bool,
    pub duration: Option<f64>,
    /// Last period's position (None = new entry or not reported by the source).
    pub previous_rank: Option<usize>,
    /// Weeks on the chart, when the source reports it.
    pub weeks: Option<u32>,
    /// YouTube video to download for this entry (YouTube Music charts only).
    pub video_url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Chart {
    pub title: String,
    pub source: String,
    pub updated: Option<String>,
    pub entries: Vec<ChartEntry>,
}

#[derive(Deserialize)]
struct AppleFeed {
    feed: AppleFeedInner,
}
#[derive(Deserialize)]
struct AppleFeedInner {
    title: String,
    updated: Option<String>,
    #[serde(default)]
    results: Vec<AppleItem>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppleItem {
    id: String,
    name: String,
    artist_name: String,
    artwork_url100: Option<String>,
    url: Option<String>,
    release_date: Option<String>,
    content_advisory_rating: Option<String>,
    #[serde(default)]
    genres: Vec<AppleGenre>,
}
#[derive(Deserialize)]
struct AppleGenre {
    name: String,
}
#[derive(Deserialize)]
struct ItunesLookup {
    results: Vec<ItunesTrack>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ItunesTrack {
    track_id: Option<u64>,
    collection_id: Option<u64>,
    preview_url: Option<String>,
    collection_name: Option<String>,
    track_time_millis: Option<u64>,
}

pub async fn apple(client: &reqwest::Client, country: &str, kind: &str, limit: usize) -> Result<Chart> {
    let country = country.to_ascii_lowercase();
    let kind = if kind == "albums" { "albums" } else { "songs" };
    let url = format!("https://rss.marketingtools.apple.com/api/v2/{country}/music/most-played/{limit}/{kind}.json");
    let feed: AppleFeed = client.get(url).send().await?.error_for_status()?.json().await?;

    // One batched lookup gives us playable previews + album names.
    let ids: Vec<&str> = feed.feed.results.iter().map(|r| r.id.as_str()).collect();
    let mut lookup: HashMap<u64, ItunesTrack> = HashMap::new();
    if kind == "songs" && !ids.is_empty() {
        let res = client
            .get("https://itunes.apple.com/lookup")
            .query(&[("id", ids.join(",")), ("country", country.clone())])
            .send()
            .await;
        if let Ok(res) = res {
            if let Ok(data) = res.json::<ItunesLookup>().await {
                for t in data.results {
                    if let Some(id) = t.track_id.or(t.collection_id) {
                        lookup.insert(id, t);
                    }
                }
            }
        }
    }

    let entries = feed
        .feed
        .results
        .into_iter()
        .enumerate()
        .map(|(i, r)| {
            let extra = r.id.parse::<u64>().ok().and_then(|id| lookup.get(&id));
            ChartEntry {
                rank: i + 1,
                album: extra.and_then(|e| e.collection_name.clone()),
                preview: extra.and_then(|e| e.preview_url.clone()),
                duration: extra.and_then(|e| e.track_time_millis).map(|ms| ms as f64 / 1000.0),
                previous_rank: None,
                weeks: None,
                video_url: None,
                title: r.name,
                artist: r.artist_name,
                artwork: r.artwork_url100.map(|u| u.replace("100x100bb", "600x600bb")),
                url: r.url,
                release_date: r.release_date,
                genre: r.genres.into_iter().map(|g| g.name).find(|g| g != "Music"),
                explicit: r.content_advisory_rating.is_some_and(|c| c.to_lowercase().starts_with("expl")),
            }
        })
        .collect();
    Ok(Chart {
        title: format!("{} · {}", feed.feed.title, country.to_uppercase()),
        source: "Apple Music".into(),
        updated: feed.feed.updated,
        entries,
    })
}

#[derive(Deserialize)]
struct DeezerChart {
    data: Vec<DeezerTrack>,
}
#[derive(Deserialize)]
struct DeezerTrack {
    title: String,
    link: Option<String>,
    duration: Option<f64>,
    preview: Option<String>,
    #[serde(default)]
    explicit_lyrics: bool,
    artist: DeezerName,
    album: DeezerAlbum,
}
#[derive(Deserialize)]
struct DeezerName {
    name: String,
}
#[derive(Deserialize)]
struct DeezerAlbum {
    title: String,
    cover_xl: Option<String>,
}

pub async fn deezer(client: &reqwest::Client, limit: usize) -> Result<Chart> {
    let chart: DeezerChart = client
        .get("https://api.deezer.com/chart/0/tracks")
        .query(&[("limit", limit)])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let entries = chart
        .data
        .into_iter()
        .enumerate()
        .map(|(i, t)| ChartEntry {
            rank: i + 1,
            title: t.title,
            artist: t.artist.name,
            album: Some(t.album.title),
            artwork: t.album.cover_xl,
            preview: t.preview.filter(|p| !p.is_empty()),
            url: t.link,
            release_date: None,
            genre: None,
            explicit: t.explicit_lyrics,
            duration: t.duration,
            previous_rank: None,
            weeks: None,
            video_url: None,
        })
        .collect();
    Ok(Chart { title: "Global Top Tracks".into(), source: "Deezer".into(), updated: None, entries })
}

/// YouTube Music "Weekly Top Songs" (charts.youtube.com). Unofficial: this is the JSON the charts site
/// itself loads, so it can change without notice. `country` is an ISO code or "global".
pub async fn youtube(client: &reqwest::Client, country: &str) -> Result<Chart> {
    let body = serde_json::json!({
        "context": { "client": { "clientName": "WEB_MUSIC_ANALYTICS", "clientVersion": "2.0", "hl": "en", "gl": "US" } },
        "browseId": "FEmusic_analytics_charts_home",
        "query": format!(
            "perspective=CHART_DETAILS&chart_params_country_code={}&chart_params_chart_type=TRACKS&chart_params_period_type=WEEKLY",
            country.to_lowercase()
        ),
    });
    let v: serde_json::Value = client
        .post("https://charts.youtube.com/youtubei/v1/browse?alt=json")
        .header("Origin", "https://charts.youtube.com")
        .header("Referer", "https://charts.youtube.com/")
        .json(&body)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let views = find_key(&v, "trackViews").and_then(|t| t.as_array()).ok_or_else(|| anyhow::anyhow!("YouTube chart format changed"))?;
    let entries = views
        .iter()
        .enumerate()
        .filter_map(|(i, t)| {
            let s = |k: &str| t.get(k).and_then(|x| x.as_str()).map(str::to_string);
            let meta = t.get("chartEntryMetadata");
            let num = |k: &str| meta.and_then(|m| m.get(k)).and_then(|x| x.as_u64());
            let artists: Vec<&str> = t
                .get("artists")
                .and_then(|a| a.as_array())
                .map(|a| a.iter().filter_map(|x| x.get("name").and_then(|n| n.as_str())).collect())
                .unwrap_or_default();
            // The "art track" is the audio-only upload: best for downloading the song itself.
            let video = s("atvExternalVideoId").or_else(|| s("encryptedVideoId"));
            let art = t
                .pointer("/thumbnail/thumbnails")
                .and_then(|a| a.as_array())
                .and_then(|a| a.last())
                .and_then(|x| x.get("url"))
                .and_then(|u| u.as_str())
                .map(|u| u.replace("=w180-h180", "=w544-h544"));
            Some(ChartEntry {
                rank: num("currentPosition").map(|n| n as usize).unwrap_or(i + 1),
                title: s("name")?,
                artist: artists.join(", "),
                album: None,
                artwork: art,
                preview: None,
                url: video.as_ref().map(|id| format!("https://music.youtube.com/watch?v={id}")),
                release_date: None,
                genre: None,
                explicit: false,
                duration: None,
                previous_rank: num("previousPosition").filter(|&n| n > 0).map(|n| n as usize),
                weeks: num("periodsOnChart").map(|n| n as u32),
                video_url: video.map(|id| format!("https://www.youtube.com/watch?v={id}")),
            })
        })
        .collect();
    let place = if country.eq_ignore_ascii_case("global") { "Global".to_string() } else { country.to_uppercase() };
    Ok(Chart {
        title: format!("Weekly Top Songs · {place}"),
        source: "YouTube Music".into(),
        updated: find_key(&v, "endDate").and_then(|d| d.as_str()).map(str::to_string),
        entries,
    })
}

/// Genre charts (Apple Music / iTunes Store top songs per genre and country), like the
/// "Browse" categories in Spotify and Apple Music. `(iTunes genre id, name)`.
pub const GENRES: &[(&str, &str)] = &[
    ("14", "Pop"),
    ("18", "Hip-Hop/Rap"),
    ("15", "R&B/Soul"),
    ("21", "Rock"),
    ("20", "Alternative"),
    ("17", "Dance"),
    ("7", "Electronic"),
    ("51", "K-Pop"),
    ("27", "J-Pop"),
    ("12", "Latin"),
    ("6", "Country"),
    ("10", "Singer/Songwriter"),
    ("11", "Jazz"),
    ("5", "Classical"),
    ("16", "Soundtrack"),
    ("24", "Reggae"),
    ("2", "Blues"),
    ("22", "Christian"),
];

/// Mood and activity mixes from Deezer's editors. `(key, name, search)`.
pub const MOODS: &[(&str, &str, &str)] = &[
    ("chill", "Chill", "chill vibes"),
    ("focus", "Focus", "focus"),
    ("workout", "Workout", "workout"),
    ("party", "Party", "party"),
    ("sleep", "Sleep", "sleep"),
    ("romance", "Romance", "love songs"),
    ("happy", "Feel Good", "feel good"),
    ("sad", "Sad", "sad songs"),
    ("roadtrip", "Road Trip", "road trip"),
    ("acoustic", "Acoustic", "acoustic"),
    ("throwback", "Throwback", "throwback"),
    ("indie", "Indie", "indie"),
];

pub fn genre_name(id: &str) -> Option<&'static str> {
    GENRES.iter().find(|(g, _)| *g == id).map(|(_, n)| *n)
}

/// Top songs of one genre in one country (the legacy iTunes RSS: the v2 feed has no genres).
pub async fn apple_genre(client: &reqwest::Client, country: &str, genre: &str, limit: usize) -> Result<Chart> {
    let country = country.to_ascii_lowercase();
    let name = genre_name(genre).ok_or_else(|| anyhow::anyhow!("unknown genre {genre}"))?;
    let url = format!("https://itunes.apple.com/{country}/rss/topsongs/limit={limit}/genre={genre}/json");
    let v: serde_json::Value = client.get(url).send().await?.error_for_status()?.json().await?;
    let label = |e: &serde_json::Value, k: &str| e.get(k).and_then(|x| x.get("label")).and_then(|x| x.as_str()).map(str::to_string);
    // One entry comes back as an object instead of a list.
    let list = match v.pointer("/feed/entry") {
        Some(serde_json::Value::Array(a)) => a.clone(),
        Some(e @ serde_json::Value::Object(_)) => vec![e.clone()],
        _ => vec![],
    };
    let entries = list
        .iter()
        .enumerate()
        .filter_map(|(i, e)| {
            let links: Vec<&serde_json::Value> = match e.get("link") {
                Some(serde_json::Value::Array(a)) => a.iter().collect(),
                Some(l) => vec![l],
                None => vec![],
            };
            let href = |pred: &dyn Fn(&serde_json::Value) -> bool| {
                links.iter().find(|l| pred(l)).and_then(|l| l.pointer("/attributes/href")).and_then(|h| h.as_str()).map(str::to_string)
            };
            Some(ChartEntry {
                rank: i + 1,
                title: label(e, "im:name")?,
                artist: label(e, "im:artist").unwrap_or_default(),
                album: e.get("im:collection").and_then(|c| label(c, "im:name")),
                artwork: e
                    .get("im:image")
                    .and_then(|a| a.as_array())
                    .and_then(|a| a.last())
                    .and_then(|x| x.get("label"))
                    .and_then(|x| x.as_str())
                    .map(|u| u.replace("170x170bb", "600x600bb")),
                preview: href(&|l| l.pointer("/attributes/im:assetType").and_then(|x| x.as_str()) == Some("preview")),
                url: href(&|l| l.pointer("/attributes/rel").and_then(|x| x.as_str()) == Some("alternate")),
                release_date: e.pointer("/im:releaseDate/label").and_then(|x| x.as_str()).map(str::to_string),
                genre: Some(name.to_string()),
                explicit: false,
                // The preview link carries the preview's length, not the song's.
                duration: None,
                previous_rank: None,
                weeks: None,
                video_url: None,
            })
        })
        .collect();
    Ok(Chart { title: format!("{name} · {}", country.to_uppercase()), source: "Apple Music".into(), updated: None, entries })
}

#[derive(Deserialize)]
struct DeezerPlaylists {
    data: Vec<DeezerPlaylist>,
}
#[derive(Deserialize)]
struct DeezerPlaylist {
    id: u64,
    title: String,
    user: DeezerName,
}

/// A mood mix: the top Deezer editors' playlist for the mood's search.
pub async fn deezer_mood(client: &reqwest::Client, key: &str, limit: usize) -> Result<Chart> {
    let (_, name, query) = MOODS.iter().find(|(k, ..)| *k == key).ok_or_else(|| anyhow::anyhow!("unknown mood {key}"))?;
    let found: DeezerPlaylists = client
        .get("https://api.deezer.com/search/playlist")
        .query(&[("q", *query), ("limit", "25")])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    // Editors' playlists are curated and kept fresh; anyone else's is a lottery.
    let pick = found
        .data
        .iter()
        .find(|p| p.user.name.contains("Deezer"))
        .or(found.data.first())
        .ok_or_else(|| anyhow::anyhow!("no {name} mix found"))?;
    let tracks: DeezerChart = client
        .get(format!("https://api.deezer.com/playlist/{}/tracks", pick.id))
        .query(&[("limit", limit)])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let entries = tracks
        .data
        .into_iter()
        .enumerate()
        .map(|(i, t)| ChartEntry {
            rank: i + 1,
            title: t.title,
            artist: t.artist.name,
            album: Some(t.album.title),
            artwork: t.album.cover_xl,
            preview: t.preview.filter(|p| !p.is_empty()),
            url: t.link,
            release_date: None,
            genre: Some(name.to_string()),
            explicit: t.explicit_lyrics,
            duration: t.duration,
            previous_rank: None,
            weeks: None,
            video_url: None,
        })
        .collect();
    Ok(Chart { title: format!("{name} · {}", pick.title), source: "Deezer".into(), updated: None, entries })
}

/// Depth-first search for the first value under `key`.
fn find_key<'a>(v: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
    match v {
        serde_json::Value::Object(m) => m.get(key).or_else(|| m.values().find_map(|x| find_key(x, key))),
        serde_json::Value::Array(a) => a.iter().find_map(|x| find_key(x, key)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    /// Network: the unofficial YouTube Music chart endpoint still answers in the expected shape.
    #[tokio::test]
    #[ignore]
    async fn youtube_chart_loads() {
        let c = reqwest::Client::new();
        for country in ["id", "global"] {
            let chart = super::youtube(&c, country).await.unwrap();
            println!("{} ({:?}): #1 {} - {}", chart.title, chart.updated, chart.entries[0].artist, chart.entries[0].title);
            assert!(chart.entries.len() >= 50);
            assert!(chart.entries.iter().all(|e| e.video_url.is_some() && !e.artist.is_empty()));
        }
    }

    /// Network: genre charts and mood mixes load (the Browse categories).
    #[tokio::test]
    #[ignore]
    async fn categories_load() {
        let c = reqwest::Client::new();
        let pop = super::apple_genre(&c, "id", "14", 25).await.unwrap();
        println!("{}: #1 {} - {}", pop.title, pop.entries[0].artist, pop.entries[0].title);
        assert!(pop.entries.len() >= 10 && pop.entries.iter().all(|e| e.preview.is_some()));
        let chill = super::deezer_mood(&c, "chill", 50).await.unwrap();
        println!("{}: #1 {} - {}", chill.title, chill.entries[0].artist, chill.entries[0].title);
        assert!(chill.entries.len() >= 10);
    }
}
