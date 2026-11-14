<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { api, ytBridge, type YtStatus, type YtVideo } from "../lib/api";
  import { isMobileOS, isWindows } from "../lib/platform";
  import { downloads } from "../lib/downloads.svelte";
  import { store } from "../lib/store.svelte";
  import { fmtTime, toQueueItem } from "../lib/util";
  import Icon from "../components/Icon.svelte";
  import { prepare, ytQueueItem } from "../lib/yt";

  let { query: initial }: { query?: string } = $props();

  // The view is re-created ({#key}) when navigated to with a new query, so the initial value is enough.
  // svelte-ignore state_referenced_locally
  let query = $state(initial ?? "");
  let results = $state<YtVideo[]>([]);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let status = $state<YtStatus | null>(null);
  let unsupported = $state(false); // phones: no yt-dlp
  let format = $state<"mp3" | "original">((localStorage.getItem("stafy.ytFormat") as "mp3" | "original") ?? "mp3");
  // Editable song title/artist per video, prefilled from the video title.
  let edits = $state<Record<string, { title: string; artist: string }>>({});

  // ffmpeg is optional: without it (iPhone, or a Mac without Homebrew) songs are saved as M4A.
  const ready = $derived(!!status?.ytDlp);
  const canMp3 = $derived(!!status?.ffmpeg);
  $effect(() => {
    if (status && !canMp3) format = "original";
  });

  api.ytStatus().then((s) => {
    status = s;
    if (initial && s.ytDlp) search();
  }).catch(() => (unsupported = true));

  $effect(() => localStorage.setItem("stafy.ytFormat", format));

  async function search() {
    const q = query.trim();
    if (!q) return;
    loading = true;
    error = null;
    try {
      results = await api.ytLookup(q);
      prepare(results.slice(0, 2).map((v) => `yt:${v.id}`)); // most likely to be played
      for (const v of results) edits[v.id] ??= { title: v.title, artist: v.artist };
      if (!results.length) error = "No videos found.";
    } catch (e) {
      error = String(e);
      results = [];
    } finally {
      loading = false;
    }
  }

  async function paste() {
    try {
      const text = await navigator.clipboard.readText();
      if (text) {
        query = text.trim();
        search();
      }
    } catch {
      /* clipboard not available */
    }
  }

  function download(v: YtVideo) {
    const e = edits[v.id] ?? { title: v.title, artist: v.artist };
    downloads.start(v, e.title.trim() || v.title, e.artist.trim(), format);
  }

  /** Stream the full song now (no download); the other results queue after it. */
  function playStream(i: number) {
    const items = results.map((v) =>
      ytQueueItem({
        videoId: v.id,
        title: edits[v.id]?.title || v.title,
        artist: edits[v.id]?.artist ?? v.artist,
        cover: v.thumbnail,
        duration: v.duration,
      }),
    );
    store.playItems(items, i);
  }
  const nowPath = $derived(store.player.track?.path);

  function playDownloaded(path: string) {
    const t = store.tracks.find((t) => t.path === path);
    if (t) store.playItems([toQueueItem(t)], 0);
    else store.notify("Still adding it to your library…");
  }

  async function chooseFolder() {
    const dir = await open({ directory: true, multiple: false, title: "Save downloads to" });
    if (typeof dir !== "string") return;
    await api.ytSetDir(dir);
    status = await api.ytStatus();
  }

  const recent = $derived(Object.values(downloads.items).reverse());

  // Android bundles yt-dlp; YouTube changes often, so allow a manual refresh too.
  let updating = $state(false);
  async function updateYtDlp() {
    updating = true;
    try {
      const v = await api.ytUpdate();
      store.notify(v ? `yt-dlp updated to ${v}` : "yt-dlp is up to date");
      status = await api.ytStatus();
    } catch (e) {
      store.notify(`Update failed: ${e}`);
    } finally {
      updating = false;
    }
  }
</script>

