// Browser preview of the notch window (pnpm dev → /notch.html). Not bundled in production.
import { emit } from "@tauri-apps/api/event";
import { mockIPC, mockConvertFileSrc } from "@tauri-apps/api/mocks";

export function installNotchMocks() {
  mockConvertFileSrc("macos");
  document.documentElement.style.background = "linear-gradient(#1d2b44, #42516e)";
  const state = {
    playing: true, position: 74, duration: 241, index: 1, volume: 0.7, shuffle: false, repeat: "all", error: null, queueVersion: 1,
    track: { id: 2, path: "/music/2.flac", title: "Harbor Lights", artist: "Aurora Lines", album: "Night Drive", duration: 241, cover: null },
    stream: { codec: "flac", sampleRate: 96000, bitDepth: 24, channels: 2, lossless: true, duration: 241, outputRate: 48000 },
  };
  mockIPC(
    (cmd) => {
      if (cmd === "player_state") return state;
      if (cmd === "notch_geometry") return { notchWidth: 185, notchHeight: 32, hasNotch: true };
      return null;
    },
    { shouldMockEvents: true },
  );
  // window.__notch(true|false) simulates hover; window.__track() simulates a song change.
  Object.assign(window, {
    __notch: (on: boolean) => emit("notch://hover", on),
    __track: () => emit("player://state", { ...state, track: { ...state.track, path: "/music/3.flac", title: "Slow Orbit" } }),
  });
}
