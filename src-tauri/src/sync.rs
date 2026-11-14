//! Wi-Fi sync between Catlify devices: no account and no cloud.
//!
//! Each device that turns sync on runs a small HTTP server on the local network and announces
//! itself over mDNS (`_catlify._tcp`). Devices pair once with a 6-digit code shown on one and typed
//! on the other; both derive a shared secret from it (the secret itself never crosses the network)
//! and sign every later request with it.
//!
//! "Sync now" is two-way: this device copies the songs it's missing from the other one, then asks
//! the other device to do the same from us. Songs are matched by a content fingerprint, so the same
//! file in different folders isn't copied twice. Likes, play counts and playlists are merged
//! (likes: union, counts: highest, playlists: by name, songs added). Nothing is ever deleted.
//!
//! Endpoints: GET /hello · POST /pair/start · POST /pair/finish (unsigned, pairing only)
//!            GET /manifest · GET /file/<fp> · POST /sync/request (signed)

use crate::library::Library;
use anyhow::{anyhow, bail, Context, Result};
use hmac::{Hmac, KeyInit, Mac};
use parking_lot::Mutex;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom};
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const SERVICE: &str = "_catlify._tcp.local.";
const PROTOCOL: u32 = 1;
const PAIR_TTL: Duration = Duration::from_secs(180);
const MAX_SKEW: i64 = 300;
/// Marks a song still being copied; the library scanner skips these.
pub const PARTIAL: &str = ".catlify-partial.";

type HmacSha256 = Hmac<Sha256>;

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn random_hex(n: usize) -> String {
    use rand::RngCore;
    let mut b = vec![0u8; n];
    rand::thread_rng().fill_bytes(&mut b);
    hex(&b)
}

fn hmac_hex(key: &[u8], msg: &str) -> String {
    let mut m = <HmacSha256 as KeyInit>::new_from_slice(key).expect("HMAC takes any key length");
    m.update(msg.as_bytes());
    hex(&m.finalize().into_bytes())
}

/// Constant-time comparison for signatures and proofs.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Content fingerprint: size + SHA-256 of the first and last 64 KB. Identical files match on every device.
pub fn fingerprint(path: &Path) -> Option<String> {
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let mut h = Sha256::new();
    h.update(len.to_le_bytes());
    let mut buf = vec![0u8; 64 * 1024];
    let n = read_full(&mut f, &mut buf)?;
    h.update(&buf[..n]);
    if len > 128 * 1024 {
        f.seek(SeekFrom::End(-(64 * 1024))).ok()?;
        let n = read_full(&mut f, &mut buf)?;
        h.update(&buf[..n]);
    }
    Some(hex(&h.finalize()[..16]))
}

fn read_full(f: &mut std::fs::File, buf: &mut [u8]) -> Option<usize> {
    let mut n = 0;
    while n < buf.len() {
        match f.read(&mut buf[n..]).ok()? {
            0 => break,
            k => n += k,
        }
    }
    Some(n)
}

/// Fills in fingerprints for songs scanned before sync existed (or changed since).
pub fn ensure_fingerprints(lib: &Library) -> Result<()> {
    let todo: Vec<(i64, String)> = lib.with_db(|db| -> Result<_> {
        let mut st = db.prepare("SELECT id, path FROM tracks WHERE fp IS NULL AND missing_since IS NULL")?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    })?;
    for (id, path) in todo {
        if let Some(fp) = fingerprint(Path::new(&path)) {
            lib.with_db(|db| -> rusqlite::Result<()> {
                db.execute("UPDATE tracks SET fp = ?2 WHERE id = ?1", params![id, fp])?;
                // The user has this song again: let sync treat it normally.
                db.execute("DELETE FROM deleted WHERE fp = ?1", [&fp])?;
                Ok(())
            })?;
        }
    }
    Ok(())
}

// ---- state ------------------------------------------------------------------------------

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Nearby {
    id: String,
    name: String,
    addr: String,
    #[serde(skip)]
    seen: Option<Instant>,
}

/// On the device showing the code: a pairing request waiting for the other side to enter it.
struct Incoming {
    peer_id: String,
    peer_name: String,
    peer_addr: String,
    nonce_a: String,
    nonce_b: String,
    code: String,
    expires: Instant,
    attempts: u32,
}

/// On the device typing the code.
struct Outgoing {
    peer_id: String,
    peer_name: String,
    addr: String,
    nonce_a: String,
    nonce_b: String,
}

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub active: bool,
    pub peer: Option<String>,
    /// connecting | comparing | copying | merging | done | error
    pub phase: String,
    pub done: usize,
    pub total: usize,
    pub bytes: u64,
    pub current: Option<String>,
    pub added: usize,
    /// Songs that couldn't be copied this run (reason of the last one in `error`).
    pub failed: usize,
    pub error: Option<String>,
    pub finished_at: Option<i64>,
}

pub struct Sync {
    lib: Arc<Library>,
    port: u16,
    enabled: AtomicBool,
    mdns: Mutex<Option<mdns_sd::ServiceDaemon>>,
    nearby: Mutex<HashMap<String, Nearby>>,
    incoming: Mutex<Option<Incoming>>,
    outgoing: Mutex<Option<Outgoing>>,
    progress: Mutex<Progress>,
}

