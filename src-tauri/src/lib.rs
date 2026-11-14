mod audio;
mod charts;
pub mod core;
pub mod ffi;
#[cfg(target_os = "android")]
mod jni_bridge;
mod library;
mod lyrics;
mod media_controls;
#[cfg(target_os = "macos")]
mod notch;
mod importer;
mod rpc;
mod sleep;
pub mod sync;
// Desktop and iOS run yt-dlp from Rust (ytrun.rs); Android drives its own from Kotlin.
#[cfg(not(target_os = "android"))]
mod youtube;
#[cfg(not(target_os = "android"))]
pub mod ytrun;
mod ytmeta;

use audio::{Player, PlayerState, QueueItem, Repeat};
use library::{Library, Playlist, ScanProgress, Track};
use parking_lot::Mutex;
use std::path::Path;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

struct AppState {
    player: Arc<Player>,
    library: Arc<Library>,
    http: reqwest::Client,
}

type Res<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

// ---- player ---------------------------------------------------------------------------

#[tauri::command(async)]
fn player_play_queue(s: State<AppState>, items: Vec<QueueItem>, start_index: usize) {
    s.player.play_queue(items, start_index);
}

#[tauri::command(async)]
fn player_enqueue(s: State<AppState>, items: Vec<QueueItem>, play_next: bool) {
    s.player.enqueue(items, play_next);
}

#[tauri::command(async)]
fn player_remove(s: State<AppState>, index: usize) {
    s.player.remove_from_queue(index);
}

#[tauri::command(async)]
fn player_jump(s: State<AppState>, index: usize) {
    s.player.jump(index);
}

#[derive(serde::Serialize)]
struct QueueEntry {
    index: usize,
    item: QueueItem,
}

#[tauri::command(async)]
fn player_queue(s: State<AppState>) -> Vec<QueueEntry> {
    s.player.queue().into_iter().map(|(index, item)| QueueEntry { index, item }).collect()
}

#[tauri::command(async)]
fn player_state(s: State<AppState>) -> PlayerState {
    s.player.state()
}

#[tauri::command(async)]
fn player_control(s: State<AppState>, action: String) -> Res<()> {
    match action.as_str() {
        "play" => s.player.play(),
        "pause" => s.player.pause(),
        "toggle" => s.player.toggle(),
        "next" => s.player.next(),
        "previous" => s.player.previous(),
        other => return Err(format!("unknown action {other}")),
    }
    Ok(())
}

#[tauri::command(async)]
fn player_seek(s: State<AppState>, position: f64) {
    s.player.seek(position);
}

#[tauri::command(async)]
fn player_set_volume(s: State<AppState>, volume: f32) {
    s.player.set_volume(volume);
}

#[tauri::command(async)]
fn player_set_shuffle(s: State<AppState>, shuffle: bool) {
    s.player.set_shuffle(shuffle);
}

#[tauri::command(async)]
fn player_set_repeat(s: State<AppState>, repeat: Repeat) {
    s.player.set_repeat(repeat);
}

// ---- library --------------------------------------------------------------------------

#[tauri::command(async)]
fn library_tracks(s: State<AppState>) -> Res<Vec<Track>> {
    s.library.tracks().map_err(err)
}

#[tauri::command(async)]
fn library_folders(s: State<AppState>) -> Res<Vec<String>> {
    s.library.folders().map_err(err)
}

fn spawn_scan(app: AppHandle, library: Arc<Library>) {
    std::thread::spawn(move || {
        let progress_app = app.clone();
        let result = library.scan(move |p: ScanProgress| {
            let _ = progress_app.emit("library://scan", p);
        });
        if let Err(e) = result {
            log::error!("scan failed: {e:#}");
        }
        let _ = app.emit("library://changed", ());
    });
}

#[tauri::command(async)]
fn library_add_folder(app: AppHandle, s: State<AppState>, path: String) -> Res<()> {
    s.library.add_folder(&path).map_err(err)?;
    spawn_scan(app, s.library.clone());
    Ok(())
}

#[tauri::command(async)]
fn library_remove_folder(app: AppHandle, s: State<AppState>, path: String) -> Res<()> {
    s.library.remove_folder(&path).map_err(err)?;
    let _ = app.emit("library://changed", ());
    Ok(())
}

#[tauri::command(async)]
fn library_rescan(app: AppHandle, s: State<AppState>) {
    spawn_scan(app, s.library.clone());
}

