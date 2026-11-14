import { api, ytBridge } from "./api";

const ua = navigator.userAgent;
export const isAndroid = /Android/i.test(ua);
export const isIOS = /iPhone|iPad|iPod/i.test(ua) || (ua.includes("Mac") && navigator.maxTouchPoints > 1);
/** Phone apps (not just a narrow desktop window). */
export const isMobileOS = isAndroid || isIOS;

/** Phone-sized layout: narrow windows, or narrow touch screens (small tablets in portrait). */
export const mobileQuery = matchMedia("(max-width: 760px), (pointer: coarse) and (max-width: 1100px)");

/** A phone (not a tablet): always the phone layout, in portrait and landscape alike. */
export const isPhone = isMobileOS && Math.min(screen.width, screen.height) < 600;

export const useMobileLayout = () => isPhone || mobileQuery.matches;

/**
 * YouTube (full songs, downloads, videos): desktop and iPhone/iPad run yt-dlp through Rust (Mac and
 * iOS bundle it via ytdlp-apple); Android builds that bundle yt-dlp expose window.StafyYouTube.
 */
export const ytAvailable = () => !isAndroid || !!ytBridge();

/**
 * Safe areas for notches / punch-hole cameras / home indicator. iOS fills env(safe-area-inset-*);
 * Android's WebView doesn't reliably, so MainActivity.kt exposes the real insets (incl. the display
 * cutout and keyboard) and we mirror them into CSS variables used by app.css.
 */
export function initPlatform() {
  const root = document.documentElement;
  root.classList.toggle("android", isAndroid);
  root.classList.toggle("ios", isIOS);
  root.classList.toggle("phone", isPhone);
  const w = window as unknown as {
    StafyInsets?: { get(): string };
    __stafyInsets?: () => void;
    __stafyRescan?: () => void;
  };
  const apply = () => {
    if (!w.StafyInsets) return;
    try {
      const i = JSON.parse(w.StafyInsets.get());
      for (const k of ["top", "right", "bottom", "left", "keyboard"]) root.style.setProperty(`--native-${k}`, `${i[k] ?? 0}px`);
    } catch {
      /* ignore */
    }
  };
  w.__stafyInsets = apply;
  apply();
  addEventListener("resize", apply);
  // MainActivity calls this once the media permission is granted.
  w.__stafyRescan = () => api.rescan();
}

/**
 * Keep the screen awake (e.g. while lyrics are visible). Reference-counted, so several views can
 * hold it. Android uses FLAG_KEEP_SCREEN_ON via MainActivity; elsewhere the Screen Wake Lock API.
 */
let awakeHolders = 0;
let wakeLock: { release(): Promise<void> } | null = null;

async function applyAwake() {
  const on = awakeHolders > 0;
  const native = (window as unknown as { StafyScreen?: { keepOn(on: boolean): void } }).StafyScreen;
  if (native) return native.keepOn(on);
  const wl = (navigator as unknown as { wakeLock?: { request(t: "screen"): Promise<{ release(): Promise<void> }> } }).wakeLock;
  if (!wl) return;
  try {
    if (on && !wakeLock && document.visibilityState === "visible") wakeLock = await wl.request("screen");
    else if (!on && wakeLock) {
      await wakeLock.release();
      wakeLock = null;
    }
  } catch {
    wakeLock = null; // denied (e.g. low battery); nothing else to do
  }
}

// The browser drops wake locks when the app is hidden; take it again on return.
document.addEventListener("visibilitychange", () => {
  if (document.visibilityState === "visible") {
    wakeLock = null;
    applyAwake();
  }
});

/** Returns a release function. */
export function keepScreenOn(): () => void {
  awakeHolders++;
  applyAwake();
  let released = false;
  return () => {
    if (released) return;
    released = true;
    awakeHolders = Math.max(0, awakeHolders - 1);
    applyAwake();
  };
}
