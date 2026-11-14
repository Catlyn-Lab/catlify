// In-app updates (macOS, Windows, Linux) from GitHub releases: tauri-plugin-updater checks
// latest.json (published by .github/workflows/release.yml), verifies the signature, installs and
// restarts. Phones update through their stores / Firebase instead.
import type { Update } from "@tauri-apps/plugin-updater";
import { isMobileOS } from "./platform";
import { store } from "./store.svelte";

const SKIP_KEY = "catlify.update.skip";
const FIRST_CHECK_MS = 8_000;
const EVERY_MS = 6 * 60 * 60 * 1000;

export type UpdatePhase = "idle" | "checking" | "downloading" | "installing" | "error";

class Updater {
  /** The newer version found by the last check. */
  update = $state<Update | null>(null);
  /** The update popup is showing. */
  open = $state(false);
  phase = $state<UpdatePhase>("idle");
  /** Download progress 0…1, or -1 when the size is unknown. */
  progress = $state(0);
  error = $state("");
  lastChecked = $state<number | null>(null);

  get supported() {
    return !isMobileOS && "__TAURI_INTERNALS__" in window;
  }
  #timer: ReturnType<typeof setInterval> | null = null;

  get busy() {
    return this.phase === "checking" || this.phase === "downloading" || this.phase === "installing";
  }

  /** Checks shortly after launch, then every few hours. */
  start() {
    if (!this.supported || this.#timer) return;
    setTimeout(() => this.check(), FIRST_CHECK_MS);
    this.#timer = setInterval(() => this.check(), EVERY_MS);
  }

  /** `manual`: from the Settings button, so always show the result (even a skipped version). */
  async check(manual = false) {
    if (!this.supported || this.busy) return;
    this.phase = "checking";
    this.error = "";
    try {
      const { check } = await import("@tauri-apps/plugin-updater");
      const update = await check();
      this.lastChecked = Date.now();
      this.phase = "idle";
      this.update = update;
      if (!update) {
        if (manual) store.notify("Catlify is up to date");
        return;
      }
      if (manual || localStorage.getItem(SKIP_KEY) !== update.version) this.open = true;
    } catch (e) {
      this.phase = manual ? "error" : "idle";
      this.error = String(e);
      if (manual) store.notify("Couldn't check for updates");
    }
  }

  /** Downloads, verifies and installs, then restarts Catlify. */
  async install() {
    const update = this.update;
    if (!update || this.busy) return;
    this.open = true;
    this.phase = "downloading";
    this.progress = -1;
    this.error = "";
    let total = 0;
    let received = 0;
    try {
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") {
          total = event.data.contentLength ?? 0;
          this.progress = total ? 0 : -1;
        } else if (event.event === "Progress") {
          received += event.data.chunkLength;
          this.progress = total ? Math.min(1, received / total) : -1;
        } else {
          this.phase = "installing";
        }
      });
      // Windows exits by itself to run the installer; macOS and Linux restart here.
      const { relaunch } = await import("@tauri-apps/plugin-process");
      await relaunch();
    } catch (e) {
      this.phase = "error";
      this.error = String(e);
    }
  }

  later() {
    if (this.phase === "downloading" || this.phase === "installing") return;
    this.open = false;
    if (this.phase === "error") this.phase = "idle";
  }

  /** Don't pop up for this version again (Settings still offers it). */
  skip() {
    if (this.update) localStorage.setItem(SKIP_KEY, this.update.version);
    this.later();
  }
}

export const updater = new Updater();