static SYNC: OnceLock<Arc<Sync>> = OnceLock::new();

pub fn get() -> Option<&'static Arc<Sync>> {
    SYNC.get()
}

fn device_id(lib: &Library) -> String {
    lib.setting("device_id").unwrap_or_else(|| {
        let id = random_hex(8);
        let _ = lib.set_setting("device_id", &id);
        id
    })
}

pub fn device_name(lib: &Library) -> String {
    lib.setting("device_name").filter(|n| !n.trim().is_empty()).unwrap_or_else(default_name)
}

fn default_name() -> String {
    #[cfg(target_os = "macos")]
    if let Ok(o) = std::process::Command::new("scutil").args(["--get", "ComputerName"]).output() {
        let n = String::from_utf8_lossy(&o.stdout).trim().to_string();
        if !n.is_empty() {
            return n;
        }
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    if let Ok(o) = std::process::Command::new("hostname").output() {
        let n = String::from_utf8_lossy(&o.stdout).trim().trim_end_matches(".local").to_string();
        if !n.is_empty() {
            return n;
        }
    }
    #[cfg(target_os = "android")]
    {
        let prop = |k: &str| {
            std::process::Command::new("getprop").arg(k).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        };
        if let Some(model) = prop("ro.product.model").filter(|m| !m.is_empty()) {
            let maker = prop("ro.product.manufacturer").unwrap_or_default();
            let maker = maker[..maker.len().min(1)].to_uppercase() + maker.get(1..).unwrap_or("");
            return if maker.is_empty() || model.to_lowercase().starts_with(&maker.to_lowercase()) { model } else { format!("{maker} {model}") };
        }
    }
    if cfg!(target_os = "android") {
        "Android phone".into()
    } else if cfg!(target_os = "ios") {
        "iPhone".into()
    } else {
        "Catlify".into()
    }
}

/// Where songs copied from other devices go (added to the library automatically).
pub fn sync_dir(lib: &Library) -> PathBuf {
    if let Some(d) = lib.setting("sync_dir") {
        return PathBuf::from(d);
    }
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_default();
    if cfg!(target_os = "android") {
        PathBuf::from("/storage/emulated/0/Music/Catlify/Synced")
    } else if cfg!(target_os = "ios") {
        home.join("Documents/Synced") // Files app → On My iPhone → Catlify → Synced
    } else {
        home.join("Music").join("Catlify").join("Synced")
    }
}

fn ensure_in_library(lib: &Library, dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("can't create {}", dir.display()))?;
    let d = dir.to_string_lossy().to_string();
    let covered = lib.folders()?.iter().any(|f| d == *f || d.starts_with(&format!("{}/", f.trim_end_matches('/'))));
    if !covered {
        lib.add_folder(&d)?;
    }
    Ok(())
}

/// Starts sync at launch if the user turned it on earlier.
pub fn start_if_enabled(lib: Arc<Library>) {
    if lib.setting("sync_enabled").as_deref() == Some("true") {
        if let Err(e) = enable(lib, true) {
            log::warn!("sync: couldn't start: {e:#}");
        }
    }
}

/// Turns this device's visibility on/off. The server starts once and stays up for the session.
pub fn enable(lib: Arc<Library>, on: bool) -> Result<()> {
    lib.set_setting("sync_enabled", if on { "true" } else { "false" })?;
    if !on {
        if let Some(s) = get() {
            s.enabled.store(false, Ordering::SeqCst);
            s.stop_mdns();
        }
        return Ok(());
    }
    let s = match get() {
        Some(s) => s.clone(),
        None => {
            let s = Sync::start(lib)?;
            let _ = SYNC.set(s.clone());
            s
        }
    };
    s.enabled.store(true, Ordering::SeqCst);
    s.start_mdns();
    Ok(())
}

