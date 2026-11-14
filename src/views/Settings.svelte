<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { isAndroid, isIOS, isMobileOS } from "../lib/platform";
  import { api } from "../lib/api";
  import { store } from "../lib/store.svelte";
  import Icon from "../components/Icon.svelte";

  async function addFolder() {
    const dir = await open({ directory: true, multiple: false, title: "Choose a music folder" });
    if (typeof dir !== "string") return;
    await api.addFolder(dir);
    store.folders = await api.folders();
    store.notify("Scanning your music…");
  }

  async function removeFolder(path: string) {
    await api.removeFolder(path);
    store.folders = await api.folders();
  }

  const formats = [
    ["Lossless", "FLAC · ALAC · WAV · AIFF · APE (Monkey's Audio) · WavPack · TTA · TAK · WMA Lossless"],
    ["Hi-Res", "Up to 32-bit / 384 kHz PCM · DSD64–DSD512 (DSF/DFF, converted to PCM)"],
    ["Lossy", "MP3 · AAC / HE-AAC · Opus · Ogg Vorbis · WMA · Musepack · Speex"],
  ];

  // null = not supported here (only the macOS build has the notch player).
  let notch = $state<boolean | null>(null);
  api.notchEnabled().then((on) => (notch = on)).catch(() => {});
  function toggleNotch() {
    notch = !notch;
    api.setNotchEnabled(notch);
  }

  let carAutoplay = $state(true);
  api.settings().then((s) => (carAutoplay = s.car_autoplay)).catch(() => {});
  function toggleCarAutoplay() {
    carAutoplay = !carAutoplay;
    api.setSetting("car_autoplay", String(carAutoplay));
  }

  const lossless = $derived(store.tracks.filter((t) => t.lossless).length);
  const hires = $derived(store.tracks.filter((t) => t.lossless && ((t.bitDepth ?? 0) > 16 || (t.sampleRate ?? 0) > 48000)).length);
</script>