#[tauri::command(async)]
fn library_is_scanning(s: State<AppState>) -> bool {
    s.library.is_scanning()
}

#[tauri::command(async)]
fn library_toggle_like(s: State<AppState>, id: i64) -> Res<bool> {
    s.library.toggle_like(id).map_err(err)
}

/// Reads tags for files opened outside the library (Open File / drag & drop).
#[tauri::command]
async fn probe_files(s: State<'_, AppState>, paths: Vec<String>) -> Res<Vec<Track>> {
    let library = s.library.clone();
    tauri::async_runtime::spawn_blocking(move || {
        paths.iter().filter_map(|p| library.read_track(Path::new(p))).collect::<Vec<_>>()
    })
    .await
    .map_err(err)
}

#[tauri::command(async)]
fn audio_extensions() -> Vec<&'static str> {
    library::AUDIO_EXTENSIONS.to_vec()
}

// ---- playlists ------------------------------------------------------------------------

#[tauri::command(async)]
fn playlists(s: State<AppState>) -> Res<Vec<Playlist>> {
    s.library.playlists().map_err(err)
}

#[tauri::command(async)]
fn playlist_create(s: State<AppState>, name: String) -> Res<i64> {
    s.library.create_playlist(&name).map_err(err)
}

#[tauri::command(async)]
fn playlist_rename(s: State<AppState>, id: i64, name: String) -> Res<()> {
    s.library.rename_playlist(id, &name).map_err(err)
}

#[tauri::command(async)]
fn playlist_delete(s: State<AppState>, id: i64) -> Res<()> {
    s.library.delete_playlist(id).map_err(err)
}

#[tauri::command(async)]
fn playlist_tracks(s: State<AppState>, id: i64) -> Res<Vec<Track>> {
    s.library.playlist_tracks(id).map_err(err)
}

#[tauri::command(async)]
fn playlist_add(s: State<AppState>, id: i64, track_ids: Vec<i64>) -> Res<()> {
    s.library.add_to_playlist(id, &track_ids).map_err(err)
}

#[tauri::command(async)]
fn playlist_remove(s: State<AppState>, id: i64, position: usize) -> Res<()> {
    s.library.remove_from_playlist(id, position).map_err(err)
}

// ---- lyrics & charts ------------------------------------------------------------------

#[tauri::command]
async fn lyrics_get(s: State<'_, AppState>, item: QueueItem, force: bool) -> Res<lyrics::Lyrics> {
    lyrics::get(&s.library, &s.http, &item, force).await.map_err(err)
}

/// Same RPC surface the car integrations use (settings, browse tree).
#[tauri::command(async)]
fn rpc(method: String, args: serde_json::Value) -> Res<serde_json::Value> {
    let core = core::get().ok_or("engine not initialised")?;
    let started = std::time::Instant::now();
    let r = rpc::call(core, &method, &args).map_err(|e| format!("{e:#}"));
    if started.elapsed() > std::time::Duration::from_millis(500) {
        eprintln!("catlify: slow rpc {method}: {:?}", started.elapsed());
    }
    r
}

/// Import: a public Spotify / Apple Music playlist or album link.
#[tauri::command]
async fn import_link(s: State<'_, AppState>, url: String) -> Res<importer::Collection> {
    importer::from_link(&s.http, &url).await.map_err(|e| format!("{e:#}"))
}