impl Sync {
    /// Opens the sync server on a free port (not announced on mDNS until `start_mdns`).
    pub fn start(lib: Arc<Library>) -> Result<Arc<Self>> {
        // Reuse last session's port so paired devices can still find us without mDNS.
        let preferred = lib.setting("sync_port").and_then(|p| p.parse::<u16>().ok()).unwrap_or(0);
        let server = tiny_http::Server::http(("0.0.0.0", preferred))
            .or_else(|_| tiny_http::Server::http("0.0.0.0:0"))
            .map_err(|e| anyhow!("can't open the sync port: {e}"))?;
        let port = server.server_addr().to_ip().map(|a| a.port()).ok_or_else(|| anyhow!("no sync port"))?;
        let _ = lib.set_setting("sync_port", &port.to_string());
        let s = Arc::new(Sync {
            lib,
            port,
            enabled: AtomicBool::new(true),
            mdns: Mutex::new(None),
            nearby: Mutex::default(),
            incoming: Mutex::default(),
            outgoing: Mutex::default(),
            progress: Mutex::default(),
        });
        let srv = s.clone();
        std::thread::Builder::new().name("catlify-sync".into()).spawn(move || {
            for req in server.incoming_requests() {
                let srv = srv.clone();
                std::thread::spawn(move || srv.handle(req));
            }
        })?;
        log::info!("sync: listening on port {port}");
        Ok(s)
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    fn me(&self) -> (String, String) {
        (device_id(&self.lib), device_name(&self.lib))
    }

    // ---- discovery --------------------------------------------------------------------

    fn start_mdns(self: &Arc<Self>) {
        let mut slot = self.mdns.lock();
        if slot.is_some() {
            return;
        }
        let daemon = match mdns_sd::ServiceDaemon::new() {
            Ok(d) => d,
            Err(e) => return log::warn!("sync: mDNS unavailable ({e}); add devices by address instead"),
        };
        let (id, name) = self.me();
        let props = [("id", id.as_str()), ("name", name.as_str()), ("v", "1")];
        match mdns_sd::ServiceInfo::new(SERVICE, &id, &format!("catlify-{id}.local."), "", self.port, &props[..]) {
            Ok(info) => {
                if let Err(e) = daemon.register(info.enable_addr_auto()) {
                    log::warn!("sync: can't announce this device: {e}");
                }
            }
            Err(e) => log::warn!("sync: bad mDNS record: {e}"),
        }
        if let Ok(rx) = daemon.browse(SERVICE) {
            let me = self.clone();
            std::thread::spawn(move || {
                while let Ok(ev) = rx.recv() {
                    match ev {
                        mdns_sd::ServiceEvent::ServiceResolved(r) => {
                            let Some(pid) = r.get_property_val_str("id").map(str::to_string) else { continue };
                            if pid == device_id(&me.lib) {
                                continue;
                            }
                            let Some(ip) = r.get_addresses_v4().into_iter().next() else { continue };
                            let name = r.get_property_val_str("name").unwrap_or("Catlify").to_string();
                            let addr = format!("{ip}:{}", r.port);
                            me.nearby.lock().insert(pid.clone(), Nearby { id: pid, name, addr, seen: Some(Instant::now()) });
                        }
                        mdns_sd::ServiceEvent::ServiceRemoved(_, fullname) => {
                            me.nearby.lock().retain(|id, _| !fullname.starts_with(id.as_str()));
                        }
                        mdns_sd::ServiceEvent::SearchStopped(_) => break,
                        _ => {}
                    }
                }
            });
        }
        *slot = Some(daemon);
    }

    fn stop_mdns(&self) {
        if let Some(d) = self.mdns.lock().take() {
            let _ = d.shutdown();
        }
        self.nearby.lock().clear();
    }

    // ---- server -------------------------------------------------------------------------

    fn handle(self: &Arc<Self>, mut req: tiny_http::Request) {
        let remote = req.remote_addr().map(|a| a.ip());
        let method = req.method().as_str().to_string();
        let path = req.url().split('?').next().unwrap_or("").to_string();
        let mut body = String::new();
        if method == "POST" {
            let _ = req.as_reader().take(64 * 1024).read_to_string(&mut body);
        }
        let header = |name: &str| {
            req.headers().iter().find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name)).map(|h| h.value.as_str().to_string())
        };
        let auth = (header("x-nc-id"), header("x-nc-ts"), header("x-nc-sig"), header("x-nc-port"));