<div class="page fade-in">
  <div class="eyebrow">Preferences</div>
  <h1 class="page-title">Settings</h1>

  <section>
    <h2 class="section-title">Music folders</h2>
    {#if isIOS}
      <p class="hint">
        Copy music into the Files app → On My iPhone → Catlify (or drag it there from Finder on your Mac), then tap Rescan.
        Files are never modified.
      </p>
    {:else if isAndroid}
      <p class="hint">Catlify reads the Music and Download folders on your phone. Files are never modified.</p>
    {:else}
      <p class="hint">Catlify watches these folders and reads tags, embedded artwork and lyrics. Files are never modified.</p>
    {/if}
    <div class="folders">
      {#each store.folders as f (f)}
        <div class="folder">
          <Icon name="folder" size={17} />
          <span class="ellipsis">{f}</span>
          <button class="rm" onclick={() => removeFolder(f)} aria-label="Remove folder"><Icon name="trash" size={15} /></button>
        </div>
      {:else}
        <p class="hint">No folders yet.</p>
      {/each}
    </div>
    <div class="row">
      {#if !isMobileOS}
        <button class="btn primary" onclick={addFolder}><Icon name="plus" size={15} />Add folder</button>
      {/if}
      <button class="btn" onclick={() => api.rescan()} disabled={!store.folders.length}><Icon name="refresh" size={15} />Rescan</button>
      {#if store.scan}
        <span class="hint">
          {store.scan.done
            ? `Scan complete: ${store.scan.added} new or updated`
            : `Scanning ${store.scan.scanned.toLocaleString()} of ${store.scan.total.toLocaleString()}…`}
        </span>
      {/if}
    </div>
  </section>

  <section>
    <h2 class="section-title">Library</h2>
    <div class="facts">
      <div><b class="serif">{store.tracks.length.toLocaleString()}</b><span>songs</span></div>
      <div><b class="serif">{lossless.toLocaleString()}</b><span>lossless</span></div>
      <div><b class="serif">{hires.toLocaleString()}</b><span>hi-res</span></div>
      <div><b class="serif">{store.playlists.length}</b><span>playlists</span></div>
    </div>
  </section>

  <section>
    <h2 class="section-title">Playback</h2>
    <dl>
      <dt>Engine</dt>
      <dd>FFmpeg decoder → 32-bit float pipeline → system output at {store.player.stream ? `${store.player.stream.outputRate / 1000} kHz` : "device rate"}</dd>
      <dt>Gapless</dt>
      <dd>Always on. The next track is decoded before the current one ends.</dd>
      {#each formats as [k, v]}
        <dt>{k}</dt>
        <dd>{v}</dd>
      {/each}
    </dl>
  </section>

  {#if notch !== null}
    <section>
      <h2 class="section-title">Notch</h2>
      <label class="toggle">
        <input type="checkbox" checked={notch} onchange={toggleNotch} />
        <span>
          <b>Show the player in the notch</b>
          <small>Artwork and a level meter sit beside the camera. Hover to open controls. On displays without a notch it appears in the middle of the menu bar.</small>
        </span>
      </label>
    </section>
  {/if}

  <section>
    <h2 class="section-title">Car &amp; Bluetooth</h2>
    <label class="toggle">
      <input type="checkbox" checked={carAutoplay} onchange={toggleCarAutoplay} />
      <span>
        <b>Start playing when the car connects</b>
        <small>Picks up where you left off when your phone joins the car’s Bluetooth, even if Catlify isn’t open (Android needs the “Nearby devices” permission). Turn this off to ignore the “play” some head units (e.g. Mazda Connect) send on connect.</small>
      </span>
    </label>
    <dl>
      <dt>Android Auto</dt><dd>Browse Recent, Library, Billboard and Playlists on the car screen.</dd>
      <dt>CarPlay</dt><dd>Same tabs, plus the CarPlay Now Playing screen.</dd>
      <dt>Bluetooth</dt><dd>Title, artist, album, artwork, track number and steering-wheel controls. Long-press skip seeks. Playback pauses when the car disconnects.</dd>
    </dl>
  </section>

  {#if !isMobileOS}
  <section>
    <h2 class="section-title">Keyboard</h2>
    <dl class="keys">
      <dt><kbd>Space</kbd></dt><dd>Play / pause</dd>
      <dt><kbd>⌘</kbd> <kbd>→</kbd> / <kbd>←</kbd></dt><dd>Next / previous track</dd>
      <dt><kbd>→</kbd> / <kbd>←</kbd></dt><dd>Seek 5 seconds</dd>
      <dt><kbd>⌘</kbd> <kbd>↑</kbd> / <kbd>↓</kbd></dt><dd>Volume</dd>
      <dt><kbd>⌘</kbd> <kbd>F</kbd></dt><dd>Search</dd>
      <dt><kbd>L</kbd> · <kbd>Q</kbd></dt><dd>Lyrics · Queue</dd>
    </dl>
  </section>
  {/if}

  <section>
    <h2 class="section-title">About</h2>
    <p class="hint">
      Catlify 0.1.0. Charts from Apple Music and Deezer; synced lyrics from LRCLIB. Drop audio files anywhere in the window to play them.
    </p>
  </section>
</div>

<style>
  section {
    max-width: 820px;
  }
  .hint {
    color: var(--muted);
    font-size: 13px;
    margin: 0 0 14px;
  }
  .folders {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-bottom: 14px;
  }
  .folder {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 14px;
    border-radius: 10px;
    background: var(--surface);
    color: var(--muted);
  }
  .folder span {
    flex: 1;
    color: var(--text);
    font-family: var(--mono);
    font-size: 12.5px;
  }
  .rm {
    color: var(--dim);
  }
  .rm:hover {
    color: #ff8a8a;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .row .hint {
    margin: 0 0 0 6px;
  }
  .facts {
    display: flex;
    gap: 40px;
  }
  .facts div {
    display: flex;
    flex-direction: column;
  }
  .facts b {
    font-size: 30px;
    line-height: 1;
  }
  .facts span {
    color: var(--dim);
    font-size: 12px;
    margin-top: 4px;
  }
  dl {
    display: grid;
    grid-template-columns: 160px 1fr;
    gap: 10px 20px;
    margin: 0;
    font-size: 13.5px;
  }
  dt {
    color: var(--dim);
  }
  dd {
    margin: 0;
    color: var(--muted);
  }
  .toggle {
    display: flex;
    gap: 12px;
    align-items: flex-start;
    padding: 14px;
    border-radius: 10px;
    background: var(--surface);
    margin-bottom: 16px;
    cursor: pointer;
  }
  .toggle input {
    margin-top: 3px;
    accent-color: var(--coral);
  }
  .toggle span {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .toggle small {
    color: var(--muted);
    font-size: 12.5px;
  }
  kbd {
    display: inline-block;
    min-width: 22px;
    padding: 1px 6px;
    border-radius: 5px;
    border: 1px solid var(--line-strong);
    border-bottom-width: 2px;
    font-family: var(--sans);
    font-size: 11.5px;
    text-align: center;
    color: var(--text);
  }
</style>
