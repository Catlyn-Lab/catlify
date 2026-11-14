//! Sleep timer: pauses playback after a number of minutes (fading out over the last few
//! seconds) or when the current song ends. Lives in the engine, not the UI, so a timer set
//! from the car or the lock screen still fires with the app closed.

use crate::audio::Player;
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const FADE: Duration = Duration::from_secs(12);
const TICK: Duration = Duration::from_millis(250);

#[derive(Clone, Copy)]
enum Mode {
    /// Pause at this moment. `ends_at_ms` is the same moment as Unix time, for the UI.
    At { deadline: Instant, ends_at_ms: u64 },
    /// Pause when the song that was playing when the timer was set is over.
    EndOfTrack { index: Option<usize> },
}

static MODE: Mutex<Option<Mode>> = Mutex::new(None);
/// Bumped on every set/cancel so an older timer thread knows to stop.
static GENERATION: AtomicU64 = AtomicU64::new(0);

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Pause after `minutes`.
pub fn set_minutes(player: &Arc<Player>, minutes: f64) {
    let after = Duration::from_secs_f64((minutes * 60.0).max(1.0));
    start(player, Mode::At { deadline: Instant::now() + after, ends_at_ms: now_ms() + after.as_millis() as u64 });
}

/// Pause when the current song finishes.
pub fn set_end_of_track(player: &Arc<Player>) {
    start(player, Mode::EndOfTrack { index: player.state().index });
}

pub fn cancel() {
    GENERATION.fetch_add(1, Ordering::SeqCst);
    *MODE.lock() = None;
}

/// `{ active, endsAt (Unix ms) | null, endOfTrack }`
pub fn status() -> Value {
    match *MODE.lock() {
        Some(Mode::At { ends_at_ms, .. }) => json!({ "active": true, "endsAt": ends_at_ms, "endOfTrack": false }),
        Some(Mode::EndOfTrack { .. }) => json!({ "active": true, "endsAt": null, "endOfTrack": true }),
        None => json!({ "active": false, "endsAt": null, "endOfTrack": false }),
    }
}

fn start(player: &Arc<Player>, mode: Mode) {
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    *MODE.lock() = Some(mode);
    let player = player.clone();
    let _ = std::thread::Builder::new().name("catlify-sleep".into()).spawn(move || run(player, mode, generation));
}

fn run(player: Arc<Player>, mode: Mode, generation: u64) {
    let current = || GENERATION.load(Ordering::SeqCst) == generation;
    // Volume before the fade started, restored after pausing (or if the timer is cancelled mid-fade).
    let mut faded_from: Option<f32> = None;
    loop {
        std::thread::sleep(TICK);
        if !current() {
            if let Some(v) = faded_from {
                player.set_volume(v);
            }
            return;
        }
        let state = player.state();
        let fire = match mode {
            Mode::At { deadline, .. } => {
                let left = deadline.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    true
                } else {
                    if left < FADE && state.playing {
                        let from = *faded_from.get_or_insert(state.volume);
                        player.set_volume(from * (left.as_secs_f32() / FADE.as_secs_f32()));
                    }
                    false
                }
            }
            // A new song started (gapless, so it's already a moment in) or the queue ran out.
            Mode::EndOfTrack { index } => state.index != index || state.ended,
        };
        if fire {
            player.pause();
            if let Mode::EndOfTrack { index } = mode {
                if state.index != index {
                    player.seek(0.0); // resuming later starts the next song from its beginning
                }
            }
            if let Some(v) = faded_from {
                std::thread::sleep(Duration::from_millis(300)); // let the pause land before restoring
                player.set_volume(v);
            }
            if current() {
                *MODE.lock() = None;
            }
            log::info!("sleep timer: paused");
            return;
        }
    }
}
