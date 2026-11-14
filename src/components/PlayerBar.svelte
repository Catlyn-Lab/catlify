<script lang="ts">
  import { api } from "../lib/api";
  import { store } from "../lib/store.svelte";
  import { fmtTime, isHiRes, streamLabel } from "../lib/util";
  import Cover from "./Cover.svelte";
  import Icon from "./Icon.svelte";
  import Slider from "./Slider.svelte";
  import Spinner from "./Spinner.svelte";
  import { downloads } from "../lib/downloads.svelte";
  import { isYouTubePath } from "../lib/yt";

  const p = $derived(store.player);
  const t = $derived(p.track);
  const libTrack = $derived(store.currentTrack);
  let scrub = $state<number | null>(null);
  let lastVolume = 0.8;

  const repeatNext = { off: "all", all: "one", one: "off" } as const;
  const hires = $derived(p.stream ? isHiRes(p.stream.bitDepth, p.stream.sampleRate, p.stream.lossless) : false);
  const isPreview = $derived(!!t && t.id === null && t.path.startsWith("http"));
  const fromYouTube = $derived(!!t && isYouTubePath(t.path));

  function toggleMute() {
    if (p.volume > 0) {
      lastVolume = p.volume;
      store.setVolume(0);
    } else store.setVolume(lastVolume || 0.8);
  }
</script>