/// Import: an export file (Spotify, Exportify, Apple Music, Google Takeout, CSV), as text.
#[tauri::command(async)]
fn import_file(name: String, text: String) -> Res<Vec<importer::Collection>> {
    importer::from_file(&name, &text).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
async fn charts_get(s: State<'_, AppState>, source: String, country: String, kind: String) -> Res<charts::Chart> {
    match source.as_str() {
        "deezer" => charts::deezer(&s.http, 100).await,
        "youtube" => charts::youtube(&s.http, &country).await,
        // Browse categories: `kind` is the genre id / mood key.
        "genre" => charts::apple_genre(&s.http, &country, &kind, 100).await,
        "mood" => charts::deezer_mood(&s.http, &kind, 100).await,
        _ => charts::apple(&s.http, &country, &kind, 100).await,
    }
    .map_err(|e| format!("{e:#}"))
}

// ---------------------------------------------------------------------------------------

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let handle = app.handle().clone();
            // On mobile the media service may already have started the engine (e.g. from
            // Android Auto / CarPlay); core::init returns that same instance.
            let core = core::init(&app.path().app_data_dir()?, &app.path().app_cache_dir()?)?;

            let ui = handle.clone();
            let last_playing = Mutex::new(None::<bool>);
            core.observe(move |state: &PlayerState, track_changed: bool, _json: &str| {
                let _ = ui.emit("player://state", state);
                if track_changed {
                    let _ = ui.emit("player://track", state);
                }
                let mut last = last_playing.lock();
                if track_changed || *last != Some(state.playing) {
                    *last = Some(state.playing);
                    media_controls::update(&ui, state, track_changed);
                }
            });

            // Full songs from YouTube (charts, search) play through the system yt-dlp.
            #[cfg(not(target_os = "android"))]
            audio::engine::set_stream_resolver(Box::new(youtube::fetch_stream));
            #[cfg(not(target_os = "android"))]
            audio::engine::set_link_extractor(Box::new(youtube::extract_link));
            media_controls::init(&handle, core.player.clone());
            #[cfg(target_os = "macos")]
            if let Err(e) = notch::init(&handle) {
                log::error!("notch player unavailable: {e}");
            }
            app.manage(AppState { player: core.player.clone(), library: core.library.clone(), http: core.http.clone() });

            // Phones have no folder picker that yields real paths: use the standard music locations.
            #[cfg(mobile)]
            if core.library.folders().unwrap_or_default().is_empty() {
                #[cfg(target_os = "android")]
                let defaults: Vec<std::path::PathBuf> =
                    vec!["/storage/emulated/0/Music".into(), "/storage/emulated/0/Download".into()];
                // iOS: Files app → On My iPhone → Catlify (UIFileSharingEnabled).
                #[cfg(target_os = "ios")]
                let defaults: Vec<std::path::PathBuf> = app.path().document_dir().into_iter().collect();
                for dir in defaults {
                    let _ = std::fs::create_dir_all(&dir);
                    let _ = core.library.add_folder(&dir.to_string_lossy());
                }
            }

            // Pick up files added since the last launch.
            if !core.library.folders().unwrap_or_default().is_empty() {
                spawn_scan(handle, core.library.clone());
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            player_play_queue,
            player_enqueue,
            player_remove,
            player_jump,
            player_queue,
            player_state,
            player_control,
            player_seek,
            player_set_volume,
            player_set_shuffle,
            player_set_repeat,
            library_tracks,
            library_folders,
            library_add_folder,
            library_remove_folder,
            library_rescan,
            library_is_scanning,
            library_toggle_like,
            probe_files,
            audio_extensions,
            playlists,
            playlist_create,
            playlist_rename,
            playlist_delete,
            playlist_tracks,
            playlist_add,
            playlist_remove,
            lyrics_get,
            charts_get,
            import_link,
            import_file,
            rpc,
            #[cfg(not(target_os = "android"))]
            youtube::youtube_status,
            #[cfg(not(target_os = "android"))]
            youtube::youtube_lookup,
            #[cfg(not(target_os = "android"))]
            youtube::youtube_video,
            #[cfg(not(target_os = "android"))]
            youtube::youtube_download,
            #[cfg(not(target_os = "android"))]
            youtube::youtube_cancel,
            #[cfg(desktop)]
            youtube::youtube_set_dir,
            #[cfg(target_os = "macos")]
            notch::notch_geometry,
            #[cfg(target_os = "macos")]
            notch::notch_shape,
            #[cfg(target_os = "macos")]
            notch::notch_enabled,
            #[cfg(target_os = "macos")]
            notch::notch_set_enabled,
            #[cfg(target_os = "macos")]
            notch::notch_open_app,
        ])
        .on_window_event(|window, event| {
            // macOS convention for players: closing the window keeps the music (and notch) going.
            #[cfg(target_os = "macos")]
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
            #[cfg(not(target_os = "macos"))]
            let _ = (window, event);
        })
        .build(tauri::generate_context!())
        .expect("error while running Catlify")
        .run(|app, event| {
            // Dock icon click brings the hidden window back.
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event {
                if let Some(main) = app.get_webview_window("main") {
                    let _ = main.show();
                    let _ = main.set_focus();
                }
            }
            #[cfg(not(target_os = "macos"))]
            let _ = (app, event);
        });
}

#[cfg(test)]
mod tests;
