//! A small JSON RPC over the core engine, used by the native media services
//! (Android Auto / Bluetooth via Media3, CarPlay / Now Playing on iOS).

use crate::audio::{QueueItem, Repeat};
use crate::charts::ChartEntry;
use crate::core::Core;
use crate::library::Track;
use anyhow::{anyhow, Result};
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

pub const SETTING_CAR_AUTOPLAY: &str = "car_autoplay";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    /// Local file path (cover cache) or https URL.
    pub art: Option<String>,
    pub playable: bool,
    pub browsable: bool,
    pub duration: f64,
    /// "category" | "album" | "artist" | "playlist" | "track" | "lyric"
    pub kind: &'static str,
    /// Section heading in long lists (Browse: "Charts", "Genres", "Moods").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<&'static str>,
}

fn category(id: &str, title: &str) -> Node {
    Node { id: id.into(), title: title.into(), subtitle: None, art: None, playable: false, browsable: true, duration: 0.0, kind: "category", group: None }
}

fn to_item(t: &Track) -> QueueItem {
    QueueItem {
        id: Some(t.id),
        path: t.path.clone(),
        title: t.title.clone(),
        artist: t.artist.clone(),
        album: t.album.clone(),
        duration: t.duration,
        cover: t.cover.clone(),
    }
}

fn album_key(t: &Track) -> String {
    format!("{}\u{1}{}", t.album_artist, t.album)
}

/// Same normalisation as the UI, for matching chart entries to local files.
pub fn norm(s: &str) -> String {
    let lower = s.to_lowercase();
    let mut cut = lower.as_str();
    for sep in [" (feat", " (ft", " [feat", " (with", " - remaster", " (remaster"] {
        if let Some(i) = cut.find(sep) {
            cut = &cut[..i];
        }
    }
    cut.replace('&', "and").chars().filter(|c| c.is_alphanumeric()).collect()
}

/// Charts by browse id ("billboard", "genre:14", "mood:chill"), with the country they're for.
static CHARTS: Mutex<Vec<(String, Instant, String, Vec<ChartEntry>)>> = Mutex::new(Vec::new());

fn chart(core: &Core, parent: &str) -> Result<Vec<ChartEntry>> {
    let country = core.library.setting("chart_country").unwrap_or_else(|| "us".into());
    if let Some((.., entries)) = CHARTS
        .lock()
        .iter()
        .find(|(p, at, c, _)| p == parent && *c == country && at.elapsed() < Duration::from_secs(1800))
    {
        return Ok(entries.clone());
    }
    let fetch = async {
        match parent.split_once(':') {
            Some(("genre", id)) => crate::charts::apple_genre(&core.http, &country, id, 50).await,
            Some(("mood", key)) => crate::charts::deezer_mood(&core.http, key, 50).await,
            _ => crate::charts::apple(&core.http, &country, "songs", 50).await,
        }
    };
    let chart = core.runtime.block_on(fetch)?;
    let mut cache = CHARTS.lock();
    cache.retain(|(p, ..)| p != parent);
    cache.push((parent.to_string(), Instant::now(), country, chart.entries.clone()));
    Ok(chart.entries)
}

fn is_chart(parent: &str) -> bool {
    parent == "billboard" || parent.starts_with("genre:") || parent.starts_with("mood:")
}

/// Node id of a chart entry: "chart:3" in Billboard (kept for older ids), "genre:14#3" elsewhere.
fn chart_entry_id(parent: &str, rank: usize) -> String {
    if parent == "billboard" { format!("chart:{rank}") } else { format!("{parent}#{rank}") }
}

/// The browse id an entry id belongs to.
fn chart_parent(id: &str) -> Option<&str> {
    if id.starts_with("chart:") {
        Some("billboard")
    } else {
        id.split_once('#').map(|(p, _)| p).filter(|p| is_chart(p))
    }
}

