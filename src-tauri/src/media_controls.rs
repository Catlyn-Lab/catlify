//! OS media integration: media keys, macOS Now Playing, Windows SMTC overlay, Linux MPRIS.

use crate::audio::{Player, PlayerState};
use std::sync::Arc;
use tauri::AppHandle;

#[cfg(desktop)]
mod imp {
    use super::*;
    use souvlaki::{MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig, SeekDirection};
    use std::cell::RefCell;
    use std::time::Duration;

    thread_local! {
        // MediaControls is not Send on every platform, so it lives on the main thread.
        static CONTROLS: RefCell<Option<MediaControls>> = const { RefCell::new(None) };
    }

    pub fn init(app: &AppHandle, player: Arc<Player>) {
        #[cfg(target_os = "windows")]
        let hwnd = {
            use tauri::Manager;
            app.get_webview_window("main").and_then(|w| w.hwnd().ok()).map(|h| h.0 as *mut std::ffi::c_void)
        };
        #[cfg(not(target_os = "windows"))]
        let hwnd = {
            let _ = app;
            None
        };
        let config = PlatformConfig { display_name: "Catlify", dbus_name: "catlify", hwnd };
        let Ok(mut controls) = MediaControls::new(config) else { return };
        let attached = controls.attach(move |event| match event {
            MediaControlEvent::Play => player.play(),
            MediaControlEvent::Pause => player.pause(),
            MediaControlEvent::Toggle => player.toggle(),
            MediaControlEvent::Next => player.next(),
            MediaControlEvent::Previous => player.previous(),
            MediaControlEvent::Stop => player.pause(),
            MediaControlEvent::SetPosition(MediaPosition(d)) => player.seek(d.as_secs_f64()),
            MediaControlEvent::Seek(dir) => seek_by(&player, dir, 10.0),
            MediaControlEvent::SeekBy(dir, d) => seek_by(&player, dir, d.as_secs_f64()),
            MediaControlEvent::SetVolume(v) => player.set_volume(v as f32),
            _ => {}
        });
        if attached.is_ok() {
            CONTROLS.with(|c| *c.borrow_mut() = Some(controls));
        }
    }

    fn seek_by(player: &Player, dir: SeekDirection, secs: f64) {
        let delta = if matches!(dir, SeekDirection::Forward) { secs } else { -secs };
        player.seek((player.state().position + delta).max(0.0));
    }

    pub fn update(app: &AppHandle, state: &PlayerState, metadata_changed: bool) {
        let state = state.clone();
        let _ = app.run_on_main_thread(move || {
            CONTROLS.with(|c| {
                let mut c = c.borrow_mut();
                let Some(controls) = c.as_mut() else { return };
                if metadata_changed {
                    if let Some(t) = &state.track {
                        let cover = t.cover.as_ref().map(|c| {
                            if c.starts_with("http") { c.clone() } else { format!("file://{c}") }
                        });
                        let _ = controls.set_metadata(MediaMetadata {
                            title: Some(&t.title),
                            artist: Some(&t.artist),
                            album: Some(&t.album),
                            cover_url: cover.as_deref(),
                            duration: Some(Duration::from_secs_f64(state.duration.max(0.0))),
                        });
                    }
                }
                let progress = Some(MediaPosition(Duration::from_secs_f64(state.position.max(0.0))));
                let _ = controls.set_playback(match (&state.track, state.playing) {
                    (None, _) => MediaPlayback::Stopped,
                    (Some(_), true) => MediaPlayback::Playing { progress },
                    (Some(_), false) => MediaPlayback::Paused { progress },
                });
            });
        });
    }
}

#[cfg(mobile)]
mod imp {
    // On phones the native media services own this (Android: gen/android/.../media/StafyMediaService.kt,
    // iOS: gen/apple/Sources/stafy/media/NowPlaying.swift); they observe the engine via core::observe.
    use super::*;
    pub fn init(_app: &AppHandle, _player: Arc<Player>) {}
    pub fn update(_app: &AppHandle, _state: &PlayerState, _metadata_changed: bool) {}
}

pub use imp::{init, update};
