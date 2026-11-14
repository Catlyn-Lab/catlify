<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { onDestroy } from "svelte";
  import { api, type VideoStream } from "../lib/api";
  import { isAndroid, isIOS, keepScreenOn, ytAvailable } from "../lib/platform";
  import { store } from "../lib/store.svelte";
  import Icon from "./Icon.svelte";
  import Spinner from "./Spinner.svelte";

  const req = store.video!;
  const isId = /^[\w-]{11}$/.test(req.query);

  let stream = $state<VideoStream | null>(null);
  let error = $state<string | null>(null);
  /** yt-dlp unavailable (iOS) or its link failed: YouTube's own embedded player. */
  let embedId = $state<string | null>(null);

  // Music and video at once is never wanted; pick the song back up afterwards.
  const resume = store.player.playing;
  if (resume) api.control("pause");
  const release = keepScreenOn();
  onDestroy(() => {
    release();
    exitFull();
    if (resume) api.control("play");
  });

  // ---- Fullscreen as soon as the video plays: landscape on phones, full window on desktop ----
  let full = $state(false);
  let videoEl: HTMLVideoElement | undefined = $state();
  let madeWindowFull = false;
  const screenBridge = () => (window as unknown as { StafyScreen?: { videoMode?(on: boolean): void } }).StafyScreen;

  async function enterFull() {
    if (full) return;
    full = true;
    if (isAndroid) {
      screenBridge()?.videoMode?.(true); // MainActivity: landscape, system bars hidden
    } else if (isIOS) {
      // iOS's own player: fullscreen and it follows the phone into landscape.
      (videoEl as unknown as { webkitEnterFullscreen?(): void } | undefined)?.webkitEnterFullscreen?.();
    } else {
      try {
        const w = getCurrentWindow();
        if (!(await w.isFullscreen())) {
          await w.setFullscreen(true);
          madeWindowFull = true;
        }
      } catch {
        // Browser preview: the in-page full view is enough.
      }
    }
  }

  function exitFull() {
    if (isAndroid) screenBridge()?.videoMode?.(false);
    if (madeWindowFull) getCurrentWindow().setFullscreen(false).catch(() => {});
    madeWindowFull = false;
  }

  const searchUrl = `https://www.youtube.com/results?search_query=${encodeURIComponent(req.query + (isId ? "" : " official music video"))}`;
  const watchUrl = $derived(stream?.id || embedId ? `https://www.youtube.com/watch?v=${stream?.id ?? embedId}` : isId ? `https://www.youtube.com/watch?v=${req.query}` : searchUrl);

  if (ytAvailable()) {
    api
      .ytVideo(req.query)
      .then((s) => {
        stream = s;
        if (!s.url) embedId = s.id;
      })
      .catch((e) => {
        if (isId) embedId = req.query;
        else error = String(e);
      });
  } else if (isId) {
    embedId = req.query;
  } else {
    // No yt-dlp here (iPhone): YouTube can search; we can't.
    openUrl(searchUrl);
    store.video = null;
  }

  function fallBack() {
    // Direct links expire after a few hours or can fail on some networks.
    if (stream?.id) {
      embedId = stream.id;
      stream = { ...stream, url: null };
    } else error = "The video couldn't be played";
  }

  const close = () => (store.video = null);
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && close()} />

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="scrim fade-in" class:full onclick={close}>
  <div class="box" onclick={(e) => e.stopPropagation()} role="dialog" aria-label="Video" tabindex="-1">
    <header>
      <div class="meta">
        <b class="ellipsis">{stream?.title ?? req.title}</b>
        <span class="ellipsis">{stream?.channel ?? req.artist ?? ""}</span>
      </div>
      <button class="icon-btn" onclick={() => openUrl(watchUrl)} title="Open on YouTube" aria-label="Open on YouTube">
        <Icon name="external" size={18} />
      </button>
      <button class="icon-btn" onclick={close} aria-label="Close"><Icon name="x" size={20} /></button>
    </header>
    <div class="screen">
      {#if stream?.url}
        <!-- svelte-ignore a11y_media_has_caption -->
        <video
          bind:this={videoEl}
          src={stream.url}
          poster={stream.thumbnail ?? undefined}
          controls
          autoplay
          playsinline
          onplaying={enterFull}
          onerror={fallBack}
        ></video>
      {:else if embedId}
        <iframe
          src="https://www.youtube-nocookie.com/embed/{embedId}?autoplay=1&playsinline=1&rel=0"
          title={req.title}
          allow="autoplay; encrypted-media; picture-in-picture; fullscreen"
          referrerpolicy="strict-origin-when-cross-origin"
          onload={enterFull}
          allowfullscreen
        ></iframe>
      {:else if error}
        <div class="msg">
          <p>{error}</p>
          <button class="btn" onclick={() => openUrl(watchUrl)}><Icon name="external" size={15} />Open on YouTube</button>
        </div>
      {:else}
        <div class="msg"><Spinner size={30} color="#fff" /><p>Finding the video…</p></div>
      {/if}
    </div>
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 200;
    background: rgba(0, 0, 0, 0.78);
    display: grid;
    place-items: center;
    padding: 24px;
  }
  .box {
    width: min(1100px, 100%);
    background: #000;
    border-radius: 14px;
    overflow: hidden;
    box-shadow: 0 30px 80px rgba(0, 0, 0, 0.6);
  }
  header {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 10px 10px 10px 16px;
    background: var(--surface);
  }
  .meta {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .meta span {
    color: var(--muted);
    font-size: 13px;
  }
  .icon-btn {
    width: 36px;
    height: 36px;
    display: grid;
    place-items: center;
    border-radius: 50%;
    color: var(--text);
  }
  .icon-btn:hover {
    background: var(--hover, rgba(255, 255, 255, 0.08));
  }
  .screen {
    aspect-ratio: 16 / 9;
    background: #000;
    display: grid;
  }
  video,
  iframe {
    width: 100%;
    height: 100%;
    border: 0;
  }
  .msg {
    display: grid;
    place-items: center;
    align-content: center;
    gap: 14px;
    color: #ddd;
    text-align: center;
    padding: 20px;
  }
  /* Playing: the video fills the screen; title and buttons float over the top edge. */
  .scrim.full {
    padding: 0;
    background: #000;
    place-items: stretch;
  }
  .full .box {
    width: 100vw;
    height: 100dvh;
    border-radius: 0;
    box-shadow: none;
    position: relative;
  }
  .full header {
    position: absolute;
    inset: 0 0 auto 0;
    z-index: 1;
    background: linear-gradient(rgba(0, 0, 0, 0.7), transparent);
    padding: max(10px, env(safe-area-inset-top)) max(12px, env(safe-area-inset-right)) 24px max(16px, env(safe-area-inset-left));
    opacity: 0.85;
    pointer-events: none;
  }
  .full header > * {
    pointer-events: auto;
  }
  .full .meta span {
    color: #ccc;
  }
  .full .icon-btn {
    color: #fff;
  }
  .full .screen {
    aspect-ratio: auto;
    height: 100%;
  }
  .full video {
    object-fit: contain;
  }
  @media (max-width: 760px) {
    .scrim {
      padding: 0;
      padding-top: env(safe-area-inset-top);
      align-items: start;
      background: #000;
    }
    .box {
      border-radius: 0;
      box-shadow: none;
    }
  }
</style>