/// Playable items under `parent`, in order, with their node ids.
fn playables(core: &Core, parent: &str) -> Result<Vec<(String, QueueItem)>> {
    let tracks = core.library.tracks()?;
    let pick = |f: &dyn Fn(&Track) -> bool| -> Vec<(String, QueueItem)> {
        tracks.iter().filter(|t| f(t)).map(|t| (format!("track:{}", t.id), to_item(t))).collect()
    };
    Ok(match parent.split_once(':').unwrap_or((parent, "")) {
        ("recent", _) => {
            let mut t: Vec<&Track> = tracks.iter().filter(|t| t.last_played.is_some()).collect();
            t.sort_by_key(|t| std::cmp::Reverse(t.last_played));
            t.into_iter().take(50).map(|t| (format!("track:{}", t.id), to_item(t))).collect()
        }
        ("liked", _) => pick(&|t| t.liked),
        ("songs", _) => pick(&|_| true),
        ("album", key) => pick(&|t| album_key(t) == key),
        ("artist", name) => pick(&|t| t.album_artist == name),
        ("playlist", id) => core
            .library
            .playlist_tracks(id.parse()?)?
            .iter()
            .map(|t| (format!("track:{}", t.id), to_item(t)))
            .collect(),
        _ if is_chart(parent) => {
            let index: std::collections::HashMap<String, &Track> =
                tracks.iter().map(|t| (format!("{}|{}", norm(&t.artist), norm(&t.title)), t)).collect();
            chart(core, parent)?
                .iter()
                .filter_map(|e| {
                    let id = chart_entry_id(parent, e.rank);
                    if let Some(t) = index.get(&format!("{}|{}", norm(&e.artist), norm(&e.title))) {
                        return Some((id, to_item(t)));
                    }
                    // Full song from YouTube when this build can stream it, else the 30 s preview.
                    let full = crate::audio::engine::has_stream_resolver();
                    let path = match (full, &e.preview) {
                        (true, preview) => Some(format!(
                            "ytq:{} {}{}",
                            e.artist,
                            e.title,
                            preview.as_ref().map(|p| format!("#{p}")).unwrap_or_default()
                        )),
                        (false, Some(p)) => Some(p.clone()),
                        (false, None) => None,
                    }?;
                    Some((id, QueueItem {
                        id: None,
                        path,
                        title: e.title.clone(),
                        artist: e.artist.clone(),
                        album: e.album.clone().unwrap_or_default(),
                        duration: if full { e.duration.unwrap_or(0.0) } else { 30.0 },
                        cover: e.artwork.clone(),
                    }))
                })
                .collect()
        }
        ("search", q) => {
            let terms: Vec<String> = q.to_lowercase().split_whitespace().map(String::from).collect();
            let hits: Vec<(String, QueueItem)> = tracks
                .iter()
                .filter(|t| {
                    let hay = format!("{} {} {}", t.title, t.artist, t.album).to_lowercase();
                    terms.iter().all(|w| hay.contains(w))
                })
                .take(100)
                .map(|t| (format!("track:{}", t.id), to_item(t)))
                .collect();
            hits
        }
        _ => vec![],
    })
}