        let result: Result<Reply> = if !self.enabled.load(Ordering::SeqCst) {
            Err(anyhow!("sync is turned off on this device"))
        } else {
            match (method.as_str(), path.as_str()) {
                ("GET", "/hello") => {
                    let (id, name) = self.me();
                    Ok(Reply::Json(json!({ "app": "catlify", "protocol": PROTOCOL, "id": id, "name": name })))
                }
                ("POST", "/pair/start") => self.pair_start(&body, remote),
                ("POST", "/pair/finish") => self.pair_finish(&body),
                _ => match self.verify(&method, &path, auth, remote) {
                    Err(e) => Err(e),
                    Ok(peer) => match (method.as_str(), path.as_str()) {
                        ("GET", "/manifest") => self.manifest().map(Reply::Json),
                        ("GET", p) if p.starts_with("/file/") => self.file(&p["/file/".len()..]),
                        ("POST", "/sync/request") => {
                            // The other device just copied from us; now copy from it.
                            self.start_pull(peer);
                            Ok(Reply::Json(json!({ "ok": true })))
                        }
                        _ => Err(anyhow!("not found")),
                    },
                },
            }
        };
        let response = match result {
            Ok(Reply::Json(v)) => tiny_http::Response::from_string(v.to_string()).with_header(json_header()).boxed(),
            Ok(Reply::File(f, len)) => tiny_http::Response::new(tiny_http::StatusCode(200), vec![], f, Some(len as usize), None).boxed(),
            Err(e) => tiny_http::Response::from_string(json!({ "error": format!("{e:#}") }).to_string())
                .with_status_code(403)
                .with_header(json_header())
                .boxed(),
        };
        let _ = req.respond(response);
    }

    /// Checks the request signature; returns the peer id. Also remembers where the peer can be reached.
    fn verify(
        &self,
        method: &str,
        path: &str,
        (id, ts, sig, port): (Option<String>, Option<String>, Option<String>, Option<String>),
        remote: Option<IpAddr>,
    ) -> Result<String> {
        let (Some(id), Some(ts), Some(sig)) = (id, ts, sig) else { bail!("not paired") };
        let ts_n: i64 = ts.parse().context("bad timestamp")?;
        if (now() - ts_n).abs() > MAX_SKEW {
            bail!("clock difference too large; check both devices' time");
        }
        let secret = peer_secret(&self.lib, &id)?.ok_or_else(|| anyhow!("not paired with this device"))?;
        if !same(&hmac_hex(secret.as_bytes(), &format!("{id}|{ts}|{method}|{path}")), &sig) {
            bail!("bad signature");
        }
        // (Loopback means a tunnel or the same machine: not an address another device can use.)
        if let (Some(ip), Some(port)) = (remote.filter(|ip| !ip.is_loopback()), port.and_then(|p| p.parse::<u16>().ok())) {
            let addr = SocketAddr::new(ip, port).to_string();
            self.lib.with_db(|db| db.execute("UPDATE peers SET addr = ?2 WHERE id = ?1", params![id, addr]))?;
        }
        Ok(id)
    }

    fn pair_start(&self, body: &str, remote: Option<IpAddr>) -> Result<Reply> {
        #[derive(Deserialize)]
        struct Start {
            id: String,
            name: String,
            nonce: String,
            port: u16,
        }
        let s: Start = serde_json::from_str(body).context("bad pairing request")?;
        let code = format!("{:06}", rand::random::<u32>() % 1_000_000);
        let nonce_a = random_hex(16);
        let ip = remote.ok_or_else(|| anyhow!("unknown address"))?;
        *self.incoming.lock() = Some(Incoming {
            peer_id: s.id,
            peer_name: s.name.chars().take(60).collect(),
            peer_addr: SocketAddr::new(ip, s.port).to_string(),
            nonce_a: nonce_a.clone(),
            nonce_b: s.nonce,
            code,
            expires: Instant::now() + PAIR_TTL,
            attempts: 0,
        });
        let (id, name) = self.me();
        Ok(Reply::Json(json!({ "id": id, "name": name, "nonce": nonce_a })))
    }

    fn pair_finish(&self, body: &str) -> Result<Reply> {
        #[derive(Deserialize)]
        struct Finish {
            id: String,
            proof: String,
        }
        let f: Finish = serde_json::from_str(body).context("bad pairing request")?;
        let mut slot = self.incoming.lock();
        let Some(inc) = slot.as_mut().filter(|i| i.peer_id == f.id && i.expires > Instant::now()) else {
            bail!("no pairing in progress; start again")
        };
        inc.attempts += 1;
        let expected = hmac_hex(inc.code.as_bytes(), &format!("proof|{}|{}", inc.nonce_a, inc.nonce_b));
        if !same(&expected, &f.proof) {
            if inc.attempts >= 5 {
                *slot = None;
            }
            bail!("wrong code");
        }
        let secret = hmac_hex(inc.code.as_bytes(), &format!("secret|{}|{}", inc.nonce_a, inc.nonce_b));
        save_peer(&self.lib, &inc.peer_id, &inc.peer_name, &secret, Some(&inc.peer_addr))?;
        *slot = None;
        Ok(Reply::Json(json!({ "ok": true })))
    }

    fn manifest(&self) -> Result<Value> {
        ensure_fingerprints(&self.lib)?;
        self.lib.with_db(|db| -> Result<Value> {
            let mut st = db.prepare(
                "SELECT fp, path, title, artist, album, album_artist, track_no, disc_no, liked, play_count, last_played
                 FROM tracks WHERE fp IS NOT NULL AND missing_since IS NULL",
            )?;
            let tracks: Vec<Value> = st
                .query_map([], |r| {
                    let path: String = r.get(1)?;
                    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                    let ext = Path::new(&path).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
                    Ok(json!({
                        "fp": r.get::<_, String>(0)?, "size": size, "ext": ext,
                        "title": r.get::<_, String>(2)?, "artist": r.get::<_, String>(3)?, "album": r.get::<_, String>(4)?,
                        "albumArtist": r.get::<_, String>(5)?, "trackNo": r.get::<_, Option<u32>>(6)?, "discNo": r.get::<_, Option<u32>>(7)?,
                        "liked": r.get::<_, bool>(8)?, "playCount": r.get::<_, i64>(9)?, "lastPlayed": r.get::<_, Option<i64>>(10)?,
                    }))
                })?
                .collect::<rusqlite::Result<_>>()?;
            let mut st = db.prepare(
                "SELECT p.name, t.fp FROM playlists p JOIN playlist_tracks pt ON pt.playlist_id = p.id
                 JOIN tracks t ON t.id = pt.track_id WHERE t.fp IS NOT NULL AND t.missing_since IS NULL ORDER BY p.id, pt.position",
            )?;
            let mut playlists: Vec<(String, Vec<String>)> = vec![];
            for row in st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
                let (name, fp) = row?;
                match playlists.last_mut() {
                    Some((n, fps)) if *n == name => fps.push(fp),
                    _ => playlists.push((name, vec![fp])),
                }
            }
            let playlists: Vec<Value> = playlists.into_iter().map(|(name, fps)| json!({ "name": name, "fps": fps })).collect();
            Ok(json!({ "tracks": tracks, "playlists": playlists }))
        })
    }

    fn file(&self, fp: &str) -> Result<Reply> {
        let path: Option<String> = self
            .lib
            .with_db(|db| db.query_row("SELECT path FROM tracks WHERE fp = ?1 AND missing_since IS NULL", [fp], |r| r.get(0)).optional())?;
        let path = path.ok_or_else(|| anyhow!("no such song"))?;
        let f = std::fs::File::open(&path)?;
        let len = f.metadata()?.len();
        Ok(Reply::File(f, len))
    }

    // ---- status (polled by the Sync page) ------------------------------------------------

    pub fn status(&self) -> Result<Value> {
        let (id, name) = self.me();
        let paired = list_peers(&self.lib)?;
        let paired_ids: HashSet<String> = paired.iter().map(|p| p.id.clone()).collect();
        let mut nearby = self.nearby.lock();
        nearby.retain(|_, n| n.seen.is_some_and(|t| t.elapsed() < Duration::from_secs(600)));
        let near: Vec<Value> = nearby
            .values()
            .map(|n| json!({ "id": n.id, "name": n.name, "addr": n.addr, "paired": paired_ids.contains(&n.id) }))
            .collect();
        let peers: Vec<Value> = paired
            .iter()
            .map(|p| {
                let online = nearby.get(&p.id).map(|n| n.addr.clone());
                json!({ "id": p.id, "name": p.name, "lastSync": p.last_sync, "online": online.is_some(),
                        "addr": online.or(p.addr.clone()) })
            })
            .collect();
        let incoming = self.incoming.lock().as_ref().filter(|i| i.expires > Instant::now()).map(|i| json!({ "name": i.peer_name, "code": i.code }));
        let outgoing = self.outgoing.lock().as_ref().map(|o| json!({ "name": o.peer_name }));
        Ok(json!({
            "enabled": self.enabled.load(Ordering::SeqCst),
            "me": { "id": id, "name": name, "port": self.port, "addresses": local_ipv4s() },
            "nearby": near, "paired": peers, "incoming": incoming, "outgoing": outgoing,
            "progress": *self.progress.lock(), "syncDir": sync_dir(&self.lib).to_string_lossy(),
        }))
    }

    // ---- client: pairing --------------------------------------------------------------

    /// Asks the device at `addr` to show a pairing code.
    pub fn pair_begin(&self, addr: &str) -> Result<Value> {
        let addr = normalize_addr(addr)?;
        let hello: Value = http(Duration::from_secs(6))?.get(format!("http://{addr}/hello")).send()?.error_for_status()?.json()?;
        if hello.get("app").and_then(Value::as_str) != Some("catlify") {
            bail!("that address isn't a Catlify device");
        }
        let (my_id, my_name) = self.me();
        let nonce_b = random_hex(16);
        let resp: Value = http(Duration::from_secs(6))?
            .post(format!("http://{addr}/pair/start"))
            .json(&json!({ "id": my_id, "name": my_name, "nonce": nonce_b, "port": self.port }))
            .send()?
            .json()?;
        if let Some(e) = resp.get("error").and_then(Value::as_str) {
            bail!("{e}");
        }
        let peer_id = resp["id"].as_str().ok_or_else(|| anyhow!("bad reply"))?.to_string();
        let peer_name = resp["name"].as_str().unwrap_or("Catlify").to_string();
        let nonce_a = resp["nonce"].as_str().ok_or_else(|| anyhow!("bad reply"))?.to_string();
        *self.outgoing.lock() = Some(Outgoing { peer_id, peer_name: peer_name.clone(), addr, nonce_a, nonce_b });
        Ok(json!({ "name": peer_name }))
    }

    /// Completes pairing with the code shown on the other device.
    pub fn pair_complete(&self, code: &str) -> Result<Value> {
        let code: String = code.chars().filter(|c| c.is_ascii_digit()).collect();
        let o = self.outgoing.lock().take().ok_or_else(|| anyhow!("no pairing in progress"))?;
        let proof = hmac_hex(code.as_bytes(), &format!("proof|{}|{}", o.nonce_a, o.nonce_b));
        let (my_id, _) = self.me();
        let resp: Value = http(Duration::from_secs(6))?
            .post(format!("http://{}/pair/finish", o.addr))
            .json(&json!({ "id": my_id, "proof": proof }))
            .send()?
            .json()?;
        if let Some(e) = resp.get("error").and_then(Value::as_str) {
            // Let the user try the code again.
            let wrong = e.contains("wrong code");
            if wrong {
                *self.outgoing.lock() = Some(o);
            }
            bail!("{e}");
        }
        let secret = hmac_hex(code.as_bytes(), &format!("secret|{}|{}", o.nonce_a, o.nonce_b));
        save_peer(&self.lib, &o.peer_id, &o.peer_name, &secret, Some(&o.addr))?;
        Ok(json!({ "id": o.peer_id, "name": o.peer_name }))
    }

    pub fn pair_cancel(&self) {
        *self.outgoing.lock() = None;
        *self.incoming.lock() = None;
    }

    pub fn forget(&self, id: &str) -> Result<()> {
        self.lib.with_db(|db| db.execute("DELETE FROM peers WHERE id = ?1", [id]))?;
        Ok(())
    }

    // ---- client: syncing ----------------------------------------------------------------

    fn set_progress(&self, f: impl FnOnce(&mut Progress)) {
        f(&mut self.progress.lock());
    }

    /// Copies what we're missing from `peer_id`, merges library data, then (if `reverse`) asks the
    /// peer to copy what it's missing from us.
    fn sync_with(&self, peer_id: &str, reverse: bool) -> Result<()> {
        let peer = find_peer(&self.lib, peer_id)?.ok_or_else(|| anyhow!("not paired with that device"))?;
        let addr = self.nearby.lock().get(peer_id).map(|n| n.addr.clone()).or(peer.addr.clone());
        let addr = addr.ok_or_else(|| anyhow!("{} isn't on this network right now", peer.name))?;
        self.set_progress(|p| *p = Progress { active: true, peer: Some(peer.name.clone()), phase: "connecting".into(), ..Default::default() });

        let client = Signed { http: http(Duration::from_secs(20))?, addr: addr.clone(), secret: peer.secret.clone(), me: self.me().0, port: self.port };
        let manifest: Manifest = client.get("/manifest")?.json().context("bad manifest")?;

        self.set_progress(|p| p.phase = "comparing".into());
        ensure_fingerprints(&self.lib)?;
        let have: HashSet<String> = self.lib.with_db(|db| -> Result<_> {
            let mut st = db.prepare("SELECT fp FROM tracks WHERE fp IS NOT NULL AND missing_since IS NULL")?;
            let rows = st.query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<HashSet<_>>>()?;
            Ok(rows)
        })?;
        // Songs deleted on this device stay deleted: don't copy them back.
        let deleted: HashSet<String> = self.lib.with_db(|db| -> Result<_> {
            let mut st = db.prepare("SELECT fp FROM deleted")?;
            let rows = st.query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<HashSet<_>>>()?;
            Ok(rows)
        })?;
        let missing: Vec<&RemoteTrack> = manifest.tracks.iter().filter(|t| !have.contains(&t.fp) && !deleted.contains(&t.fp)).collect();
        let dir = sync_dir(&self.lib);
        if !missing.is_empty() {
            ensure_in_library(&self.lib, &dir)?;
        }

        self.set_progress(|p| {
            p.phase = "copying".into();
            p.total = missing.len();
        });
        let mut added = 0;
        for t in &missing {
            self.set_progress(|p| p.current = Some(format!("{} — {}", t.artist, t.title)));
            match client.download(t, &dir, |n| self.set_progress(|p| p.bytes += n)) {
                Ok(used) => {
                    if used != dir {
                        ensure_in_library(&self.lib, &used)?;
                    }
                    added += 1
                }
                Err(e) => {
                    log::warn!("sync: couldn't copy {}: {e:#}", t.title);
                    self.set_progress(|p| {
                        p.failed += 1;
                        p.error = Some(format!("{}: {e:#}", t.title));
                    });
                }
            }
            self.set_progress(|p| p.done += 1);
        }

        self.set_progress(|p| {
            p.phase = "merging".into();
            p.current = None;
        });
        if added > 0 {
            self.lib.scan(|_| {})?;
            ensure_fingerprints(&self.lib)?;
        }
        merge(&self.lib, &manifest)?;
        self.lib.with_db(|db| db.execute("UPDATE peers SET last_sync = ?2, addr = ?3 WHERE id = ?1", params![peer_id, now(), addr]))?;

        if reverse {
            // Ask the other device to copy what it's missing from us.
            if let Err(e) = client.post("/sync/request") {
                log::warn!("sync: {} couldn't sync back: {e:#}", peer.name);
            }
        }
        self.set_progress(|p| {
            p.active = false;
            p.phase = "done".into();
            p.added = added;
            p.finished_at = Some(now());
        });
        Ok(())
    }
}

