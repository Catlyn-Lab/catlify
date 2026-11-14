<script lang="ts">
  import { api } from "../lib/api";
  import { store } from "../lib/store.svelte";
  import { coverSrc, fmtTime, isHiRes, streamLabel, albumKey } from "../lib/util";
  import Slider from "./Slider.svelte";
  import Spinner from "./Spinner.svelte";
  import { downloads } from "../lib/downloads.svelte";
  import { isYouTubePath } from "../lib/yt";
  import Cover from "./Cover.svelte";
  import Icon from "./Icon.svelte";
  import LyricsView from "./LyricsView.svelte";

  const t = $derived(store.player.track);
  const s = $derived(store.player.stream);
  const lib = $derived(store.currentTrack);
  const bg = $derived(coverSrc(t?.cover));

  // Phone-only state
  const p = $derived(store.player);
  let scrub = $state<number | null>(null);
  let showLyrics = $state(false);
  const repeatNext = { off: "all", all: "one", one: "off" } as const;
  const close = () => (store.mobile ? history.back() : (store.nowPlayingOpen = false));
  function goTo(view: Parameters<typeof store.go>[0]) {
    store.nowPlayingOpen = false;
    store.go(view);
  }
</script>

{#if store.mobile}
  <section class="np m">
    {#if bg}<div class="backdrop" style:background-image="url({bg})"></div>{/if}
    <div class="shade"></div>
    <div class="mwrap">
      <header>
        <button class="round" onclick={close} aria-label="Close"><Icon name="down" size={22} /></button>
        <span class="eyebrow">{p.loading ? "Loading…" : p.playing ? "Now Playing" : "Paused"}</span>
        <button class="round" onclick={() => store.togglePanel("queue")} aria-label="Queue"><Icon name="queue" size={20} /></button>
      </header>

      {#if t}
        <div class="stage">
          {#if showLyrics}
            <div class="mlyrics"><LyricsView large /></div>
          {:else}
            <button class="martwrap" onclick={() => (showLyrics = true)} aria-label="Show lyrics">
              <div class="mart">
                <Cover src={t.cover} radius="16px" seed={t.album} />
                {#if p.loading}<span class="mload"><Spinner size={34} color="#fff" /><span>Loading…</span></span>{/if}
              </div>
            </button>
          {/if}
        </div>

        <div class="minfo">
          <div class="mtext">
            <h1 class="ellipsis">{t.title}</h1>
            <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
            <p class="ellipsis" onclick={() => lib && goTo({ name: "artist", artist: lib.albumArtist })}>
              {t.artist}{t.album ? ` · ${t.album}` : ""}
            </p>
          </div>
          {#if lib}
            <button class="like" class:on={lib.liked} onclick={() => store.toggleLike(lib.id)} aria-label="Like">
              <Icon name="heart" size={24} fill={lib.liked} />
            </button>
          {/if}
        </div>

        <div class="mseek">
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
          <div class="times num">
            <span>{fmtTime(scrub ?? store.position)}</span>
            {#if s}
              <span class="badge" class:hires={isHiRes(s.bitDepth, s.sampleRate, s.lossless)} class:lossless={s.lossless}>
                {streamLabel(s)}
              </span>
            {/if}
            <span>-{fmtTime(Math.max(p.duration - (scrub ?? store.position), 0))}</span>
          </div>
        </div>

        <div class="mcontrols">
          <button class="c" class:on={p.shuffle} onclick={() => api.setShuffle(!p.shuffle)} aria-label="Shuffle">
            <Icon name="shuffle" size={20} />
          </button>
          <button class="c big" onclick={() => api.control("previous")} aria-label="Previous"><Icon name="prev" size={30} /></button>
          <button class="play" onclick={() => api.control("toggle")} aria-label={p.playing ? "Pause" : "Play"}>
            {#if p.loading}<Spinner size={28} />{:else}<Icon name={p.playing ? "pause" : "play"} size={30} />{/if}
          </button>
          <button class="c big" onclick={() => api.control("next")} aria-label="Next"><Icon name="next" size={30} /></button>
          <button class="c" class:on={p.repeat !== "off"} onclick={() => api.setRepeat(repeatNext[p.repeat])} aria-label="Repeat">
            <Icon name="repeat" size={20} />
            {#if p.repeat === "one"}<span class="one">1</span>{/if}
          </button>
        </div>

        <div class="mfoot">
          <button class:on={showLyrics} onclick={() => (showLyrics = !showLyrics)}><Icon name="lyrics" size={18} />Lyrics</button>
          {#if lib}
            <button onclick={() => goTo({ name: "album", key: albumKey(lib) })}><Icon name="disc" size={18} />Album</button>
          {/if}
          {#if isYouTubePath(t.path)}
            <button onclick={() => downloads.downloadCurrent()}><Icon name="download" size={18} />Download</button>
          {/if}
          <button onclick={() => store.openVideo({ title: t.title, artist: t.artist })}><Icon name="video" size={18} />Video</button>
          <button class:on={store.sleep.active} onclick={(e) => store.sleepMenu(e)}>
            <Icon name="moon" size={18} />{store.sleepLabel ?? "Sleep"}
          </button>
          <button onclick={() => store.togglePanel("queue")}><Icon name="queue" size={18} />Up next</button>
        </div>
      {/if}
    </div>
  </section>
{:else}
<section class="np">
  {#if bg}<div class="backdrop" style:background-image="url({bg})"></div>{/if}
  <div class="shade"></div>
  <button class="close" onclick={() => (store.nowPlayingOpen = false)} aria-label="Close"><Icon name="down" size={22} /></button>

  {#if t}
    <div class="content">
      <div class="left">
        <div class="art"><Cover src={t.cover} radius="14px" seed={t.album} /></div>
        <h1 class="serif">{t.title}</h1>
        <p class="by">
          <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
          <span class="link" onclick={() => lib && store.go({ name: "artist", artist: lib.albumArtist })}>{t.artist}</span>
          {#if t.album}
            <span class="dot">·</span>
            <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
            <span class="link" onclick={() => lib && store.go({ name: "album", key: albumKey(lib) })}>{t.album}</span>
          {/if}
        </p>
        {#if s}
          <div class="tech">
            {#if isHiRes(s.bitDepth, s.sampleRate, s.lossless)}<span class="badge hires">Hi-Res Lossless</span>
            {:else if s.lossless}<span class="badge lossless">Lossless</span>{/if}
            <span class="num">{streamLabel(s)}</span>
            <span class="num dim">→ out {s.outputRate / 1000} kHz</span>
          </div>
        {/if}
      </div>
      <div class="right"><LyricsView large /></div>
    </div>
  {/if}
</section>
{/if}

<style>
  .np {
    position: absolute;
    inset: 0;
    z-index: 15;
    overflow: hidden;
    background: var(--bg);
    animation: rise 0.35s cubic-bezier(0.2, 0.8, 0.2, 1) both;
  }
  @keyframes rise {
    from {
      transform: translateY(30px);
      opacity: 0;
    }
  }
  .backdrop {
    position: absolute;
    inset: -80px;
    background-size: cover;
    background-position: center;
    filter: blur(70px) saturate(1.5);
    opacity: 0.55;
    transform: scale(1.1);
  }
  .shade {
    position: absolute;
    inset: 0;
    background: radial-gradient(ellipse at 20% 30%, transparent, rgba(12, 11, 15, 0.75) 70%),
      linear-gradient(transparent 40%, rgba(12, 11, 15, 0.8));
  }
  .close {
    position: absolute;
    top: 36px;
    right: 28px;
    z-index: 2;
    width: 38px;
    height: 38px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    background: rgba(255, 255, 255, 0.08);
  }
  .close:hover {
    background: rgba(255, 255, 255, 0.16);
  }
  .content {
    position: relative;
    height: 100%;
    display: grid;
    grid-template-columns: minmax(300px, 0.9fr) 1.1fr;
    gap: 5vw;
    padding: 70px 6vw 30px;
    align-items: center;
  }
  .left {
    display: flex;
    flex-direction: column;
    max-width: 460px;
  }
  .art {
    width: min(100%, 52vh);
    box-shadow: 0 40px 80px rgba(0, 0, 0, 0.55);
    border-radius: 14px;
  }
  h1 {
    font-size: clamp(28px, 3vw, 42px);
    letter-spacing: -0.02em;
    line-height: 1.08;
    margin: 30px 0 8px;
  }
  .by {
    color: var(--muted);
    font-size: 16px;
    margin: 0;
  }
  .dot {
    margin: 0 6px;
    color: var(--dim);
  }
  .tech {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-top: 18px;
    color: var(--muted);
  }
  .dim {
    color: var(--dim);
  }
  .right {
    height: 100%;
    min-height: 0;
  }

  /* ---- phone ---- */
  .np.m {
    position: fixed;
    z-index: 30;
  }
  .mwrap {
    position: relative;
    height: 100%;
    display: flex;
    flex-direction: column;
    padding: calc(var(--safe-top) + 6px) calc(22px + var(--safe-right)) calc(var(--safe-bottom) + 14px)
      calc(22px + var(--safe-left));
  }
  .m header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .round {
    width: 40px;
    height: 40px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    background: rgba(255, 255, 255, 0.08);
  }
  .stage {
    flex: 1;
    min-height: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 18px 0;
  }
  .martwrap {
    width: 100%;
    display: flex;
    justify-content: center;
  }
  .mart {
    width: min(100%, 46vh);
    border-radius: 16px;
    box-shadow: 0 30px 60px rgba(0, 0, 0, 0.55);
    transition: transform 0.35s cubic-bezier(0.2, 0.8, 0.2, 1);
    transform: scale(var(--s, 1));
  }
  .mart {
    position: relative;
  }
  .mload {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    border-radius: 16px;
    background: rgba(0, 0, 0, 0.45);
    color: #fff;
    font-weight: 600;
    font-size: 14px;
  }
  .mlyrics {
    width: 100%;
    height: 100%;
  }
  .minfo {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .mtext {
    flex: 1;
    min-width: 0;
  }
  .minfo h1 {
    font-family: var(--serif);
    font-size: 24px;
    line-height: 1.15;
    margin: 0;
  }
  .minfo p {
    margin: 4px 0 0;
    color: var(--muted);
    font-size: 15px;
  }
  .ellipsis {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .like {
    width: 44px;
    height: 44px;
    display: grid;
    place-items: center;
    color: var(--muted);
  }
  .like.on {
    color: var(--coral);
  }
  .mseek {
    margin-top: 16px;
  }
  .times {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-top: 6px;
    color: var(--dim);
    font-size: 12px;
  }
  .mcontrols {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin: 14px 0 10px;
  }
  .c {
    position: relative;
    width: 48px;
    height: 48px;
    display: grid;
    place-items: center;
    color: var(--muted);
  }
  .c.big {
    color: var(--text);
  }
  .c.on {
    color: var(--coral);
  }
  .one {
    position: absolute;
    bottom: 9px;
    right: 9px;
    font-size: 9px;
    font-weight: 800;
  }
  .play {
    width: 72px;
    height: 72px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    background: var(--text);
    color: #000;
    transition: transform 0.15s ease;
  }
  .play:active {
    transform: scale(0.92);
  }
  .mfoot {
    display: flex;
    justify-content: space-around;
    margin-top: 4px;
  }
  .mfoot button {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    min-width: 0;
    padding: 8px 4px;
    color: var(--muted);
    font-size: 11px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }
  .mfoot button.on {
    color: var(--coral);
  }
</style>