pub fn children(core: &Core, parent: &str) -> Result<Vec<Node>> {
    // Android Auto shows at most four root tabs.
    if parent == "root" {
        return Ok(vec![
            category("recent", "Recent"),
            category("library", "Library"),
            category("browse", "Browse"),
            category("playlists", "Playlists"),
        ]);
    }
    if parent == "browse" {
        let mut nodes = vec![Node { group: Some("Charts"), ..category("billboard", "Top Songs") }];
        for (id, name) in crate::charts::GENRES {
            nodes.push(Node { group: Some("Genres"), ..category(&format!("genre:{id}"), name) });
        }
        for (key, name, _) in crate::charts::MOODS {
            nodes.push(Node { group: Some("Moods"), ..category(&format!("mood:{key}"), name) });
        }
        return Ok(nodes);
    }
    if parent == "lyrics" {
        let lines = current_lyrics(core)?;
        if lines.is_empty() {
            return Ok(vec![lyric_node(None, "No lyrics for this song")]);
        }
        return Ok(lines.into_iter().map(|(t, text)| lyric_node(t, &text)).collect());
    }
    if parent == "recent" {
        // Lyrics for the song that's playing, at the top of the first tab: Android Auto has no
        // other place for them (its Now Playing screen can't show custom text).
        let mut nodes = Vec::new();
        if let Some(t) = core.player.state().track {
            nodes.push(Node {
                subtitle: Some(format!("{} · {}", t.title, t.artist)),
                group: Some("Now playing"),
                ..category("lyrics", "Lyrics")
            });
        }
        nodes.extend(playables(core, "recent")?.into_iter().map(|(id, it)| track_node(id, it, Some("Recently played"))));
        return Ok(nodes);
    }
    if parent == "library" {
        return Ok(vec![
            category("albums", "Albums"),
            category("artists", "Artists"),
            category("liked", "Liked Songs"),
            category("songs", "All Songs"),
        ]);
    }
    let tracks = core.library.tracks()?;
    match parent {
        "albums" => {
            let mut seen = std::collections::HashSet::new();
            Ok(tracks
                .iter()
                .filter(|t| seen.insert(album_key(t)))
                .map(|t| Node {
                    id: format!("album:{}", album_key(t)),
                    title: t.album.clone(),
                    subtitle: Some(t.album_artist.clone()),
                    art: tracks.iter().find(|x| album_key(x) == album_key(t) && x.cover.is_some()).and_then(|x| x.cover.clone()),
                    playable: false,
                    browsable: true,
                    duration: 0.0,
                    kind: "album",
                    group: None,
                })
                .collect())
        }
        "artists" => {
            let mut names: Vec<&str> = tracks.iter().map(|t| t.album_artist.as_str()).collect();
            names.sort_unstable_by_key(|n| n.to_lowercase());
            names.dedup();
            Ok(names
                .into_iter()
                .map(|n| Node {
                    id: format!("artist:{n}"),
                    title: n.into(),
                    subtitle: None,
                    art: tracks.iter().find(|t| t.album_artist == n && t.cover.is_some()).and_then(|t| t.cover.clone()),
                    playable: false,
                    browsable: true,
                    duration: 0.0,
                    kind: "artist",
                    group: None,
                })
                .collect())
        }
        "playlists" => Ok(core
            .library
            .playlists()?
            .into_iter()
            .map(|p| Node {
                id: format!("playlist:{}", p.id),
                title: p.name,
                subtitle: Some(format!("{} songs", p.track_count)),
                art: None,
                playable: false,
                browsable: true,
                duration: 0.0,
                kind: "playlist",
                group: None,
            })
            .collect()),
        _ if is_chart(parent) => {
            let entries = chart(core, parent)?;
            let items = playables(core, parent)?;
            Ok(items
                .into_iter()
                .map(|(id, it)| {
                    let rank = id.rsplit([':', '#']).next().unwrap_or("").to_string();
                    let local = it.id.is_some();
                    let e = entries.iter().find(|e| e.rank.to_string() == rank);
                    Node {
                        title: format!("{rank}. {}", it.title),
                        subtitle: Some(if local {
                            format!("{} · In your library", it.artist)
                        } else if it.path.starts_with("ytq:") {
                            format!("{} · YouTube", it.artist)
                        } else {
                            format!("{} · Preview", it.artist)
                        }),
                        art: e.and_then(|e| e.artwork.clone()).or(it.cover.clone()),
                        playable: true,
                        browsable: false,
                        duration: it.duration,
                        kind: "track",
                        group: None,
                        id,
                    }
                })
                .collect())
        }
        _ => Ok(playables(core, parent)?.into_iter().map(|(id, it)| track_node(id, it, None)).collect()),
    }
}

fn track_node(id: String, it: QueueItem, group: Option<&'static str>) -> Node {
    Node {
        id,
        title: it.title,
        subtitle: Some(it.artist),
        art: it.cover,
        playable: true,
        browsable: false,
        duration: it.duration,
        kind: "track",
        group,
    }
}