enum Reply {
    Json(Value),
    File(std::fs::File, u64),
}

fn json_header() -> tiny_http::Header {
    tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).expect("static header")
}

fn http(timeout: Duration) -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder().connect_timeout(Duration::from_secs(5)).timeout(timeout).build()?)
}

fn normalize_addr(addr: &str) -> Result<String> {
    let a = addr.trim().trim_start_matches("http://").trim_end_matches('/');
    let sa: SocketAddr = a.parse().context("use the address shown on the other device, like 192.168.1.20:52814")?;
    Ok(sa.to_string())
}

/// This device's LAN addresses, shown for manual pairing when mDNS is blocked.
fn local_ipv4s() -> Vec<String> {
    // Connecting a UDP socket picks the outgoing interface without sending anything.
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| s.connect("8.8.8.8:80").map(|_| s))
        .and_then(|s| s.local_addr())
        .map(|a| vec![a.ip().to_string()])
        .unwrap_or_default()
}

impl Sync {
    /// One-way pull from `peer_id` in the background (when a peer asks us to sync back).
    fn start_pull(self: &Arc<Self>, peer_id: String) {
        if self.progress.lock().active {
            return;
        }
        let s = self.clone();
        std::thread::spawn(move || s.run(&peer_id, false));
    }

    /// "Sync now": two-way, in the background. Poll `status()` for progress.
    pub fn sync_now(self: &Arc<Self>, peer_id: &str) -> Result<()> {
        if self.progress.lock().active {
            bail!("a sync is already running");
        }
        let (s, id) = (self.clone(), peer_id.to_string());
        std::thread::spawn(move || s.run(&id, true));
        Ok(())
    }

