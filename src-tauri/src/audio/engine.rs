//! Gapless playback engine.
//!
//! One decode thread owns the cpal output stream (which is !Send on some platforms)
//! and feeds a lock-free ring buffer. When a track ends it immediately opens the next
//! one and records the frame boundary, so the status thread can switch the "now playing"
//! track exactly when the audio callback reaches it.

use super::decoder::{Decoder, StreamInfo};
use crossbeam_channel::{Receiver, Sender};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use parking_lot::Mutex;
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QueueItem {
    pub id: Option<i64>,
    pub path: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: f64,
    pub cover: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Repeat {
    #[default]
    Off,
    All,
    One,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PlayerState {
    pub playing: bool,
    pub position: f64,
    pub duration: f64,
    pub index: Option<usize>,
    pub track: Option<QueueItem>,
    pub volume: f32,
    pub shuffle: bool,
    pub repeat: Repeat,
    pub stream: Option<StreamInfo>,
    pub error: Option<String>,
    pub queue_version: u64,
    /// Reached the end of the queue (repeat off). Media sessions report this as "ended".
    pub ended: bool,
    /// The current track is still being fetched (YouTube streams take a few seconds).
    pub loading: bool,
}

enum Cmd {
    /// Start decoding the queue entry at `pos` (position in play order) from `start` seconds.
    Load { pos: usize, start: f64, resume: bool },
    Seek(f64),
    Stop,
}

struct Boundary {
    frame: u64,
    pos: usize,
    info: StreamInfo,
}

#[derive(Default)]
struct Inner {
    queue: Vec<QueueItem>,
    /// Play order: indices into `queue` (identity unless shuffled).
    order: Vec<usize>,
    shuffle: bool,
    repeat: Repeat,
    /// Order position currently audible.
    playing: Option<usize>,
    /// Order position the decode thread is working on (may be ahead, for gapless).
    decoding: Option<usize>,
    boundaries: VecDeque<Boundary>,
    base_frame: u64,
    base_offset: f64,
    /// Frame at which the last track ends with nothing queued after it.
    end_frame: Option<u64>,
    ended: bool,
    stream: Option<StreamInfo>,
    error: Option<String>,
    queue_version: u64,
    /// Order position being opened right now (slow for YouTube sources).
    loading: Option<usize>,
}

impl Inner {
    fn next_pos(&self, from: usize, manual: bool) -> Option<usize> {
        if self.order.is_empty() {
            return None;
        }
        match self.repeat {
            Repeat::One if !manual => Some(from),
            Repeat::Off if from + 1 >= self.order.len() => None,
            _ => Some((from + 1) % self.order.len()),
        }
    }

    fn item_at(&self, pos: usize) -> Option<&QueueItem> {
        self.order.get(pos).and_then(|&i| self.queue.get(i))
    }

    fn rebuild_order(&mut self, keep_current: Option<usize>) {
        let n = self.queue.len();
        let mut order: Vec<usize> = (0..n).collect();
        if self.shuffle {
            order.shuffle(&mut rand::thread_rng());
            if let Some(cur) = keep_current {
                if let Some(p) = order.iter().position(|&i| i == cur) {
                    order.swap(0, p);
                }
            }
        }
        self.order = order;
        let pos = keep_current.and_then(|cur| self.order.iter().position(|&i| i == cur));
        self.playing = pos;
        self.queue_version += 1;
    }

    fn position_at(&self, played: u64, rate: u32) -> f64 {
        self.base_offset + played.saturating_sub(self.base_frame) as f64 / rate.max(1) as f64
    }

    /// True when the decode thread has already moved past the audible track
    /// (opened the next one, or hit the end of the queue).
    fn has_prefetch(&self) -> bool {
        !self.boundaries.is_empty() || self.end_frame.is_some()
    }

    /// Drops a gapless prefetch that may no longer match the queue.
    fn invalidate_prefetch(&mut self) {
        self.boundaries.clear();
        self.end_frame = None;
        self.decoding = self.playing;
    }
}

struct Shared {
    /// Set by the audio callback's error handler (e.g. AAudio "disconnected" when a car's
    /// Bluetooth connects) or when the default output device changes.
    device_lost: AtomicBool,
    played: AtomicU64,
    flush: AtomicBool,
    paused: AtomicBool,
    volume: AtomicU32,
    out_rate: AtomicU32,
}

pub type StateListener = Box<dyn Fn(&PlayerState, bool) + Send + Sync>;

pub struct Player {
    inner: Arc<Mutex<Inner>>,
    shared: Arc<Shared>,
    tx: Sender<Cmd>,
}

impl Player {
    /// `listener(state, track_changed)` is invoked from the status thread ~5x per second.
    /// `preview_dir` caches remote sources (chart previews) so FFmpeg only ever reads local files.
    pub fn new(preview_dir: std::path::PathBuf, listener: StateListener) -> Arc<Self> {
        let shared = Arc::new(Shared {
            device_lost: AtomicBool::new(false),
            played: AtomicU64::new(0),
            flush: AtomicBool::new(false),
            paused: AtomicBool::new(true),
            volume: AtomicU32::new(0.8f32.to_bits()),
            out_rate: AtomicU32::new(48_000),
        });
        let inner = Arc::new(Mutex::new(Inner::default()));
        let (tx, rx) = crossbeam_channel::unbounded();

        {
            let (inner, shared) = (inner.clone(), shared.clone());
            std::thread::Builder::new()
                .name("stafy-decode".into())
                .spawn(move || decode_thread(rx, inner, shared, preview_dir))
                .expect("spawn decode thread");
        }

        let player = Arc::new(Self { inner, shared, tx });
        {
            let p = player.clone();
            std::thread::Builder::new()
                .name("stafy-status".into())
                .spawn(move || p.status_loop(listener))
                .expect("spawn status thread");
        }
        player
    }

    fn status_loop(&self, listener: StateListener) {
        let mut last_pos: Option<usize> = None;
        loop {
            std::thread::sleep(Duration::from_millis(200));
            let state = self.tick();
            let changed = state.index.is_some() && state.index != last_pos;
            last_pos = state.index;
            listener(&state, changed);
        }
    }

    /// Advances "now playing" across gapless boundaries and returns a snapshot.
    fn tick(&self) -> PlayerState {
        let played = self.shared.played.load(Ordering::Relaxed);
        let mut inner = self.inner.lock();
        while inner.boundaries.front().is_some_and(|b| played >= b.frame) {
            let b = inner.boundaries.pop_front().unwrap();
            inner.playing = Some(b.pos);
            inner.base_frame = b.frame;
            inner.base_offset = 0.0;
            inner.stream = Some(b.info);
        }
        if inner.end_frame.is_some_and(|f| played >= f) {
            inner.end_frame = None;
            inner.ended = true;
            self.shared.paused.store(true, Ordering::Relaxed);
        }
        self.snapshot(&inner, played)
    }

    fn snapshot(&self, inner: &Inner, played: u64) -> PlayerState {
        let rate = self.shared.out_rate.load(Ordering::Relaxed).max(1) as f64;
        // While a song is being fetched, show that song (not the one before it) at 0:00.
        let loading = inner.loading.filter(|p| inner.item_at(*p).is_some());
        let shown = loading.or(inner.playing);
        let track = shown.and_then(|p| inner.item_at(p).cloned());
        let duration = if loading.is_some() {
            track.as_ref().map(|t| t.duration).unwrap_or(0.0)
        } else {
            inner.stream.as_ref().map(|s| s.duration).filter(|d| *d > 0.0).or(track.as_ref().map(|t| t.duration)).unwrap_or(0.0)
        };
        let mut position = inner.base_offset + played.saturating_sub(inner.base_frame) as f64 / rate;
        if loading.is_some() {
            position = 0.0;
        } else if inner.ended || duration > 0.0 && position > duration {
            position = duration;
        }
        PlayerState {
            playing: !self.shared.paused.load(Ordering::Relaxed),
            position,
            duration,
            index: shown.and_then(|p| inner.order.get(p).copied()),
            track,
            volume: f32::from_bits(self.shared.volume.load(Ordering::Relaxed)),
            shuffle: inner.shuffle,
            repeat: inner.repeat,
            stream: if loading.is_some() { None } else { inner.stream.clone() },
            error: inner.error.clone(),
            queue_version: inner.queue_version,
            ended: inner.ended,
            loading: loading.is_some(),
        }
    }

    pub fn state(&self) -> PlayerState {
        self.tick()
    }

    fn load(&self, pos: usize, start: f64) {
        // Loading starts now (not when the decode thread gets to it), so the UI shows it right away.
        {
            let mut g = self.inner.lock();
            // A newer choice replaces whatever was loading.
            g.loading = g.item_at(pos).filter(|i| is_remote(&i.path)).map(|_| pos);
        }
        let _ = self.tx.send(Cmd::Load { pos, start, resume: true });
    }

    pub fn play_queue(&self, items: Vec<QueueItem>, start_index: usize) {
        let mut inner = self.inner.lock();
        inner.queue = items;
        let start = start_index.min(inner.queue.len().saturating_sub(1));
        inner.rebuild_order(Some(start));
        inner.invalidate_prefetch();
        if let Some(pos) = inner.playing {
            drop(inner);
            self.load(pos, 0.0);
        }
    }

    pub fn enqueue(&self, items: Vec<QueueItem>, play_next: bool) {
        let mut inner = self.inner.lock();
        if inner.queue.is_empty() {
            drop(inner);
            return self.play_queue(items, 0);
        }
        let current = inner.playing.and_then(|p| inner.order.get(p).copied());
        let at = match (play_next, current) {
            (true, Some(c)) => c + 1,
            _ => inner.queue.len(),
        };
        let n = items.len();
        inner.queue.splice(at..at, items);
        if !inner.shuffle {
            inner.rebuild_order(current);
        } else {
            // Shuffled: insert new entries right after the current position or at the end.
            let mut order: Vec<usize> = inner.order.iter().map(|&i| if i >= at { i + n } else { i }).collect();
            let insert_at = if play_next { inner.playing.map_or(0, |p| p + 1) } else { order.len() };
            order.splice(insert_at..insert_at, at..at + n);
            inner.order = order;
            inner.queue_version += 1;
        }
        // Indices shifted, so a pending boundary's position is stale either way.
        self.restart_prefetch(inner);
    }

    pub fn remove_from_queue(&self, index: usize) {
        let mut inner = self.inner.lock();
        if index >= inner.queue.len() {
            return;
        }
        let current = inner.playing.and_then(|p| inner.order.get(p).copied());
        if current == Some(index) {
            drop(inner);
            return self.next();
        }
        inner.queue.remove(index);
        let order: Vec<usize> = inner
            .order
            .iter()
            .filter(|&&i| i != index)
            .map(|&i| if i > index { i - 1 } else { i })
            .collect();
        inner.order = order;
        let cur = current.map(|c| if c > index { c - 1 } else { c });
        inner.playing = cur.and_then(|c| inner.order.iter().position(|&i| i == c));
        inner.queue_version += 1;
        self.restart_prefetch(inner);
    }

    /// The decode thread may already have prefetched a track that is no longer next.
    /// Re-seek the current track at the current position so gapless picks the new next one.
    fn restart_prefetch(&self, mut inner: parking_lot::MutexGuard<'_, Inner>) {
        // Keep the decode thread's view consistent after `order` changed.
        let had_prefetch = inner.has_prefetch();
        inner.invalidate_prefetch();
        let Some(pos) = inner.playing else { return };
        if inner.ended || !had_prefetch {
            return;
        }
        let position = self.snapshot(&inner, self.shared.played.load(Ordering::Relaxed)).position;
        drop(inner);
        let resume = !self.shared.paused.load(Ordering::Relaxed);
        let _ = self.tx.send(Cmd::Load { pos, start: position, resume });
    }

    pub fn queue(&self) -> Vec<(usize, QueueItem)> {
        let inner = self.inner.lock();
        inner.order.iter().map(|&i| (i, inner.queue[i].clone())).collect()
    }

    pub fn jump(&self, index: usize) {
        let inner = self.inner.lock();
        let pos = inner.order.iter().position(|&i| i == index);
        drop(inner);
        if let Some(pos) = pos {
            self.load(pos, 0.0);
        }
    }

    pub fn play(&self) {
        let inner = self.inner.lock();
        if inner.ended {
            let pos = inner.playing;
            drop(inner);
            if let Some(pos) = pos {
                self.load(pos, 0.0);
            }
            return;
        }
        if inner.playing.is_some() {
            self.shared.paused.store(false, Ordering::Relaxed);
        }
    }

    pub fn pause(&self) {
        self.shared.paused.store(true, Ordering::Relaxed);
    }

    pub fn toggle(&self) {
        if self.shared.paused.load(Ordering::Relaxed) {
            self.play()
        } else {
            self.pause()
        }
    }

    pub fn next(&self) {
        let inner = self.inner.lock();
        let next = inner.playing.and_then(|p| inner.next_pos(p, true));
        drop(inner);
        match next {
            Some(pos) => self.load(pos, 0.0),
            None => {
                self.pause();
                let _ = self.tx.send(Cmd::Stop);
            }
        }
    }

    pub fn previous(&self) {
        let state = self.state();
        let inner = self.inner.lock();
        let Some(p) = inner.playing else { return };
        let pos = if state.position > 3.0 || p == 0 && inner.repeat != Repeat::All {
            p
        } else if p == 0 {
            inner.order.len() - 1
        } else {
            p - 1
        };
        drop(inner);
        self.load(pos, 0.0);
    }

    pub fn seek(&self, secs: f64) {
        let inner = self.inner.lock();
        let ended = inner.ended;
        let playing = inner.playing;
        drop(inner);
        if ended {
            if let Some(pos) = playing {
                let resume = !self.shared.paused.load(Ordering::Relaxed);
                let _ = self.tx.send(Cmd::Load { pos, start: secs, resume });
                return;
            }
        }
        let _ = self.tx.send(Cmd::Seek(secs));
    }

    pub fn set_volume(&self, v: f32) {
        self.shared.volume.store(v.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    pub fn set_shuffle(&self, on: bool) {
        let mut inner = self.inner.lock();
        if inner.shuffle == on {
            return;
        }
        inner.shuffle = on;
        let current = inner.playing.and_then(|p| inner.order.get(p).copied());
        inner.rebuild_order(current);
        self.restart_prefetch(inner);
    }

    pub fn set_repeat(&self, repeat: Repeat) {
        let mut inner = self.inner.lock();
        inner.repeat = repeat;
        self.restart_prefetch(inner);
    }
}

// ---------------------------------------------------------------------------------------

fn decode_thread(rx: Receiver<Cmd>, inner: Arc<Mutex<Inner>>, shared: Arc<Shared>, preview_dir: std::path::PathBuf) {
    let open_at = |path: &str, rate: u32, channels: u16, start: f64| open_at(path, &preview_dir, rate, channels, start);
    // Placeholder so the loop still runs (and reports errors) without an output device.
    let (mut producer, _placeholder) = rtrb::RingBuffer::<f32>::new(1);
    let mut stream: Option<cpal::Stream> = None;
    let mut device_id: Option<String> = None;
    let mut channels: u16 = 2;
    match open_output(&shared) {
        Ok((s, p, ch, id)) => {
            producer = p;
            channels = ch;
            stream = Some(s);
            device_id = id;
        }
        Err(e) => inner.lock().error = Some(format!("Audio output unavailable: {e}")),
    }
    let mut out_rate = shared.out_rate.load(Ordering::Relaxed);
    let mut pending: Option<Cmd> = None;
    let mut last_device_check = std::time::Instant::now();

    let mut decoder: Option<Decoder> = None;
    let mut buf: Vec<f32> = Vec::with_capacity(16 * 1024);
    let mut buf_read = 0usize;
    let mut produced: u64 = 0;

    let flush = |shared: &Shared| {
        shared.flush.store(true, Ordering::Release);
        for _ in 0..200 {
            if !shared.flush.load(Ordering::Acquire) {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        shared.flush.store(false, Ordering::Release);
        shared.played.load(Ordering::Relaxed)
    };

    loop {
        // Desktop: follow the system default output (headphones, AirPods, USB DAC).
        if last_device_check.elapsed() > Duration::from_secs(1) {
            last_device_check = std::time::Instant::now();
            if !cfg!(target_os = "android") && default_device_id() != device_id {
                shared.device_lost.store(true, Ordering::Relaxed);
            }
        }
        if shared.device_lost.swap(false, Ordering::Relaxed) {
            let rate_before = out_rate;
            drop(stream.take());
            // Give the OS a moment to finish routing (Bluetooth A2DP setup).
            std::thread::sleep(Duration::from_millis(300));
            match open_output(&shared) {
                Ok((s, p, ch, id)) => {
                    producer = p;
                    channels = ch;
                    stream = Some(s);
                    device_id = id;
                    out_rate = shared.out_rate.load(Ordering::Relaxed);
                    // Restart the audible track where it was, at the new device's rate.
                    let g = inner.lock();
                    if let (Some(pos), false) = (g.playing, g.ended) {
                        let start = g.position_at(shared.played.load(Ordering::Relaxed), rate_before);
                        let resume = !shared.paused.load(Ordering::Relaxed);
                        pending = Some(Cmd::Load { pos, start, resume });
                    }
                }
                Err(e) => {
                    inner.lock().error = Some(format!("Audio output unavailable: {e}"));
                    shared.device_lost.store(true, Ordering::Relaxed);
                    std::thread::sleep(Duration::from_millis(700));
                    continue;
                }
            }
        }

        // Block when idle (waking periodically to watch the output device).
        let cmd = if let Some(c) = pending.take() {
            Some(c)
        } else if decoder.is_none() && buf_read >= buf.len() {
            rx.recv_timeout(Duration::from_millis(1000)).ok()
        } else {
            rx.try_recv().ok()
        };
        if let Some(cmd) = cmd {
            match cmd {
                Cmd::Load { pos, start, resume } => {
                    let item = inner.lock().item_at(pos).cloned();
                    buf.clear();
                    buf_read = 0;
                    produced = flush(&shared);
                    decoder = None;
                    let Some(item) = item else { continue };
                    let opened = if is_remote(&item.path) {
                        inner.lock().loading = Some(pos);
                        // Fetch on a helper thread so a newer choice isn't stuck behind this one
                        // (YouTube takes seconds); the abandoned fetch still lands in the cache.
                        let (ftx, frx) = crossbeam_channel::bounded(1);
                        let (path, dir) = (item.path.clone(), preview_dir.clone());
                        std::thread::spawn(move || {
                            let _ = ftx.send(local_source(&path, &dir));
                        });
                        let fetched = loop {
                            match frx.recv_timeout(Duration::from_millis(100)) {
                                Ok(r) => break Some(r),
                                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                                    if let Ok(newer) = rx.try_recv() {
                                        pending = Some(newer);
                                        break None;
                                    }
                                }
                                Err(_) => break Some(Err(anyhow::anyhow!("fetch stopped"))),
                            }
                        };
                        let Some(fetched) = fetched else { continue }; // superseded
                        fetched.and_then(|local| open_local(&local, out_rate, channels, start))
                    } else {
                        open_at(&item.path, out_rate, channels, start)
                    };
                    {
                        let mut g = inner.lock();
                        if g.loading == Some(pos) {
                            g.loading = None;
                        }
                    }
                    match opened {
                        Ok(d) => {
                            let mut g = inner.lock();
                            warm_after(&g, pos, &preview_dir);
                            g.playing = Some(pos);
                            g.decoding = Some(pos);
                            g.boundaries.clear();
                            g.end_frame = None;
                            g.ended = false;
                            g.base_frame = produced;
                            g.base_offset = start;
                            g.stream = Some(d.info.clone());
                            g.error = None;
                            decoder = Some(d);
                            if resume {
                                shared.paused.store(false, Ordering::Relaxed);
                            }
                        }
                        Err(e) => {
                            let mut g = inner.lock();
                            g.playing = Some(pos);
                            g.error = Some(format!("{}: {e:#}", item.title));
                            g.ended = true;
                            shared.paused.store(true, Ordering::Relaxed);
                        }
                    }
                }
                Cmd::Seek(secs) => {
                    let (playing, decoding) = {
                        let g = inner.lock();
                        (g.playing, g.decoding)
                    };
                    let Some(pos) = playing else { continue };
                    buf.clear();
                    buf_read = 0;
                    produced = flush(&shared);
                    let reuse = playing == decoding && decoder.is_some();
                    let result = if reuse {
                        decoder.as_mut().unwrap().seek(secs).map(|_| ())
                    } else {
                        let path = inner.lock().item_at(pos).map(|i| i.path.clone());
                        match path.map(|p| open_at(&p, out_rate, channels, secs)) {
                            Some(Ok(d)) => {
                                decoder = Some(d);
                                Ok(())
                            }
                            Some(Err(e)) => Err(e),
                            None => Ok(()),
                        }
                    };
                    let mut g = inner.lock();
                    g.decoding = playing;
                    g.boundaries.clear();
                    g.end_frame = None;
                    g.base_frame = produced;
                    g.base_offset = secs;
                    if let Err(e) = result {
                        g.error = Some(format!("{e:#}"));
                    }
                }
                Cmd::Stop => {
                    buf.clear();
                    buf_read = 0;
                    produced = flush(&shared);
                    decoder = None;
                    let mut g = inner.lock();
                    g.boundaries.clear();
                    g.end_frame = None;
                    g.ended = true;
                }
            }
            continue;
        }
        if decoder.is_none() && buf_read >= buf.len() {
            continue;
        }

        // Push already-decoded samples into the ring buffer.
        if buf_read < buf.len() {
            let free = producer.slots();
            if free == 0 {
                std::thread::sleep(Duration::from_millis(5));
                continue;
            }
            let n = free.min(buf.len() - buf_read);
            // Whole frames only, so frame accounting stays exact.
            let n = n - n % channels as usize;
            if n == 0 {
                std::thread::sleep(Duration::from_millis(2));
                continue;
            }
            if let Ok(chunk) = producer.write_chunk_uninit(n) {
                chunk.fill_from_iter(buf[buf_read..buf_read + n].iter().copied());
            }
            buf_read += n;
            produced += (n / channels as usize) as u64;
            continue;
        }

        let Some(dec) = decoder.as_mut() else { continue };
        buf.clear();
        buf_read = 0;
        match dec.decode_next(&mut buf) {
            Ok(true) => {}
            Ok(false) | Err(_) => {
                // Track finished: open the next one immediately (gapless).
                decoder = None;
                let mut g = inner.lock();
                let next = g.decoding.and_then(|p| g.next_pos(p, false));
                let mut opened = None;
                if let Some(pos) = next {
                    if let Some(item) = g.item_at(pos).cloned() {
                        drop(g);
                        let res = open_at(&item.path, out_rate, channels, 0.0);
                        g = inner.lock();
                        match res {
                            Ok(d) => opened = Some((pos, d)),
                            Err(e) => g.error = Some(format!("{}: {e:#}", item.title)),
                        }
                    }
                }
                match opened {
                    Some((pos, d)) => {
                        warm_after(&g, pos, &preview_dir);
                        g.boundaries.push_back(Boundary { frame: produced, pos, info: d.info.clone() });
                        g.decoding = Some(pos);
                        decoder = Some(d);
                    }
                    None => g.end_frame = Some(produced),
                }
            }
        }
    }
}

fn open_at(path: &str, preview_dir: &std::path::Path, rate: u32, channels: u16, start: f64) -> anyhow::Result<Decoder> {
    open_local(&local_source(path, preview_dir)?, rate, channels, start)
}

fn open_local(local: &str, rate: u32, channels: u16, start: f64) -> anyhow::Result<Decoder> {
    let mut d = Decoder::open(local, rate, channels)?;
    if start > 0.05 {
        d.seek(start)?;
    }
    Ok(d)
}

type Output = (cpal::Stream, rtrb::Producer<f32>, u16, Option<String>);

fn default_device_id() -> Option<String> {
    cpal::default_host().default_output_device().and_then(|d| d.id().ok()).map(|id| format!("{id:?}"))
}

fn open_output(shared: &Arc<Shared>) -> anyhow::Result<Output> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| anyhow::anyhow!("no output device"))?;
    let id = device.id().ok().map(|id| format!("{id:?}"));
    let supported = device.default_output_config()?;
    let format = supported.sample_format();
    let mut config = supported.config();
    // Down-mix anything exotic to stereo; mono devices stay mono.
    config.channels = config.channels.min(2);
    shared.out_rate.store(config.sample_rate, Ordering::Relaxed);

    // ~1s of audio: enough to ride out scheduling hiccups, small enough to keep RAM low.
    let (producer, consumer) =
        rtrb::RingBuffer::<f32>::new(config.sample_rate as usize * config.channels as usize);

    let stream = match format {
        cpal::SampleFormat::F32 => build::<f32>(&device, config.clone(), consumer, shared.clone()),
        cpal::SampleFormat::I16 => build::<i16>(&device, config.clone(), consumer, shared.clone()),
        cpal::SampleFormat::I32 => build::<i32>(&device, config.clone(), consumer, shared.clone()),
        cpal::SampleFormat::U16 => build::<u16>(&device, config.clone(), consumer, shared.clone()),
        other => anyhow::bail!("unsupported sample format {other:?}"),
    }?;
    stream.play()?;
    Ok((stream, producer, config.channels, id))
}

fn build<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    mut consumer: rtrb::Consumer<f32>,
    shared: Arc<Shared>,
) -> anyhow::Result<cpal::Stream>
where
    T: SizedSample + FromSample<f32> + Send + 'static,
{
    let channels = config.channels as usize;
    let silence = T::from_sample(0.0f32);
    let shared_for_errors = shared.clone();
    let stream = device.build_output_stream::<T, _, _>(
        config,
        move |data: &mut [T], _| {
            if shared.flush.load(Ordering::Acquire) {
                let n = consumer.slots();
                if let Ok(chunk) = consumer.read_chunk(n) {
                    chunk.commit_all();
                }
                shared.flush.store(false, Ordering::Release);
            }
            if shared.paused.load(Ordering::Relaxed) {
                data.fill(silence);
                return;
            }
            let volume = f32::from_bits(shared.volume.load(Ordering::Relaxed));
            // Perceptual (squared) volume curve.
            let gain = volume * volume;
            let available = consumer.slots().min(data.len());
            let available = available - available % channels;
            if let Ok(chunk) = consumer.read_chunk(available) {
                for (out, s) in data.iter_mut().zip(chunk.into_iter()) {
                    *out = T::from_sample(s * gain);
                }
            }
            data[available..].fill(silence);
            shared.played.fetch_add((available / channels) as u64, Ordering::Relaxed);
        },
        {
            let shared = shared_for_errors;
            move |e| {
                log::warn!("audio stream error, reopening output: {e}");
                shared.device_lost.store(true, Ordering::Relaxed);
            }
        },
        None,
    )?;
    Ok(stream)
}

// ---- remote sources --------------------------------------------------------------------
//
// Queue paths can be local files, http(s) URLs (chart previews), or YouTube sources:
//   "yt:<videoId>"   a specific video          (YouTube Music charts, YouTube search results)
//   "ytq:<query>"    best YouTube Music song   (Apple/Deezer chart entries)
// optionally followed by "#<preview url>" to fall back on when YouTube can't be reached.
// The platform registers how to fetch YouTube audio (desktop: yt-dlp CLI, Android: bundled yt-dlp).

/// Fetches YouTube audio for `spec` ("yt:…"/"ytq:…") into `stem` + extension; returns the file.
pub type StreamResolver = Box<dyn Fn(&str, &std::path::Path) -> anyhow::Result<std::path::PathBuf> + Send + Sync>;

static RESOLVER: std::sync::OnceLock<StreamResolver> = std::sync::OnceLock::new();
/// One fetch per source at a time: the warm-up thread and playback share the result.
static FETCH_LOCKS: std::sync::LazyLock<Mutex<std::collections::HashMap<String, Arc<Mutex<()>>>>> =
    std::sync::LazyLock::new(Default::default);
/// YouTube songs kept in the cache (~4 MB each).
const YT_CACHE_FILES: usize = 60;

pub fn set_stream_resolver(r: StreamResolver) {
    let _ = RESOLVER.set(r);
}

// ---- fast path: look up the audio link with yt-dlp, fetch it ourselves --------------------
//
// Measured: yt-dlp needs ~4.5 s to find the audio link, but the song itself downloads in ~1 s
// once the link is known. Links are looked up early for likely taps (`prepare`) and cached until
// they expire, so starting a song is mostly just the fetch.

/// Returns yt-dlp's JSON (`-J`) for a "yt:"/"ytq:" spec (platform: CLI on desktop, bundled on Android).
pub type LinkExtractor = Box<dyn Fn(&str) -> anyhow::Result<String> + Send + Sync>;

static EXTRACTOR: std::sync::OnceLock<LinkExtractor> = std::sync::OnceLock::new();

pub fn set_link_extractor(x: LinkExtractor) {
    let _ = EXTRACTOR.set(x);
}

#[derive(Clone, Debug)]
pub struct StreamLink {
    url: String,
    headers: Vec<(String, String)>,
    ext: String,
    size: Option<u64>,
    expires: u64,
    /// The YouTube video the link belongs to (a "ytq:" search resolves to one), for downloads.
    video_id: Option<String>,
    /// YouTube refuses a brand-new link for ~2 s (measured), so fetches wait until it's ready.
    created: std::time::Instant,
}

/// How long YouTube takes to accept a freshly looked-up link.
const LINK_WARMUP: Duration = Duration::from_millis(2300);

static LINKS: std::sync::LazyLock<Mutex<std::collections::HashMap<String, StreamLink>>> =
    std::sync::LazyLock::new(Default::default);
/// Specs being looked up right now, so `prepare` and playback don't both run yt-dlp.
static LINK_LOCKS: std::sync::LazyLock<Mutex<std::collections::HashMap<String, Arc<Mutex<()>>>>> =
    std::sync::LazyLock::new(Default::default);
/// At most this many background lookups at once (yt-dlp is heavy on phones).
const PREPARE_SLOTS: usize = 2;
static PREPARING: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Parses yt-dlp `-J` output (a video, or a search whose first entry is the match).
pub fn parse_link(json: &str) -> anyhow::Result<StreamLink> {
    let v: serde_json::Value = serde_json::from_str(json)?;
    let mut o = v.get("entries").and_then(|e| e.as_array()).and_then(|e| e.first()).unwrap_or(&v);
    let video_id = o.get("id").and_then(|i| i.as_str()).map(str::to_string);
    if let Some(f) = o.get("requested_formats").and_then(|f| f.as_array()).and_then(|f| f.first()) {
        o = f;
    }
    let url = o.get("url").and_then(|u| u.as_str()).ok_or_else(|| anyhow::anyhow!("no audio link"))?.to_string();
    let headers = o
        .get("http_headers")
        .and_then(|h| h.as_object())
        .map(|h| h.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect())
        .unwrap_or_default();
    let ext = o.get("ext").and_then(|e| e.as_str()).unwrap_or("m4a").to_string();
    let size = o.get("filesize").or_else(|| o.get("filesize_approx")).and_then(|s| s.as_u64());
    // YouTube links carry their expiry; keep a safety margin.
    let expires = url
        .split(['?', '&'])
        .find_map(|p| p.strip_prefix("expire="))
        .and_then(|e| e.parse::<u64>().ok())
        .unwrap_or(now_secs() + 3600)
        .saturating_sub(600);
    Ok(StreamLink { url, headers, ext, size, expires, video_id, created: std::time::Instant::now() })
}

/// The exact YouTube video behind a queue path ("yt:<id>" or "ytq:<search>", "#preview" ignored):
/// the one that played if it was looked up already, otherwise looked up now (a few seconds).
/// Downloads use it so "Download" saves the very song that's playing, with no search step.
pub fn video_id_for(path: &str) -> anyhow::Result<String> {
    let spec = path.split('#').next().unwrap_or(path);
    if let Some(id) = spec.strip_prefix("yt:") {
        return Ok(id.to_string());
    }
    if !spec.starts_with("ytq:") {
        anyhow::bail!("not a YouTube song");
    }
    // An expired link still names the right video.
    if let Some(id) = LINKS.lock().get(spec).and_then(|l| l.video_id.clone()) {
        return Ok(id);
    }
    link_for(spec)?.video_id.ok_or_else(|| anyhow::anyhow!("YouTube didn't say which video it found"))
}

/// The audio link for `spec`, from cache or looked up now.
fn link_for(spec: &str) -> anyhow::Result<StreamLink> {
    let lock = LINK_LOCKS.lock().entry(spec.to_string()).or_default().clone();
    let _guard = lock.lock();
    if let Some(l) = LINKS.lock().get(spec).filter(|l| l.expires > now_secs()) {
        return Ok(l.clone());
    }
    let x = EXTRACTOR.get().ok_or_else(|| anyhow::anyhow!("no link extractor"))?;
    let link = parse_link(&x(spec)?)?;
    LINKS.lock().insert(spec.to_string(), link.clone());
    Ok(link)
}

/// Looks up links in the background for songs the user is likely to play (top search result,
/// Billboard rows on screen, hovered rows). Cheap: no audio is downloaded. Queued newest-first
/// (what's on screen now matters most) with at most PREPARE_SLOTS yt-dlp runs at a time.
pub fn prepare(specs: &[String]) {
    if EXTRACTOR.get().is_none() {
        return;
    }
    {
        let mut q = PREPARE_QUEUE.lock();
        for spec in specs.iter().rev() {
            let spec = spec.split('#').next().unwrap_or(spec).to_string();
            if !(spec.starts_with("yt:") || spec.starts_with("ytq:")) || LINKS.lock().get(&spec).is_some_and(|l| l.expires > now_secs()) {
                continue;
            }
            q.retain(|s| *s != spec);
            q.push_front(spec);
        }
        q.truncate(PREPARE_QUEUE_MAX); // scrolled-past songs aren't worth looking up anymore
    }
    while PREPARING.load(Ordering::SeqCst) < PREPARE_SLOTS && !PREPARE_QUEUE.lock().is_empty() {
        PREPARING.fetch_add(1, Ordering::SeqCst);
        std::thread::spawn(|| {
            loop {
                // Own statement: the queue must be unlocked while yt-dlp runs (a `while let`
                // would hold the lock for the whole loop body and block every prepare() call).
                let next = PREPARE_QUEUE.lock().pop_front();
                let Some(spec) = next else { break };
                if let Err(e) = link_for(&spec) {
                    log::debug!("prepare {spec}: {e:#}");
                }
            }
            PREPARING.fetch_sub(1, Ordering::SeqCst);
        });
    }
}

static PREPARE_QUEUE: std::sync::LazyLock<Mutex<std::collections::VecDeque<String>>> = std::sync::LazyLock::new(Default::default);
const PREPARE_QUEUE_MAX: usize = 12;

/// Downloads the audio from a looked-up link in 10 MB pieces (YouTube's own range scheme).
fn fetch_link(link: &StreamLink, stem: &std::path::Path) -> anyhow::Result<std::path::PathBuf> {
    use std::io::Write;
    // YouTube locks the link to the address yt-dlp used (`ip=`): connect over the same IP version,
    // or the request is refused.
    let ipv6 = link.url.split(['?', '&']).find_map(|p| p.strip_prefix("ip=")).is_some_and(|ip| ip.contains("%3A") || ip.contains(':'));
    let local: std::net::IpAddr = if ipv6 { std::net::Ipv6Addr::UNSPECIFIED.into() } else { std::net::Ipv4Addr::UNSPECIFIED.into() };
    let client = reqwest::blocking::Client::builder()
        .local_address(local)
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(90))
        .build()?;
    let file = stem.with_extension(&link.ext);
    let tmp = stem.with_extension(format!("{}.part", link.ext));
    let mut out = std::fs::File::create(&tmp)?;
    const CHUNK: u64 = 10 * 1024 * 1024;
    if let Some(wait) = LINK_WARMUP.checked_sub(link.created.elapsed()) {
        std::thread::sleep(wait);
    }
    let mut start = 0u64;
    loop {
        let end = match link.size {
            Some(size) if start >= size => break,
            Some(size) => (start + CHUNK).min(size) - 1,
            None => start + CHUNK - 1,
        };
        let get = || {
            let mut req = client.get(&link.url).header("Range", format!("bytes={start}-{end}")).header("Accept-Encoding", "identity");
            for (k, v) in &link.headers {
                req = req.header(k, v);
            }
            req.send()
        };
        // A link can still be refused for a moment after the warm-up: retry briefly.
        let mut resp = get()?;
        for _ in 0..3 {
            if resp.status() != reqwest::StatusCode::FORBIDDEN {
                break;
            }
            std::thread::sleep(Duration::from_millis(800));
            resp = get()?;
        }
        let bytes = resp.error_for_status()?.bytes()?;
        out.write_all(&bytes)?;
        let got = bytes.len() as u64;
        start += got;
        if got == 0 || (link.size.is_none() && got < CHUNK) {
            break;
        }
    }
    drop(out);
    if start == 0 {
        let _ = std::fs::remove_file(&tmp);
        anyhow::bail!("empty audio");
    }
    std::fs::rename(&tmp, &file)?;
    Ok(file)
}

/// Fast path (looked-up link + direct fetch), falling back to letting yt-dlp download it.
fn fetch_youtube(spec: &str, stem: &std::path::Path) -> anyhow::Result<std::path::PathBuf> {
    if EXTRACTOR.get().is_some() {
        match link_for(spec).and_then(|l| fetch_link(&l, stem)) {
            Ok(p) => return Ok(p),
            Err(e) => {
                LINKS.lock().remove(spec); // stale or refused link: don't reuse it
                log::warn!("direct fetch failed for {spec} ({e:#}); using yt-dlp");
            }
        }
    }
    match RESOLVER.get() {
        Some(resolve) => resolve(spec, stem),
        None => Err(anyhow::anyhow!("YouTube playback isn't available on this device")),
    }
}

pub fn has_stream_resolver() -> bool {
    RESOLVER.get().is_some() || EXTRACTOR.get().is_some()
}

fn is_remote(path: &str) -> bool {
    path.starts_with("http://") || path.starts_with("https://") || path.starts_with("yt:") || path.starts_with("ytq:")
}

fn short_hash(s: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    format!("{:016x}", h.finish())
}

/// Turns any queue path into a local file FFmpeg can open (fetching remote sources once).
fn local_source(path: &str, dir: &std::path::Path) -> anyhow::Result<String> {
    if !path.starts_with("yt:") && !path.starts_with("ytq:") {
        return if is_remote(path) { fetch_remote(path, dir) } else { Ok(path.to_string()) };
    }
    let (spec, fallback) = match path.split_once('#') {
        Some((s, f)) if !f.is_empty() => (s, Some(f)),
        _ => (path, None),
    };
    let lock = FETCH_LOCKS.lock().entry(spec.to_string()).or_default().clone();
    let _guard = lock.lock();
    std::fs::create_dir_all(dir)?;
    let stem = format!("yt-{}", short_hash(spec));
    if let Some(hit) = std::fs::read_dir(dir)?.filter_map(|e| e.ok()).map(|e| e.path()).find(|p| {
        p.file_stem().is_some_and(|s| s.to_string_lossy() == stem) && p.extension().is_some_and(|e| e != "part")
    }) {
        // Keep recently played songs at the front of the cache.
        let _ = std::fs::File::options().append(true).open(&hit).and_then(|f| f.set_modified(std::time::SystemTime::now()));
        return Ok(hit.to_string_lossy().to_string());
    }
    let result = fetch_youtube(spec, &dir.join(&stem));
    match (result, fallback) {
        (Ok(file), _) => {
            prune_yt_cache(dir);
            Ok(file.to_string_lossy().to_string())
        }
        (Err(e), Some(preview)) => {
            log::warn!("YouTube unavailable for {spec} ({e:#}); playing the preview");
            fetch_remote(preview, dir)
        }
        (Err(e), None) => Err(e),
    }
}

fn prune_yt_cache(dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut files: Vec<(std::time::SystemTime, std::path::PathBuf)> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("yt-"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    files.sort_by_key(|f| std::cmp::Reverse(f.0));
    for (_, p) in files.into_iter().skip(YT_CACHE_FILES) {
        let _ = std::fs::remove_file(p);
    }
}

/// Starts fetching the track after `pos` in the background, so remote songs switch gaplessly.
fn warm_after(g: &Inner, pos: usize, dir: &std::path::Path) {
    let Some(next) = g.next_pos(pos, false) else { return };
    let Some(item) = g.item_at(next) else { return };
    if next == pos || !is_remote(&item.path) {
        return;
    }
    let (path, dir) = (item.path.clone(), dir.to_path_buf());
    std::thread::spawn(move || {
        if let Err(e) = local_source(&path, &dir) {
            log::warn!("couldn't prefetch {path}: {e:#}");
        }
    });
}

/// Downloads a remote preview once into the cache (previews are ~1 MB).
fn fetch_remote(url: &str, dir: &std::path::Path) -> anyhow::Result<String> {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    url.split('?').next().unwrap_or(url).hash(&mut h);
    let ext = url.split('?').next().and_then(|u| u.rsplit('.').next()).filter(|e| e.len() <= 4).unwrap_or("bin");
    let file = dir.join(format!("{:016x}.{ext}", h.finish()));
    if !file.exists() {
        std::fs::create_dir_all(dir)?;
        let bytes = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()?
            .get(url)
            .send()?
            .error_for_status()?
            .bytes()?;
        let tmp = file.with_extension("part");
        std::fs::write(&tmp, &bytes)?;
        std::fs::rename(&tmp, &file)?;
    }
    Ok(file.to_string_lossy().to_string())
}
