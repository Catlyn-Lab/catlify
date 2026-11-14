<script lang="ts">
  import { api } from "../lib/api";
  import { store } from "../lib/store.svelte";
  import Cover from "./Cover.svelte";
  import Icon from "./Icon.svelte";
  import Spinner from "./Spinner.svelte";

  const p = $derived(store.player);
  const t = $derived(p.track);
  const progress = $derived(p.duration ? Math.min(store.position / p.duration, 1) : 0);
</script>

{#if t}
  <div class="mini">
    <span class="bar"><span style:width="{progress * 100}%"></span></span>
    <button class="open" onclick={() => store.openNowPlaying()} aria-label="Now playing">
      <Cover src={t.cover} size="44px" radius="8px" seed={t.album} />
      <span class="tt">
        <span class="ellipsis title">{t.title}</span>
        <span class="ellipsis artist">{p.loading ? "Loading…" : t.artist}</span>
      </span>
    </button>
    <button class="icon" onclick={() => api.control("toggle")} aria-label={p.playing ? "Pause" : "Play"}>
      {#if p.loading}<Spinner size={20} />{:else}<Icon name={p.playing ? "pause" : "play"} size={24} />{/if}
    </button>
    <button class="icon" onclick={() => api.control("next")} aria-label="Next"><Icon name="next" size={22} /></button>
  </div>
{/if}

<style>
  .mini {
    position: relative;
    height: var(--mini-h);
    margin: 0 8px 6px;
    padding: 0 6px 0 9px;
    display: flex;
    align-items: center;
    gap: 4px;
    border-radius: 14px;
    background: rgba(34, 31, 42, 0.94);
    backdrop-filter: blur(20px);
    -webkit-backdrop-filter: blur(20px);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
    overflow: hidden;
  }
  .bar {
    position: absolute;
    left: 12px;
    right: 12px;
    bottom: 0;
    height: 2px;
    background: rgba(255, 255, 255, 0.1);
  }
  .bar span {
    display: block;
    height: 100%;
    background: var(--accent);
  }
  .open {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 11px;
    text-align: left;
    height: 100%;
  }
  .tt {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .title {
    font-weight: 600;
    font-size: 14px;
  }
  .artist {
    color: var(--muted);
    font-size: 12.5px;
  }
  .icon {
    width: 44px;
    height: 44px;
    display: grid;
    place-items: center;
    flex: none;
  }
</style>