    /// Runs a sync and records failures in the progress state.
    pub fn run(&self, peer_id: &str, reverse: bool) {
        if let Err(e) = self.sync_with(peer_id, reverse) {
            log::warn!("sync failed: {e:#}");
            self.set_progress(|p| {
                p.active = false;
                p.phase = "error".into();
                p.error = Some(format!("{e:#}"));
            });
        }
    }

    pub fn progress(&self) -> Progress {
        self.progress.lock().clone()
    }

    /// The code shown to the user while another device is pairing with this one.
    pub fn pending_code(&self) -> Option<String> {
        self.incoming.lock().as_ref().map(|i| i.code.clone())
    }
}

// ---- signed client ------------------------------------------------------------------------

struct Signed {
    http: reqwest::blocking::Client,
    addr: String,
    secret: String,
    me: String,
    port: u16,
}

impl Signed {
    fn sign(&self, rb: reqwest::blocking::RequestBuilder, method: &str, path: &str) -> reqwest::blocking::RequestBuilder {
        let ts = now().to_string();
        let sig = hmac_hex(self.secret.as_bytes(), &format!("{}|{ts}|{method}|{path}", self.me));
        rb.header("x-nc-id", &self.me).header("x-nc-ts", ts).header("x-nc-sig", sig).header("x-nc-port", self.port.to_string())
    }