/// A lyric line. Tapping a synced line seeks there ("lyric:12.5"); plain lines are "lyric:".
fn lyric_node(time: Option<f64>, text: &str) -> Node {
    Node {
        id: format!("lyric:{}", time.map(|t| format!("{t:.2}")).unwrap_or_default()),
        title: text.to_string(),
        subtitle: time.map(|t| format!("{}:{:02}", (t / 60.0) as u64, (t % 60.0) as u64)),
        art: None,
        playable: true,
        browsable: false,
        duration: 0.0,
        kind: "lyric",
        group: None,
    }
}

/// Lyrics of the song that's playing, line by line.
fn current_lyrics(core: &Core) -> Result<Vec<(Option<f64>, String)>> {
    let Some(item) = core.player.state().track else { return Ok(vec![]) };
    let l = core.runtime.block_on(crate::lyrics::get(&core.library, &core.http, &item, false))?;
    Ok(crate::lyrics::lines(&l))
}

/// Resolves bare node ids ("track:12", "chart:3") into queue items.
fn resolve_ids(core: &Core, ids: &[String]) -> Result<Vec<QueueItem>> {
    let tracks = core.library.tracks()?;
    let mut charts: Vec<(String, Vec<(String, QueueItem)>)> = Vec::new();
    for parent in ids.iter().filter_map(|i| chart_parent(i)) {
        if !charts.iter().any(|(p, _)| p == parent) {
            charts.push((parent.to_string(), playables(core, parent)?));
        }
    }
    Ok(ids
        .iter()
        .filter_map(|id| match (id.split_once(':'), chart_parent(id)) {
            (Some(("track", n)), _) => n.parse::<i64>().ok().and_then(|n| tracks.iter().find(|t| t.id == n)).map(to_item),
            (_, Some(parent)) => charts
                .iter()
                .find(|(p, _)| p == parent)
                .and_then(|(_, items)| items.iter().find(|(cid, _)| cid == id))
                .map(|(_, it)| it.clone()),
            _ => None,
        })
        .collect())
}

/// Plays the playable children of `parent`, starting at `id` (or the first one).
pub fn play(core: &Core, parent: &str, id: Option<&str>) -> Result<()> {
    // A lyric line on a car screen: jump to it rather than replacing the queue.
    if parent == "lyrics" {
        if let Some(t) = id.and_then(|i| i.strip_prefix("lyric:")).and_then(|t| t.parse::<f64>().ok()) {
            core.player.seek(t);
        }
        core.player.play();
        return Ok(());
    }
    let items = playables(core, parent)?;
    if items.is_empty() {
        return Err(anyhow!("nothing to play in {parent}"));
    }
    let start = id.and_then(|id| items.iter().position(|(i, _)| i == id)).unwrap_or(0);
    core.player.play_queue(items.into_iter().map(|(_, it)| it).collect(), start);
    Ok(())
}

pub fn session(core: &Core) -> Value {
    let s = core.player.state();
    let queue = core.player.queue();
    let current = s.index.and_then(|i| queue.iter().position(|(qi, _)| *qi == i));
    json!({
        "playing": s.playing,
        "ended": s.ended,
        "position": s.position,
        "duration": s.duration,
        "shuffle": s.shuffle,
        "repeat": s.repeat,
        "current": current,
        "queueVersion": s.queue_version,
        "error": s.error,
        "items": queue.iter().map(|(i, it)| json!({
            "index": i,
            "mediaId": it.id.map(|id| format!("track:{id}")).unwrap_or_else(|| it.path.clone()),
            "title": it.title,
            "artist": it.artist,
            "album": it.album,
            "duration": it.duration,
            "art": it.cover,
        })).collect::<Vec<_>>(),
    })
}

