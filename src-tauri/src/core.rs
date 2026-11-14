//! Process-wide engine shared by the Tauri UI and the native media services
//! (Android MediaLibraryService / iOS CarPlay + Now Playing). A car can start the
//! service with no UI at all, so nothing in here depends on Tauri.

use crate::audio::{Player, PlayerState};
use crate::library::Library;
use parking_lot::Mutex;
use std::path::Path;
use std::sync::{Arc, OnceLock};

/// `(state, track_changed, state_json)`
pub type Observer = Box<dyn Fn(&PlayerState, bool, &str) + Send + Sync>;

pub struct Core {
    pub player: Arc<Player>,
    pub library: Arc<Library>,
    pub http: reqwest::Client,
    pub runtime: tokio::runtime::Runtime,
    observers: Arc<Mutex<Vec<Observer>>>,
}

static CORE: OnceLock<Core> = OnceLock::new();
static INIT: Mutex<()> = Mutex::new(());

/// Idempotent: the first caller (UI or media service) wins; later callers get the same engine.
pub fn init(data_dir: &Path, cache_dir: &Path) -> anyhow::Result<&'static Core> {
    if let Some(core) = CORE.get() {
        return Ok(core);
    }
    let _guard = INIT.lock();
    if let Some(core) = CORE.get() {
        return Ok(core);
    }
    crate::audio::decoder::init();
    let library = Arc::new(Library::open(data_dir, cache_dir)?);
    if let Err(e) = library.follow_moved_home() {
        log::error!("couldn't rebase library paths: {e:#}");
    }
    crate::sync::start_if_enabled(library.clone());
    let observers: Arc<Mutex<Vec<Observer>>> = Arc::default();

    let lib = library.clone();
    let obs = observers.clone();
    let last_json = Mutex::new(String::new());
    let player = Player::new(
        cache_dir.join("previews"),
        Box::new(move |state: &PlayerState, track_changed: bool| {
            if track_changed {
                if let Some(id) = state.track.as_ref().and_then(|t| t.id) {
                    let _ = lib.mark_played(id);
                }
            }
            // Paused/stopped state is identical tick to tick; don't wake the UI or media session.
            let json = serde_json::to_string(state).unwrap_or_default();
            {
                let mut last = last_json.lock();
                if !track_changed && *last == json {
                    return;
                }
                *last = json.clone();
            }
            for o in obs.lock().iter() {
                o(state, track_changed, &json);
            }
        }),
    );

    let http = reqwest::Client::builder()
        .user_agent(concat!("Catlify/", env!("CARGO_PKG_VERSION"), " (lightweight music player)"))
        .timeout(std::time::Duration::from_secs(15))
        .build()?;
    let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build()?;

    let _ = CORE.set(Core { player, library, http, runtime, observers });
    Ok(CORE.get().unwrap())
}

pub fn get() -> Option<&'static Core> {
    CORE.get()
}

impl Core {
    pub fn observe(&self, f: impl Fn(&PlayerState, bool, &str) + Send + Sync + 'static) {
        self.observers.lock().push(Box::new(f));
    }
}
