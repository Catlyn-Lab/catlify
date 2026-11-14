//! macOS notch player: a borderless window that sits over the camera notch and grows into a
//! mini player on hover (like Dynamic Island). It ignores the mouse except over the visible
//! shape, so the menu bar around it keeps working. On Macs without a notch it draws a
//! notch-sized pill in the middle of the menu bar instead.

use crate::core;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSEvent, NSScreen, NSWindow, NSWindowCollectionBehavior};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use parking_lot::Mutex;
use serde::Serialize;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

const LABEL: &str = "notch";
const SETTING: &str = "notch";
/// Fixed canvas; the island animates inside it so the window never resizes (no flicker).
const WIN_W: f64 = 720.0;
const WIN_H: f64 = 280.0;
/// Above the menu bar (24) and status items (25), below menus and popups (101).
const LEVEL: isize = 26;

#[derive(Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Geometry {
    notch_width: f64,
    notch_height: f64,
    has_notch: bool,
    /// Screen centre x and top edge, in global AppKit coordinates.
    #[serde(skip)]
    mid_x: f64,
    #[serde(skip)]
    top: f64,
}

struct State {
    enabled: bool,
    geometry: Option<Geometry>,
    /// Size of the visible island (top-centred), reported by the web view.
    shape: (f64, f64),
    hovered: bool,
}

static STATE: Mutex<State> = Mutex::new(State { enabled: false, geometry: None, shape: (0.0, 0.0), hovered: false });

fn ns_window(win: &WebviewWindow) -> Option<&NSWindow> {
    win.ns_window().ok().map(|p| unsafe { &*(p as *const NSWindow) })
}

/// Picks the built-in notched display if there is one, otherwise the main display.
fn measure(mtm: MainThreadMarker) -> Option<Geometry> {
    let screens = NSScreen::screens(mtm);
    let notched = screens.iter().find(|s| s.safeAreaInsets().top > 0.0);
    let has_notch = notched.is_some();
    let screen = notched.or_else(|| NSScreen::mainScreen(mtm))?;
    let frame = screen.frame();
    let top = frame.origin.y + frame.size.height;
    let (notch_width, notch_height) = if has_notch {
        let left = screen.auxiliaryTopLeftArea();
        let right = screen.auxiliaryTopRightArea();
        (frame.size.width - left.size.width - right.size.width, screen.safeAreaInsets().top)
    } else {
        let visible = screen.visibleFrame();
        let menu_bar = top - (visible.origin.y + visible.size.height);
        (190.0, if menu_bar >= 20.0 { menu_bar } else { 24.0 })
    };
    Some(Geometry { notch_width, notch_height, has_notch, mid_x: frame.origin.x + frame.size.width / 2.0, top })
}

fn place(win: &WebviewWindow, g: &Geometry) {
    if let Some(w) = ns_window(win) {
        let frame = NSRect::new(NSPoint::new(g.mid_x - WIN_W / 2.0, g.top - WIN_H), NSSize::new(WIN_W, WIN_H));
        w.setFrame_display(frame, true);
    }
}

pub fn init(app: &AppHandle) -> tauri::Result<()> {
    let enabled = core::get().and_then(|c| c.library.setting(SETTING)).as_deref() != Some("false");
    let win = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("notch.html".into()))
        .title("Catlify Notch")
        .inner_size(WIN_W, WIN_H)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible_on_all_workspaces(true)
        .focusable(false)
        .focused(false)
        .accept_first_mouse(true) // buttons respond on the first click, without activating first
        .visible(false)
        .build()?;

    if let Some(w) = ns_window(&win) {
        w.setLevel(LEVEL);
        w.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
        w.setHasShadow(false);
        w.setIgnoresMouseEvents(true);
    }
    set_enabled(app, enabled);

    // Hover tracking: the window can't take mouse-moved events while it ignores the mouse,
    // so poll the cursor (cheap: one main-thread hop every ~30 ms while enabled).
    let app = app.clone();
    std::thread::spawn(move || {
        let mut tick = 0u32;
        loop {
            let enabled = STATE.lock().enabled;
            std::thread::sleep(Duration::from_millis(if enabled { 30 } else { 500 }));
            if !enabled {
                continue;
            }
            tick = tick.wrapping_add(1);
            let remeasure = tick % 33 == 0; // displays plugged in/out, resolution changes
            let handle = app.clone();
            let _ = app.run_on_main_thread(move || poll(&handle, remeasure));
        }
    });
    Ok(())
}

fn poll(app: &AppHandle, remeasure: bool) {
    let (Some(win), Some(mtm)) = (app.get_webview_window(LABEL), MainThreadMarker::new()) else { return };
    let mut st = STATE.lock();
    if !st.enabled {
        return;
    }
    if remeasure || st.geometry.is_none() {
        let g = measure(mtm);
        if g != st.geometry {
            st.geometry = g;
            if let Some(g) = &g {
                place(&win, g);
                let _ = win.emit("notch://geometry", g);
            }
        }
    }
    let Some(g) = st.geometry else { return };
    let p = NSEvent::mouseLocation();
    let (w, h) = st.shape;
    // A little slack once open, so brushing the edge doesn't collapse it.
    let pad = if st.hovered { 12.0 } else { 0.0 };
    let inside = w > 0.0 && (p.x - g.mid_x).abs() <= w / 2.0 + pad && p.y >= g.top - h - pad && p.y <= g.top + 1.0;
    if inside != st.hovered {
        st.hovered = inside;
        if let Some(nw) = ns_window(&win) {
            nw.setIgnoresMouseEvents(!inside);
        }
        let _ = win.emit("notch://hover", inside);
    }
}

pub fn set_enabled(app: &AppHandle, enabled: bool) {
    let Some(win) = app.get_webview_window(LABEL) else { return };
    let mut st = STATE.lock();
    st.enabled = enabled;
    st.hovered = false;
    if enabled {
        if let Some(g) = MainThreadMarker::new().and_then(measure) {
            st.geometry = Some(g);
            place(&win, &g);
            let _ = win.emit("notch://geometry", g);
        }
        let _ = win.show();
    } else {
        let _ = win.hide();
    }
}

// ---- commands -------------------------------------------------------------------------

#[tauri::command]
pub fn notch_geometry() -> Option<Geometry> {
    STATE.lock().geometry
}

#[tauri::command]
pub fn notch_shape(width: f64, height: f64) {
    STATE.lock().shape = (width, height);
}

#[tauri::command]
pub fn notch_enabled() -> bool {
    STATE.lock().enabled
}

#[tauri::command]
pub fn notch_set_enabled(app: AppHandle, enabled: bool) -> Result<(), String> {
    if let Some(c) = core::get() {
        c.library.set_setting(SETTING, if enabled { "true" } else { "false" }).map_err(|e| e.to_string())?;
    }
    let handle = app.clone();
    app.run_on_main_thread(move || set_enabled(&handle, enabled)).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn notch_open_app(app: AppHandle) {
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.unminimize();
        let _ = main.show();
        let _ = main.set_focus();
    }
}