<footer>
  <div class="left">
    {#if t}
      <button class="art" onclick={() => (store.nowPlayingOpen = !store.nowPlayingOpen)} aria-label="Now playing">
        <Cover src={t.cover} size="56px" radius="8px" seed={t.album} />
        <span class="expand"><Icon name="expand" size={16} /></span>
      </button>
      <div class="meta">
        <div class="title ellipsis">{t.title}</div>
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
        <div class="artist ellipsis link" onclick={() => libTrack && store.go({ name: "artist", artist: libTrack.albumArtist })}>
          {p.loading ? "Loading…" : t.artist}
        </div>
      </div>
      {#if libTrack}
        <button class="icon like" class:on={libTrack.liked} onclick={() => store.toggleLike(libTrack.id)} aria-label="Like">
          <Icon name="heart" size={17} fill={libTrack.liked} />
        </button>
      {/if}
    {:else}
      <div class="idle">
        <span class="serif">Nothing playing</span>
        <span>Pick something from your library or the Billboard.</span>
      </div>
    {/if}
  </div>

  <div class="center">
    <div class="controls">
      <button class="icon" class:on={p.shuffle} onclick={() => api.setShuffle(!p.shuffle)} aria-label="Shuffle">
        <Icon name="shuffle" size={17} />
      </button>
      <button class="icon" onclick={() => api.control("previous")} aria-label="Previous"><Icon name="prev" size={20} /></button>
      <button class="playpause" onclick={() => api.control("toggle")} disabled={!t} aria-label={p.playing ? "Pause" : "Play"}>
        {#if p.loading}<Spinner size={18} />{:else}<Icon name={p.playing ? "pause" : "play"} size={20} />{/if}
      </button>
      <button class="icon" onclick={() => api.control("next")} aria-label="Next"><Icon name="next" size={20} /></button>
      <button class="icon repeat" class:on={p.repeat !== "off"} onclick={() => api.setRepeat(repeatNext[p.repeat])} aria-label="Repeat">
        <Icon name="repeat" size={17} />
        {#if p.repeat === "one"}<span class="one">1</span>{/if}
      </button>
    </div>
    <div class="progress">
      <span class="num time">{fmtTime(scrub ?? store.position)}</span>
      <Slider
        value={store.position}
        max={p.duration}
        label="Seek"
        oninput={(v) => (scrub = v)}
        oncommit={(v) => {
          scrub = null;
          store.seek(v);
        }}
      />
      <span class="num time">{fmtTime(p.duration)}</span>
    </div>
  </div>

  <div class="right">
    {#if p.stream}
      <span class="badge" class:hires class:lossless={p.stream.lossless && !hires} title="Output {p.stream.outputRate / 1000} kHz">
        {hires ? "Hi-Res · " : ""}{streamLabel(p.stream)}
      </span>
    {/if}
    {#if isPreview}<span class="badge">Preview</span>{/if}
    {#if fromYouTube}
      <span class="badge">YouTube</span>
      <button class="icon" onclick={() => downloads.downloadCurrent()} aria-label="Download to library" title="Download to library">
        <Icon name="download" size={18} />
      </button>
    {/if}
    {#if t}
      <button class="icon" onclick={() => store.openVideo({ title: t.title, artist: t.artist })} aria-label="See video" title="See video">
        <Icon name="video" size={18} />
      </button>
    {/if}
    <button class="icon sleep" class:on={store.sleep.active} onclick={(e) => store.sleepMenu(e)} aria-label="Sleep timer" title={store.sleepLabel ? `Sleep timer: ${store.sleepLabel}` : "Sleep timer"}>
      <Icon name="moon" size={18} />
      {#if store.sleepLabel}<span class="sleep-left">{store.sleepLabel}</span>{/if}
    </button>
    <button class="icon" class:on={store.panel === "lyrics"} onclick={() => (store.panel = store.panel === "lyrics" ? "none" : "lyrics")} aria-label="Lyrics">
      <Icon name="lyrics" size={18} />
    </button>
    <button class="icon" class:on={store.panel === "queue"} onclick={() => (store.panel = store.panel === "queue" ? "none" : "queue")} aria-label="Queue">
      <Icon name="queue" size={18} />
    </button>
    <div class="volume">
      <button class="icon" onclick={toggleMute} aria-label="Mute">
        <Icon name={p.volume === 0 ? "mute" : p.volume < 0.5 ? "volume-low" : "volume"} size={18} />
      </button>
      <Slider value={p.volume} max={1} label="Volume" oninput={(v) => store.setVolume(v)} oncommit={(v) => store.setVolume(v)} />
    </div>
  </div>

  {#if p.error}
    <div class="error" title={p.error}>{p.error}</div>
  {/if}
</footer>

<style>
  footer {
    position: relative;
    height: var(--player-h);
    display: grid;
    grid-template-columns: minmax(220px, 1fr) minmax(360px, 1.4fr) minmax(220px, 1fr);
    align-items: center;
    gap: 20px;
    padding: 0 18px;
    background: rgba(19, 17, 24, 0.92);
    backdrop-filter: blur(20px);
    -webkit-backdrop-filter: blur(20px);
    border-top: 1px solid var(--line);
    z-index: 20;
  }
  .left {
    display: flex;
    align-items: center;
    gap: 14px;
    min-width: 0;
  }
  .art {
    position: relative;
    border-radius: 8px;
    overflow: hidden;
    box-shadow: 0 6px 18px rgba(0, 0, 0, 0.4);
  }
  .expand {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    background: rgba(0, 0, 0, 0.45);
    opacity: 0;
    transition: opacity 0.15s;
  }
  .art:hover .expand {
    opacity: 1;
  }
  .meta {
    min-width: 0;
  }
  .title {
    font-weight: 600;
  }
  .artist {
    color: var(--muted);
    font-size: 13px;
  }
  .idle {
    display: flex;
    flex-direction: column;
    color: var(--dim);
    font-size: 12.5px;
  }
  .idle .serif {
    font-size: 17px;
    color: var(--muted);
  }
  .center {
    display: flex;
    flex-direction: column;
    gap: 4px;
    align-items: center;
  }
  .controls {
    display: flex;
    align-items: center;
    gap: 18px;
  }
  .icon {
    position: relative;
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    border-radius: 8px;
    color: var(--muted);
    transition: color 0.12s;
  }
  .icon:hover {
    color: var(--text);
  }
  .icon.on {
    color: var(--coral);
  }
  .like.on {
    color: var(--coral);
  }
  .one {
    position: absolute;
    font-size: 8.5px;
    font-weight: 800;
    top: 3px;
    right: 2px;
  }
  .playpause {
    width: 40px;
    height: 40px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    background: var(--text);
    color: #111;
    transition: transform 0.1s;
  }
  .playpause:hover {
    transform: scale(1.06);
  }
  .playpause:disabled {
    opacity: 0.4;
    transform: none;
  }
  .progress {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    max-width: 620px;
  }
  .time {
    width: 44px;
    color: var(--dim);
    text-align: center;
  }
  .right {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 6px;
  }
  .right .badge {
    margin-right: 6px;
  }
  .volume {
    display: flex;
    align-items: center;
    width: 140px;
    gap: 4px;
  }
  .error {
    position: absolute;
    top: -30px;
    left: 50%;
    transform: translateX(-50%);
    max-width: 60%;
    padding: 4px 12px;
    border-radius: 999px;
    background: #4a1f25;
    color: #ffb3b3;
    font-size: 12px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .sleep {
    width: auto;
    gap: 4px;
    padding: 0 6px;
    display: inline-flex;
    align-items: center;
  }
  .sleep-left {
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }
</style>
