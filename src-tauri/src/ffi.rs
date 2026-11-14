//! C ABI for the iOS host app (CarPlay scene, Now Playing, remote commands).
//! Android uses the JNI bindings in `jni_bridge.rs`; both go through `rpc`.

use std::ffi::{c_char, CStr, CString};

fn str_arg<'a>(p: *const c_char) -> &'a str {
    if p.is_null() {
        return "";
    }
    unsafe { CStr::from_ptr(p) }.to_str().unwrap_or("")
}

/// Starts the engine (idempotent). Paths must match the Tauri app's data/cache dirs.
#[no_mangle]
pub extern "C" fn stafy_init(data_dir: *const c_char, cache_dir: *const c_char) -> bool {
    crate::core::init(str_arg(data_dir).as_ref(), str_arg(cache_dir).as_ref()).is_ok()
}

/// Returns a JSON string the caller must release with `stafy_free`.
#[no_mangle]
pub extern "C" fn stafy_call(method: *const c_char, args_json: *const c_char) -> *mut c_char {
    let out = crate::rpc::call_json(str_arg(method), str_arg(args_json));
    CString::new(out).unwrap_or_default().into_raw()
}

#[no_mangle]
pub extern "C" fn stafy_free(s: *mut c_char) {
    if !s.is_null() {
        unsafe { drop(CString::from_raw(s)) };
    }
}

pub type StateCallback = extern "C" fn(state_json: *const c_char, track_changed: bool);

/// Registers a callback invoked (on a background thread) whenever playback state changes.
#[no_mangle]
pub extern "C" fn stafy_set_listener(cb: StateCallback) -> bool {
    let Some(core) = crate::core::get() else { return false };
    core.observe(move |_, changed, json| {
        if let Ok(c) = CString::new(json) {
            cb(c.as_ptr(), changed);
        }
    });
    true
}

/// iOS: registers the in-process yt-dlp (ytdlp-apple Swift package, media/YTDLPBridge.swift).
/// `run(request_json, line, context)` returns `{"code","stdout","stderr"}` as a string `free` releases.
#[cfg(target_os = "ios")]
#[no_mangle]
pub extern "C" fn stafy_set_ytdlp(run: crate::ytrun::RunFn, free: crate::ytrun::FreeFn) {
    crate::ytrun::register(run, free);
}
