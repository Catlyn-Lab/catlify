<script lang="ts">
  import { api, type QueueItem } from "../lib/api";
  import { store } from "../lib/store.svelte";
  import { fmtTime } from "../lib/util";
  import { downloads } from "../lib/downloads.svelte";
  import { isYouTubePath } from "../lib/yt";
  import Cover from "./Cover.svelte";
  import Icon from "./Icon.svelte";

  let entries = $state<{ index: number; item: QueueItem }[]>([]);

  $effect(() => {
    store.player.queueVersion;
    store.player.index;
    api.queue().then((q) => (entries = q));
  });

  const currentPos = $derived(entries.findIndex((e) => e.index === store.player.index));
  const upNext = $derived(currentPos >= 0 ? entries.slice(currentPos + 1) : entries);
  const remaining = $derived(upNext.reduce((s, e) => s + (e.item.duration || 0), 0));
</script>

<div class="queue">
  <h3 class="serif">Queue</h3>
  {#if store.player.track}
    <div class="eyebrow">Now playing</div>
    <div class="item current">
      <Cover src={store.player.track.cover} size="44px" radius="6px" seed={store.player.track.album} />
      <div class="meta">
        <div class="ellipsis t">{store.player.track.title}</div>
        <div class="ellipsis a">{store.player.track.artist}</div>
      </div>
    </div>
  {/if}

  <div class="eyebrow next">
    Up next {#if upNext.length}<span>· {upNext.length} songs · {fmtTime(remaining)}</span>{/if}
  </div>
  <div class="list">
    {#each upNext as e (e.index + ":" + e.item.path)}
      <div class="item" role="button" tabindex="0" ondblclick={() => api.jump(e.index)} onkeydown={(k) => k.key === "Enter" && api.jump(e.index)}>
        <Cover src={e.item.cover} size="40px" radius="6px" seed={e.item.album} />
        <div class="meta">
          <div class="ellipsis t">{e.item.title}</div>
          <div class="ellipsis a">{e.item.artist}</div>
        </div>
        <span class="num d">{fmtTime(e.item.duration)}</span>
        {#if isYouTubePath(e.item.path)}
          <button class="rm" onclick={() => downloads.downloadItem(e.item)} aria-label="Download to library" title="Download to library">
            <Icon name="download" size={14} />
          </button>
        {/if}
        <button class="rm" onclick={() => api.removeFromQueue(e.index)} aria-label="Remove"><Icon name="x" size={14} /></button>
      </div>
    {:else}
      <p class="empty-q">Nothing queued. Use “Play next” or “Add to queue” on any song.</p>
    {/each}
  </div>
</div>

<style>
  .queue {
    height: 100%;
    display: flex;
    flex-direction: column;
    padding: 18px 14px 0;
    min-height: 0;
  }
  h3 {
    font-size: 24px;
    margin: 4px 8px 16px;
  }
  .eyebrow {
    padding: 0 8px 8px;
  }
  .eyebrow span {
    text-transform: none;
    letter-spacing: 0;
    font-weight: 400;
  }
  .next {
    margin-top: 18px;
  }
  .list {
    overflow-y: auto;
    flex: 1;
    min-height: 0;
    padding-bottom: 20px;
  }
  .item {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 6px 8px;
    border-radius: 8px;
  }
  .item:hover {
    background: var(--surface);
  }
  .current .t {
    color: var(--coral);
  }
  .meta {
    flex: 1;
    min-width: 0;
  }
  .t {
    font-weight: 500;
    font-size: 13.5px;
  }
  .a {
    color: var(--muted);
    font-size: 12.5px;
  }
  .d {
    color: var(--dim);
  }
  .rm {
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    color: var(--dim);
    opacity: 0;
  }
  .item:hover .rm {
    opacity: 1;
  }
  /* Touch screens can't hover: always show the row buttons. */
  :global(html.m) .rm {
    opacity: 1;
    width: 36px;
    height: 36px;
  }
  .rm:hover {
    color: var(--text);
  }
  .empty-q {
    color: var(--dim);
    font-size: 13px;
    padding: 8px;
  }
</style>
