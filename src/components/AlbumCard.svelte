<script lang="ts">
  import { store, type Album } from "../lib/store.svelte";
  import Cover from "./Cover.svelte";
  import Icon from "./Icon.svelte";

  let { album }: { album: Album } = $props();
</script>

<div class="card">
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="art" onclick={() => store.go({ name: "album", key: album.key })} oncontextmenu={(e) => store.openMenu(e, store.trackMenu(album.tracks))}>
    <Cover src={album.cover} seed={album.title} />
    <button class="play" onclick={(e) => (e.stopPropagation(), store.playTracks(album.tracks))} aria-label="Play {album.title}">
      <Icon name="play" size={18} />
    </button>
    {#if album.hiRes}<span class="badge hires tag">Hi-Res</span>{/if}
  </div>
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="t ellipsis link" onclick={() => store.go({ name: "album", key: album.key })}>{album.title}</div>
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="a ellipsis link" onclick={() => store.go({ name: "artist", artist: album.artist })}>{album.artist}</div>
</div>

<style>
  .card {
    min-width: 0;
  }
  .art {
    position: relative;
    cursor: pointer;
    border-radius: var(--radius);
    box-shadow: 0 10px 24px rgba(0, 0, 0, 0.35);
    transition: transform 0.2s;
  }
  .art:hover {
    transform: translateY(-3px);
  }
  .play {
    position: absolute;
    right: 10px;
    bottom: 10px;
    width: 42px;
    height: 42px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    background: var(--text);
    color: #111;
    opacity: 0;
    transform: translateY(6px);
    transition: all 0.18s;
    box-shadow: 0 8px 20px rgba(0, 0, 0, 0.4);
  }
  .art:hover .play {
    opacity: 1;
    transform: none;
  }
  .tag {
    position: absolute;
    left: 8px;
    top: 8px;
    background: rgba(0, 0, 0, 0.55);
    backdrop-filter: blur(6px);
  }
  .t {
    margin-top: 10px;
    font-weight: 600;
  }
  .a {
    color: var(--muted);
    font-size: 13px;
  }
</style>
