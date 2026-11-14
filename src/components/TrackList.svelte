<script lang="ts">
  import type { Track } from "../lib/api";
  import { store, type MenuItem } from "../lib/store.svelte";
  import { fmtTime, isHiRes, qualityLabel, albumKey } from "../lib/util";
  import Cover from "./Cover.svelte";
  import Icon from "./Icon.svelte";

  let {
    tracks,
    showAlbum = true,
    showCover = true,
    numbered = false,
    extraMenu,
    onplay,
  }: {
    tracks: Track[];
    showAlbum?: boolean;
    showCover?: boolean;
    numbered?: boolean;
    extraMenu?: (t: Track, index: number) => MenuItem[];
    onplay?: (index: number) => void;
  } = $props();

  // Phones: taller rows (thumb-sized) and tap-to-play.
  const ROW = $derived(store.mobile ? 62 : 54);
  const OVERSCAN = 8;
  let listEl: HTMLDivElement | undefined = $state();
  let start = $state(0);
  let count = $state(40);

  function measure() {
    const main = listEl?.closest("main");
    if (!listEl || !main) return;
    const top = listEl.getBoundingClientRect().top - main.getBoundingClientRect().top;
    start = Math.max(0, Math.floor(-top / ROW) - OVERSCAN);
    count = Math.ceil(main.clientHeight / ROW) + OVERSCAN * 2;
  }

  $effect(() => {
    const main = listEl?.closest("main");
    if (!main) return;
    measure();
    main.addEventListener("scroll", measure, { passive: true });
    window.addEventListener("resize", measure);
    return () => {
      main.removeEventListener("scroll", measure);
      window.removeEventListener("resize", measure);
    };
  });

  const visible = $derived(tracks.slice(start, start + count));
  const currentId = $derived(store.player.track?.id ?? null);

  function play(i: number) {
    if (onplay) onplay(i);
    else store.playTracks(tracks, i);
  }
</script>