    fn get(&self, path: &str) -> Result<reqwest::blocking::Response> {
        let r = self.sign(self.http.get(format!("http://{}{path}", self.addr)), "GET", path).send()?;
        check(r)
    }

    fn post(&self, path: &str) -> Result<reqwest::blocking::Response> {
        let r = self.sign(self.http.post(format!("http://{}{path}", self.addr)).body("{}"), "POST", path).send()?;
        check(r)
    }

    /// Copies one song into `dir/<Artist>/<Album>/`, verifying its fingerprint. Returns the base
    /// folder used (Android keeps formats it won't allow in Music/ in Catlify's own folder).
    fn download(&self, t: &RemoteTrack, dir: &Path, on_bytes: impl Fn(u64)) -> Result<PathBuf> {
        let path = format!("/file/{}", t.fp);
        // Big lossless files can take a while: no overall timeout, only connect/idle limits.
        let client = reqwest::blocking::Client::builder().connect_timeout(Duration::from_secs(5)).timeout(None).build()?;
        let mut resp = check(self.sign(client.get(format!("http://{}{path}", self.addr)), "GET", &path).send()?)?;
        let stem = match t.track_no {
            Some(n) => format!("{n:02} - {}", t.title),
            None => t.title.clone(),
        };
        let ext = if t.ext.is_empty() { "bin".to_string() } else { t.ext.clone() };
        let place = |base: &Path| -> Result<(PathBuf, PathBuf, std::fs::File)> {
            let artist = if t.album_artist.is_empty() { t.artist.clone() } else { t.album_artist.clone() };
            let folder = base.join(safe(&artist)).join(safe(&t.album));
            std::fs::create_dir_all(&folder)?;
            let mut dest = folder.join(format!("{}.{ext}", safe(&stem)));
            let mut n = 2;
            while dest.exists() {
                dest = folder.join(format!("{} ({n}).{ext}", safe(&stem)));
                n += 1;
            }
            // Keep the audio extension (Android only allows audio files in Music/) and don't hide it:
            // Android indexes visible files as ours, so they can be deleted later without a prompt.
            let tmp = folder.join(format!("{}{PARTIAL}{ext}", safe(&stem)));
            let f = std::fs::File::create(&tmp)?;
            Ok((dest, tmp, f))
        };
        let mut base = dir.to_path_buf();
        let (dest, tmp, mut out) = match place(&base) {
            Ok(x) => x,
            // Android's Music folder rejects types it doesn't know (WavPack, TTA...).
            Err(e) if cfg!(target_os = "android") => {
                log::info!("sync: .{ext} not allowed in Music ({e:#}); using Catlify's own folder");
                base = PathBuf::from("/storage/emulated/0/Android/data/app.stafy.player/files/Synced");
                place(&base)?
            }
            Err(e) => return Err(e),
        };
        {
            let mut buf = vec![0u8; 256 * 1024];
            loop {
                let k = resp.read(&mut buf)?;
                if k == 0 {
                    break;
                }
                std::io::Write::write_all(&mut out, &buf[..k])?;
                on_bytes(k as u64);
            }
        }
        if fingerprint(&tmp).as_deref() != Some(t.fp.as_str()) {
            let _ = std::fs::remove_file(&tmp);
            bail!("file changed during copy");
        }
        std::fs::rename(&tmp, &dest)?;
        Ok(base)
    }
}