pub fn call(core: &Core, method: &str, args: &Value) -> Result<Value> {
    let s = |k: &str| args.get(k).and_then(Value::as_str);
    let f = |k: &str| args.get(k).and_then(Value::as_f64);
    let p = &core.player;
    match method {
        "session" => return Ok(session(core)),
        "control" => match s("action").unwrap_or("") {
            "play" => p.play(),
            "pause" => p.pause(),
            "toggle" => p.toggle(),
            "next" => p.next(),
            "previous" => p.previous(),
            a => return Err(anyhow!("unknown action {a}")),
        },
        "seek" => p.seek(f("position").unwrap_or(0.0)),
        "seekBy" => p.seek((p.state().position + f("delta").unwrap_or(0.0)).max(0.0)),
        "jump" => p.jump(f("index").unwrap_or(0.0) as usize),
        "shuffle" => p.set_shuffle(args.get("on").and_then(Value::as_bool).unwrap_or(false)),
        "repeat" => p.set_repeat(serde_json::from_value::<Repeat>(args.get("mode").cloned().unwrap_or(json!("off")))?),
        "children" => return Ok(serde_json::to_value(children(core, s("parent").unwrap_or("root"))?)?),
        "search" => {
            return Ok(serde_json::to_value(children(core, &format!("search:{}", s("query").unwrap_or("")))?)?);
        }
        "play" => play(core, s("parent").unwrap_or("songs"), s("id"))?,
        "playId" => {
            // A bare media id (e.g. from Android Auto's "recent" resumption or voice search results).
            let id = s("id").unwrap_or("");
            let parent = chart_parent(id).unwrap_or("songs");
            play(core, parent, Some(id))?;
        }
        "playIds" => {
            let ids: Vec<String> = serde_json::from_value(args.get("ids").cloned().unwrap_or(json!([])))?;
            let items = resolve_ids(core, &ids)?;
            if items.is_empty() {
                return Err(anyhow!("nothing playable"));
            }
            let start = (f("start").unwrap_or(0.0) as usize).min(items.len() - 1);
            p.play_queue(items, start);
        }
        "enqueueIds" => {
            let ids: Vec<String> = serde_json::from_value(args.get("ids").cloned().unwrap_or(json!([])))?;
            p.enqueue(resolve_ids(core, &ids)?, args.get("next").and_then(Value::as_bool).unwrap_or(false));
        }
        "remove" => p.remove_from_queue(f("index").unwrap_or(0.0) as usize),
        // Car lyrics screens: `{ title, artist, synced, lines: [{ time, text }] }`.
        "lyrics" => {
            let track = p.state().track;
            let lines = current_lyrics(core)?;
            return Ok(json!({
                "title": track.as_ref().map(|t| t.title.clone()),
                "artist": track.as_ref().map(|t| t.artist.clone()),
                "synced": lines.iter().any(|(t, _)| t.is_some()),
                "lines": lines.into_iter().map(|(t, text)| json!({ "time": t, "text": text })).collect::<Vec<_>>(),
            }));
        }
        // Sleep timer (sleep.rs): { minutes } or { endOfTrack: true }.
        "sleepSet" => {
            if args.get("endOfTrack").and_then(Value::as_bool).unwrap_or(false) {
                crate::sleep::set_end_of_track(p);
            } else {
                crate::sleep::set_minutes(p, f("minutes").ok_or_else(|| anyhow!("minutes missing"))?);
            }
            return Ok(crate::sleep::status());
        }
        "sleepCancel" => {
            crate::sleep::cancel();
            return Ok(crate::sleep::status());
        }
        "sleepStatus" => return Ok(crate::sleep::status()),
        // Browse categories for the UI (genres → Apple Music charts, moods → Deezer mixes).
        "categories" => {
            return Ok(json!({
                "genres": crate::charts::GENRES.iter().map(|(id, name)| json!({ "id": id, "name": name })).collect::<Vec<_>>(),
                "moods": crate::charts::MOODS.iter().map(|(id, name, _)| json!({ "id": id, "name": name })).collect::<Vec<_>>(),
            }))
        }
        "settings" => {
            return Ok(json!({
                SETTING_CAR_AUTOPLAY: core.library.setting(SETTING_CAR_AUTOPLAY).as_deref() != Some("false"),
                "chart_country": core.library.setting("chart_country").unwrap_or_else(|| "us".into()),
            }))
        }
        // Android YouTube downloads (StafyYouTube.kt): reuse the desktop title parsing and tagging.
        "ytVideos" => {
            let raw: Value = serde_json::from_str(s("json").unwrap_or("{}"))?;
            return Ok(serde_json::to_value(crate::ytmeta::videos_from_json(&raw))?);
        }
        // "Download" on a song streaming from YouTube: which video to save (no search page).
        "ytVideoId" => return Ok(json!(crate::audio::engine::video_id_for(s("path").unwrap_or(""))?)),
        // Android "See video" (StafyYouTube.kt): the playable link from yt-dlp's JSON.
        "ytVideoStream" => {
            let raw: Value = serde_json::from_str(s("json").unwrap_or("{}"))?;
            return crate::ytmeta::video_stream_from_json(&raw).ok_or_else(|| anyhow!("no video found"));
        }
        "retag" => crate::ytmeta::retag(
            std::path::Path::new(s("path").unwrap_or("")),
            s("title").unwrap_or(""),
            s("artist").unwrap_or(""),
        )?,
        "rescan" => {
            let lib = core.library.clone();
            std::thread::spawn(move || {
                let _ = lib.scan(|_| {});
            });
        }
        // Wi-Fi sync between devices (sync.rs). The Sync page polls syncStatus while open.
        "syncStatus" => {
            return match crate::sync::get() {
                Some(sy) => sy.status(),
                None => Ok(json!({
                    "enabled": false,
                    "me": { "name": crate::sync::device_name(&core.library) },
                    "nearby": [], "paired": [], "progress": null,
                    "syncDir": crate::sync::sync_dir(&core.library).to_string_lossy(),
                })),
            }
        }
        "syncEnable" => crate::sync::enable(core.library.clone(), args.get("on").and_then(Value::as_bool).unwrap_or(true))?,
        "syncSetName" => {
            let name: String = s("name").unwrap_or("").trim().chars().take(40).collect();
            core.library.set_setting("device_name", &name)?;
            // Re-announce under the new name.
            if crate::sync::get().is_some() && core.library.setting("sync_enabled").as_deref() == Some("true") {
                crate::sync::enable(core.library.clone(), false)?;
                crate::sync::enable(core.library.clone(), true)?;
            }
        }
        "syncPairBegin" => return sync_or_err()?.pair_begin(s("addr").unwrap_or("")),
        "syncPairComplete" => return sync_or_err()?.pair_complete(s("code").unwrap_or("")),
        "syncPairCancel" => sync_or_err()?.pair_cancel(),
        "syncNow" => sync_or_err()?.sync_now(s("id").unwrap_or(""))?,
        "syncForget" => sync_or_err()?.forget(s("id").unwrap_or(""))?,
        // Look up YouTube links early for likely taps, so they start in about a second.
        "ytPrepare" => {
            let specs: Vec<String> = serde_json::from_value(args.get("specs").cloned().unwrap_or(json!([])))?;
            crate::audio::engine::prepare(&specs);
        }
        "deleteTracks" => {
            let ids: Vec<i64> = serde_json::from_value(args.get("ids").cloned().unwrap_or(json!([])))?;
            return Ok(serde_json::to_value(core.library.delete_tracks(&ids)?)?);
        }
        "setSetting" => core.library.set_setting(s("key").unwrap_or(""), s("value").unwrap_or(""))?,
        m => return Err(anyhow!("unknown method {m}")),
    }
    Ok(Value::Null)
}

/// String-in/string-out entry used by the FFI layers. Always returns JSON: `{"ok":..}` or `{"error":".."}`.
pub fn call_json(method: &str, args: &str) -> String {
    let Some(core) = crate::core::get() else {
        return json!({"error": "engine not initialised"}).to_string();
    };
    let args: Value = serde_json::from_str(args).unwrap_or(Value::Null);
    match call(core, method, &args) {
        Ok(v) => json!({ "ok": v }).to_string(),
        Err(e) => json!({ "error": format!("{e:#}") }).to_string(),
    }
}

fn sync_or_err() -> Result<&'static std::sync::Arc<crate::sync::Sync>> {
    crate::sync::get().ok_or_else(|| anyhow!("turn on Wi-Fi sync first"))
}