<div class="page fade-in">
  <div class="eyebrow">Download</div>
  <h1 class="page-title">YouTube</h1>
  <p class="lede">Paste a link or search. Catlify reads the song title and artist from the video, then saves the audio to your library.</p>

  {#if !status && !unsupported}
    <p class="preparing">Getting the YouTube downloader ready… The first time takes a few seconds.</p>
  {/if}

  {#if unsupported}
    <div class="setup"><Icon name="download" size={18} /><div><b>Desktop only</b><p>YouTube downloads are available in Catlify for Mac, Windows and Linux. Downloaded songs sync like any other file.</p></div></div>
  {:else if status && !ready}
    <div class="setup">
      <Icon name="download" size={18} />
      <div>
        <b>One-time setup</b>
        <p>
          Catlify uses <code>yt-dlp</code>{status.ffmpeg ? "" : " and ffmpeg"} to download audio. Install {isWindows ? "it" : "with Homebrew"}, then reopen this page:
        </p>
        <code class="cmd">{isWindows ? "winget install yt-dlp.yt-dlp" : `brew install yt-dlp${status.ffmpeg ? "" : " ffmpeg"}`}</code>
      </div>
    </div>
  {:else if status && !canMp3 && !isMobileOS}
    <p class="hint">Songs are saved as M4A (YouTube's own audio). For MP3, install ffmpeg: <code>{isWindows ? "winget install Gyan.FFmpeg" : "brew install ffmpeg"}</code></p>
  {/if}

  <form class="bar" onsubmit={(e) => (e.preventDefault(), search())}>
    <input bind:value={query} placeholder="YouTube link, or search for a song…" spellcheck="false" disabled={!ready} />
    <button type="button" class="btn" onclick={paste} disabled={!ready}>Paste link</button>
    <button type="submit" class="btn primary" disabled={!ready || loading || !query.trim()}>
      <Icon name="search" size={15} />{loading ? "Searching…" : "Search"}
    </button>
  </form>

  <div class="options">
    <div class="seg" role="radiogroup" aria-label="Format">
      {#if canMp3}<button class:active={format === "mp3"} onclick={() => (format = "mp3")}>MP3</button>{/if}
      <button class:active={format === "original"} onclick={() => (format = "original")}>Original (M4A)</button>
    </div>
    <span class="hint">
      {format === "mp3"
        ? "Converted to MP3 (VBR ~245 kbps). Plays on anything."
        : "YouTube's own AAC audio with no re-encoding, so no extra quality loss."}
    </span>
    {#if status?.downloadDir}
      {#if isMobileOS}
        <span class="folder"><Icon name="folder" size={14} /><span class="ellipsis">{status.downloadDir}</span></span>
      {:else}
        <button class="folder" onclick={chooseFolder} title="Change download folder">
          <Icon name="folder" size={14} /><span class="ellipsis">{status.downloadDir}</span>
        </button>
      {/if}
    {/if}
    {#if ytBridge() && status?.ytDlp}
      <button class="link" onclick={updateYtDlp} disabled={updating}>
        {updating ? "Updating…" : `yt-dlp ${status.ytDlp} · Update`}
      </button>
    {/if}
  </div>

  {#if error}<p class="error">{error}</p>{/if}

  {#if results.length}
    <div class="results">
      {#each results as v, i (v.id)}
        {@const d = downloads.items[v.id]}
        {@const busy = d && (d.progress.stage === "downloading" || d.progress.stage === "converting")}
        <div class="result">
          <button
            class="thumb"
            class:playing={nowPath === `yt:${v.id}`}
            onclick={() => playStream(i)}
            onpointerenter={() => prepare([`yt:${v.id}`])}
            ontouchstart={() => prepare([`yt:${v.id}`])}
            title="Play"
          >
            <img src={v.thumbnail} alt="" loading="lazy" draggable="false" />
            <span class="ov">
              {#if nowPath === `yt:${v.id}` && store.player.loading}<span class="spin"></span>
              {:else}<Icon name={nowPath === `yt:${v.id}` && store.player.playing ? "pause" : "play"} size={22} />{/if}
            </span>
            {#if v.duration}<span class="dur">{fmtTime(v.duration)}</span>{/if}
          </button>
          <div class="meta">
            <div class="ellipsis vt" title={v.videoTitle}>{v.videoTitle}</div>
            <div class="ellipsis ch">
              {v.channel}
              <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
              <span class="yt-link" onclick={() => openUrl(v.url)}>· Open on YouTube</span>
            </div>
            {#if edits[v.id]}
              <div class="fields">
                <label><span>Title</span><input bind:value={edits[v.id].title} disabled={busy} /></label>
                <label><span>Artist</span><input bind:value={edits[v.id].artist} disabled={busy} /></label>
              </div>
            {/if}
          </div>
          <div class="action">
            {#if busy}
              <div class="prog">
                <span class="pbar"><span style:width="{d.progress.percent}%"></span></span>
                <span class="ptext">{d.progress.stage === "converting" ? "Converting…" : `${Math.round(d.progress.percent)}%`}</span>
                <button class="link" onclick={() => downloads.cancel(v.id)}>Cancel</button>
              </div>
            {:else if d?.progress.stage === "done" && d.progress.path}
              <button class="btn" onclick={() => playDownloaded(d.progress.path!)}><Icon name="play" size={14} />Play</button>
            {:else}
              <button class="btn" onclick={() => playStream(i)}><Icon name="play" size={14} />Play</button>
              <button class="btn" onclick={() => store.openVideo({ videoId: v.id, title: v.videoTitle, artist: v.channel })} title="See video">
                <Icon name="video" size={15} />Video
              </button>
              <button class="btn primary" onclick={() => download(v)}>
                <Icon name="download" size={15} />{d?.progress.stage === "error" ? "Retry" : format === "mp3" ? "MP3" : "M4A"}
              </button>
            {/if}
          </div>
        </div>
      {/each}
    </div>
  {/if}

  {#if recent.length}
    <h2 class="section-title">Downloads</h2>
    <div class="recent">
      {#each recent as d (d.video.id)}
        <div class="row">
          <span class="ellipsis"><b>{d.title}</b> <span class="muted">· {d.artist}</span></span>
          <span class="state {d.progress.stage}">
            {#if d.progress.stage === "downloading"}{Math.round(d.progress.percent)}%
            {:else if d.progress.stage === "converting"}Converting…
            {:else if d.progress.stage === "done"}In library
            {:else if d.progress.stage === "cancelled"}Cancelled
            {:else}<span title={d.progress.error ?? ""}>Failed</span>{/if}
          </span>
        </div>
      {/each}
    </div>
  {/if}

  <p class="note">
    Only download videos you own or have permission to save, such as your own uploads or Creative Commons music. Respect
    YouTube's Terms of Service and artists' rights.
  </p>
</div>

<style>
  .lede {
    color: var(--muted);
    margin: 4px 0 22px;
    max-width: 640px;
  }
  .setup {
    display: flex;
    gap: 14px;
    padding: 16px 18px;
    border-radius: var(--radius-lg);
    background: var(--surface);
    border: 1px solid var(--line-strong);
    margin-bottom: 20px;
    color: var(--coral);
  }
  .setup div {
    color: var(--text);
  }
  .setup p {
    margin: 4px 0 10px;
    color: var(--muted);
  }
  code {
    font-family: var(--mono);
    font-size: 12.5px;
  }
  .cmd {
    display: inline-block;
    padding: 6px 10px;
    border-radius: 8px;
    background: var(--bg);
    user-select: all;
  }
  .bar {
    display: flex;
    gap: 8px;
  }
  .bar input {
    flex: 1;
    height: 40px;
    padding: 0 14px;
    border-radius: var(--radius);
    border: 1px solid var(--line-strong);
    background: var(--surface);
    color: var(--text);
    font: inherit;
    outline: none;
  }
  .bar input:focus {
    border-color: var(--violet);
  }
  .options {
    display: flex;
    align-items: center;
    gap: 14px;
    margin: 14px 0 22px;
    flex-wrap: wrap;
  }
  .seg {
    display: inline-flex;
    padding: 3px;
    border-radius: 99px;
    background: var(--surface);
  }
  .seg button {
    border: 0;
    background: none;
    color: var(--muted);
    font: inherit;
    font-size: 12.5px;
    font-weight: 600;
    padding: 5px 12px;
    border-radius: 99px;
    cursor: pointer;
  }
  .seg button.active {
    background: var(--surface-hover);
    color: var(--text);
  }
  .hint {
    color: var(--dim);
    font-size: 12.5px;
  }
  .folder {
    margin-left: auto;
    display: inline-flex;
    gap: 6px;
    align-items: center;
    max-width: 320px;
    border: 0;
    background: none;
    color: var(--muted);
    font: inherit;
    font-size: 12.5px;
    cursor: pointer;
  }
  .folder:hover {
    color: var(--text);
  }
  .ellipsis {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .preparing {
    color: var(--muted);
    animation: pulse 1.4s ease-in-out infinite alternate;
  }
  @keyframes pulse {
    from {
      opacity: 0.55;
    }
  }
  .error {
    color: var(--coral);
  }
  .results {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .result {
    display: grid;
    grid-template-columns: 168px 1fr auto;
    gap: 16px;
    align-items: center;
    padding: 10px;
    border-radius: var(--radius-lg);
    transition: background 0.15s ease;
  }
  .result:hover {
    background: var(--surface);
  }
  .thumb {
    position: relative;
    padding: 0;
    border: 0;
    background: var(--surface);
    border-radius: 10px;
    overflow: hidden;
    aspect-ratio: 16 / 9;
    cursor: pointer;
  }
  .thumb img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }
  .dur {
    position: absolute;
    right: 6px;
    bottom: 6px;
    font-size: 11px;
    font-weight: 600;
    padding: 1px 5px;
    border-radius: 4px;
    background: rgba(0, 0, 0, 0.75);
    color: #fff;
  }
  .meta {
    min-width: 0;
  }
  .vt {
    font-weight: 600;
  }
  .ch {
    color: var(--muted);
    font-size: 12.5px;
  }
  .fields {
    display: flex;
    gap: 8px;
    margin-top: 8px;
  }
  .fields label {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    background: var(--bg-raised);
    border: 1px solid var(--line);
    border-radius: 8px;
    padding: 0 8px;
  }
  .fields span {
    font-size: 11px;
    color: var(--dim);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }
  .fields input {
    flex: 1;
    min-width: 0;
    height: 28px;
    border: 0;
    background: none;
    color: var(--text);
    font: inherit;
    font-size: 13px;
    outline: none;
  }
  .action {
    min-width: 110px;
    display: flex;
    gap: 8px;
    justify-content: flex-end;
  }
  .ov {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    background: rgba(0, 0, 0, 0.35);
    color: #fff;
    opacity: 0;
    transition: opacity 0.15s ease;
  }
  .thumb:hover .ov,
  .thumb.playing .ov {
    opacity: 1;
  }
  .thumb.playing {
    outline: 2px solid var(--coral);
    outline-offset: 2px;
  }
  .spin {
    width: 22px;
    height: 22px;
    border-radius: 50%;
    border: 2.5px solid rgba(255, 255, 255, 0.3);
    border-top-color: #fff;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .yt-link {
    color: var(--dim);
    cursor: pointer;
  }
  .yt-link:hover {
    color: var(--text);
  }
  .prog {
    display: grid;
    grid-template-columns: 110px;
    gap: 4px;
    justify-items: end;
  }
  .pbar {
    width: 100%;
    height: 4px;
    border-radius: 2px;
    background: var(--line-strong);
    overflow: hidden;
  }
  .pbar span {
    display: block;
    height: 100%;
    background: var(--accent);
    transition: width 0.3s ease;
  }
  .ptext {
    font-size: 12px;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .link {
    border: 0;
    background: none;
    color: var(--dim);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
    padding: 0;
  }
  .link:hover {
    color: var(--coral);
  }
  .recent {
    display: flex;
    flex-direction: column;
    border-top: 1px solid var(--line);
  }
  .row {
    display: flex;
    justify-content: space-between;
    gap: 16px;
    padding: 9px 4px;
    border-bottom: 1px solid var(--line);
    font-size: 13px;
  }
  .muted {
    color: var(--muted);
  }
  .state {
    color: var(--muted);
    flex: none;
  }
  .state.done {
    color: var(--violet);
  }
  .state.error {
    color: var(--coral);
  }
  /* ---- phones ---- */
  :global(html.m) .bar {
    flex-wrap: wrap;
  }
  :global(html.m) .bar input {
    flex-basis: 100%;
    height: 46px;
  }
  :global(html.m) .bar .btn {
    flex: 1;
    justify-content: center;
    height: 42px;
  }
  :global(html.m) .options {
    gap: 10px;
    margin: 12px 0 16px;
  }
  :global(html.m) .hint {
    flex-basis: 100%;
    order: 3;
  }
  :global(html.m) .folder {
    margin-left: 0;
  }
  :global(html.m) .result {
    grid-template-columns: 116px minmax(0, 1fr);
    gap: 10px 12px;
    padding: 10px 0;
    border-radius: 0;
    border-bottom: 1px solid var(--line);
  }
  :global(html.m) .result:hover {
    background: none;
  }
  :global(html.m) .fields {
    grid-column: 1 / -1;
  }
  :global(html.m) .meta {
    display: contents;
  }
  :global(html.m) .meta > .vt,
  :global(html.m) .meta > .ch {
    grid-column: 2;
  }
  :global(html.m) .vt {
    white-space: normal;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    align-self: end;
  }
  :global(html.m) .ch {
    align-self: start;
  }
  :global(html.m) .thumb {
    grid-row: span 2;
  }
  :global(html.m) .fields {
    flex-direction: column;
    margin-top: 0;
  }
  :global(html.m) .fields input {
    height: 36px;
  }
  :global(html.m) .action {
    grid-column: 1 / -1;
    min-width: 0;
  }
  :global(html.m) .action .btn {
    flex: 1;
    justify-content: center;
    height: 42px;
  }
  :global(html.m) .prog {
    width: 100%;
    grid-template-columns: 1fr auto auto;
    align-items: center;
    gap: 10px;
  }
  .note {
    margin-top: 28px;
    color: var(--dim);
    font-size: 12.5px;
    max-width: 640px;
  }
</style>