fn check(r: reqwest::blocking::Response) -> Result<reqwest::blocking::Response> {
    if r.status().is_success() {
        return Ok(r);
    }
    let v: Value = r.json().unwrap_or_default();
    Err(anyhow!("{}", v.get("error").and_then(Value::as_str).unwrap_or("the other device refused the request")))
}

fn safe(s: &str) -> String {
    let s: String = s.chars().map(|c| if "/\\:*?\"<>|".contains(c) || c.is_control() { '_' } else { c }).collect();
    let s = s.trim().trim_start_matches('.').chars().take(120).collect::<String>();
    if s.is_empty() {
        "Unknown".into()
    } else {
        s
    }
}

// ---- library data ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoteTrack {
    fp: String,
    #[serde(default)]
    ext: String,
    title: String,
    artist: String,
    album: String,
    #[serde(default)]
    album_artist: String,
    track_no: Option<u32>,
    liked: bool,
    play_count: i64,
    last_played: Option<i64>,
}

#[derive(Deserialize)]
struct RemotePlaylist {
    name: String,
    fps: Vec<String>,
}

#[derive(Deserialize)]
struct Manifest {
    tracks: Vec<RemoteTrack>,
    playlists: Vec<RemotePlaylist>,
}

/// Likes: union · play counts / last played: highest · playlists: matched by name, songs appended.
fn merge(lib: &Library, m: &Manifest) -> Result<()> {
    lib.with_db(|db| -> Result<()> {
        let tx = db.unchecked_transaction()?;
        for t in &m.tracks {
            tx.execute(
                "UPDATE tracks SET liked = MAX(liked, ?2), play_count = MAX(play_count, ?3),
                    last_played = MAX(COALESCE(last_played, 0), COALESCE(?4, 0)) WHERE fp = ?1",
                params![t.fp, t.liked, t.play_count, t.last_played],
            )?;
        }
        for pl in &m.playlists {
            let id: i64 = match tx.query_row("SELECT id FROM playlists WHERE name = ?1", [&pl.name], |r| r.get(0)).optional()? {
                Some(id) => id,
                None => {
                    tx.execute("INSERT INTO playlists (name, created_at) VALUES (?1, ?2)", params![pl.name, now()])?;
                    tx.last_insert_rowid()
                }
            };
            let mut present: HashSet<String> = {
                let mut st = tx.prepare(
                    "SELECT t.fp FROM playlist_tracks pt JOIN tracks t ON t.id = pt.track_id WHERE pt.playlist_id = ?1 AND t.fp IS NOT NULL",
                )?;
                let rows = st.query_map([id], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<HashSet<_>>>()?;
                rows
            };
            let mut pos: i64 =
                tx.query_row("SELECT COALESCE(MAX(position), -1) FROM playlist_tracks WHERE playlist_id = ?1", [id], |r| r.get(0))?;
            for fp in &pl.fps {
                if present.contains(fp) {
                    continue;
                }
                let tid: Option<i64> = tx.query_row("SELECT id FROM tracks WHERE fp = ?1", [fp], |r| r.get(0)).optional()?;
                if let Some(tid) = tid {
                    pos += 1;
                    tx.execute("INSERT INTO playlist_tracks (playlist_id, track_id, position) VALUES (?1, ?2, ?3)", params![id, tid, pos])?;
                    present.insert(fp.clone());
                }
            }
        }
        tx.commit()?;
        Ok(())
    })
}

// ---- peers table ----------------------------------------------------------------------------

struct Peer {
    id: String,
    name: String,
    secret: String,
    addr: Option<String>,
    last_sync: Option<i64>,
}

fn save_peer(lib: &Library, id: &str, name: &str, secret: &str, addr: Option<&str>) -> Result<()> {
    lib.with_db(|db| {
        db.execute(
            "INSERT INTO peers (id, name, secret, addr, paired_at) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, secret = excluded.secret, addr = excluded.addr",
            params![id, name, secret, addr, now()],
        )
    })?;
    Ok(())
}

fn peer_secret(lib: &Library, id: &str) -> Result<Option<String>> {
    Ok(lib.with_db(|db| db.query_row("SELECT secret FROM peers WHERE id = ?1", [id], |r| r.get(0)).optional())?)
}

fn find_peer(lib: &Library, id: &str) -> Result<Option<Peer>> {
    Ok(list_peers(lib)?.into_iter().find(|p| p.id == id))
}

fn list_peers(lib: &Library) -> Result<Vec<Peer>> {
    lib.with_db(|db| -> Result<Vec<Peer>> {
        let mut st = db.prepare("SELECT id, name, secret, addr, last_sync FROM peers ORDER BY name COLLATE NOCASE")?;
        let rows = st
            .query_map([], |r| Ok(Peer { id: r.get(0)?, name: r.get(1)?, secret: r.get(2)?, addr: r.get(3)?, last_sync: r.get(4)? }))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    })
}