{#if !store.mobile}
<div class="head" class:no-album={!showAlbum}>
  <span class="c-num">#</span>
  <span>Title</span>
  {#if showAlbum}<span>Album</span>{/if}
  <span>Quality</span>
  <span></span>
  <span class="c-time">Time</span>
  <span></span>
</div>
{/if}

<div class="list" class:m={store.mobile} bind:this={listEl} style:height="{tracks.length * ROW}px">
  <div style:transform="translateY({start * ROW}px)">
    {#each visible as t, vi (t.id + ":" + (start + vi))}
      {@const i = start + vi}
      {@const active = t.id === currentId}
      <div
        class="row"
        class:no-album={!showAlbum}
        class:active
        role="button"
        tabindex="0"
        ondblclick={() => play(i)}
        onclick={(e) => store.mobile && !(e.target as HTMLElement).closest("button, .link") && play(i)}
        onkeydown={(e) => e.key === "Enter" && play(i)}
        oncontextmenu={(e) => store.openMenu(e, store.trackMenu([t], extraMenu?.(t, i)))}
      >
        <span class="c-num">
          <span class="n num">
            {#if active && store.player.playing}
              <span class="eq"><i></i><i></i><i></i></span>
            {:else}
              {numbered ? (t.trackNo ?? i + 1) : i + 1}
            {/if}
          </span>
          <button class="playbtn" onclick={() => play(i)} aria-label="Play {t.title}"><Icon name="play" size={14} /></button>
        </span>
        <span class="title-cell">
          {#if showCover}<Cover src={t.cover} size="38px" radius="6px" seed={t.album} />{/if}
          <span class="tt">
            <span class="ellipsis title">{t.title}</span>
            <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
            <span class="ellipsis artist link" onclick={() => store.go({ name: "artist", artist: t.albumArtist })}>{t.artist}</span>
          </span>
        </span>
        {#if showAlbum}
          <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
          <span class="ellipsis album link" onclick={() => store.go({ name: "album", key: albumKey(t) })}>{t.album}</span>
        {/if}
        <span class="q">
          <span class="badge" class:hires={isHiRes(t.bitDepth, t.sampleRate, t.lossless)} class:lossless={t.lossless && !isHiRes(t.bitDepth, t.sampleRate, t.lossless)}>
            {qualityLabel(t.codec, t.lossless ? t.bitDepth : null, t.sampleRate)}
          </span>
        </span>
        <button class="like" class:on={t.liked} onclick={() => store.toggleLike(t.id)} aria-label="Like">
          <Icon name="heart" size={15} fill={t.liked} />
        </button>
        <span class="c-time num">{fmtTime(t.duration)}</span>
        <button class="more" onclick={(e) => store.openMenu(e, store.trackMenu([t], extraMenu?.(t, i)))} aria-label="More">
          <Icon name="more" size={18} />
        </button>
      </div>
    {/each}
  </div>
</div>

<style>
  .head,
  .row {
    display: grid;
    grid-template-columns: 44px minmax(200px, 3fr) minmax(120px, 2fr) 116px 30px 56px 32px;
    align-items: center;
    gap: 12px;
    padding: 0 12px;
  }
  .no-album {
    grid-template-columns: 44px minmax(200px, 1fr) 116px 30px 56px 32px;
  }
  .head {
    height: 34px;
    font-size: 11px;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--dim);
    border-bottom: 1px solid var(--line);
    margin-bottom: 6px;
  }
  .list {
    position: relative;
    contain: strict;
    width: 100%;
  }
  .row {
    height: 54px;
    border-radius: 8px;
    cursor: default;
  }
  .row:hover,
  .row:focus-visible {
    background: var(--surface);
    outline: none;
  }
  .row.active .title {
    color: var(--coral);
  }
  .c-num {
    text-align: center;
    color: var(--dim);
    position: relative;
  }
  .c-time {
    text-align: right;
    color: var(--muted);
  }
  .playbtn {
    display: none;
    position: absolute;
    inset: -8px 0;
    place-items: center;
    color: var(--text);
  }
  .row:hover .playbtn {
    display: grid;
  }
  .row:hover .n {
    visibility: hidden;
  }
  .title-cell {
    display: flex;
    align-items: center;
    gap: 12px;
    min-width: 0;
  }
  .tt {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .title {
    font-weight: 500;
  }
  .artist,
  .album {
    color: var(--muted);
    font-size: 13px;
  }
  .artist:hover,
  .album:hover {
    color: var(--text);
  }
  .like,
  .more {
    color: var(--dim);
    opacity: 0;
    display: grid;
    place-items: center;
    transition: opacity 0.1s, color 0.1s;
  }
  .row:hover .like,
  .row:hover .more,
  .like.on {
    opacity: 1;
  }
  .like.on {
    color: var(--coral);
  }
  .like:hover,
  .more:hover {
    color: var(--text);
  }
  /* ---- phone: [cover + title/artist] [more] ---- */
  .m .row {
    grid-template-columns: minmax(0, 1fr) 40px;
    height: 62px;
    gap: 6px;
    padding: 0 0 0 2px;
  }
  .m .row:hover {
    background: none;
  }
  .m .row:active {
    background: var(--surface);
  }
  .m .c-num,
  .m .album,
  .m .q,
  .m .like:not(.on),
  .m .c-time {
    display: none;
  }
  .m .like.on {
    display: none;
  }
  .m .more {
    opacity: 1;
    width: 40px;
    height: 44px;
  }
  .m .title-cell :global(.cover) {
    width: 46px !important;
    flex: none;
  }
  .m .title {
    font-size: 15px;
  }
  .m .artist {
    pointer-events: none;
  }
  .eq {
    display: inline-flex;
    gap: 2px;
    height: 12px;
    align-items: flex-end;
  }
  .eq i {
    width: 3px;
    background: var(--coral);
    border-radius: 1px;
    animation: eq 0.9s ease-in-out infinite;
  }
  .eq i:nth-child(2) {
    animation-delay: -0.3s;
  }
  .eq i:nth-child(3) {
    animation-delay: -0.6s;
  }
  @keyframes eq {
    0%,
    100% {
      height: 3px;
    }
    50% {
      height: 12px;
    }
  }
</style>
