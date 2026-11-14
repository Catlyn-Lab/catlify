<script lang="ts">
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { onMount } from "svelte";
  import { api, ytBridge } from "./lib/api";
  import { store } from "./lib/store.svelte";
  import { toQueueItem } from "./lib/util";
  import Icon from "./components/Icon.svelte";
  import LyricsView from "./components/LyricsView.svelte";
  import NowPlaying from "./components/NowPlaying.svelte";
  import Overlays from "./components/Overlays.svelte";
  import VideoPlayer from "./components/VideoPlayer.svelte";
  import UpdateDialog from "./components/UpdateDialog.svelte";
  import { updater } from "./lib/updater.svelte";
  import PlayerBar from "./components/PlayerBar.svelte";
  import QueuePanel from "./components/QueuePanel.svelte";
  import Sidebar from "./components/Sidebar.svelte";
  import MiniPlayer from "./components/MiniPlayer.svelte";
  import TabBar from "./components/TabBar.svelte";
  import Library from "./views/Library.svelte";
  import Sync from "./views/Sync.svelte";
  import Import from "./views/Import.svelte";
  import { importer } from "./lib/importer.svelte";
  import AlbumDetail from "./views/AlbumDetail.svelte";
  import Albums from "./views/Albums.svelte";
  import ArtistDetail from "./views/ArtistDetail.svelte";
  import Artists from "./views/Artists.svelte";
  import Billboard from "./views/Billboard.svelte";
  import YouTube from "./views/YouTube.svelte";
  import Home from "./views/Home.svelte";
  import PlaylistView from "./views/PlaylistView.svelte";
  import Search from "./views/Search.svelte";
  import Settings from "./views/Settings.svelte";
  import Songs from "./views/Songs.svelte";

  let sidebar: Sidebar | undefined = $state();
  let dropping = $state(false);
  let extensions = new Set<string>();

  onMount(() => {
    // Unfinished imports continue once the library is loaded (to skip songs already there).
    store.init().then(() => importer.kick());
    updater.start();
    // Android: a YouTube link shared to Catlify (StafyYouTube.kt) opens the YouTube page.
    const w = window as unknown as { __stafyShare?: (url: string) => void };
    w.__stafyShare = (url) => store.go({ name: "youtube", query: url });
    const shared = ytBridge()?.takeShare();
    if (shared) w.__stafyShare(shared);
    api.audioExtensions().then((e) => (extensions = new Set(e)));
    const unlisten = getCurrentWebview().onDragDropEvent(async (e) => {
      if (e.payload.type === "over" || e.payload.type === "enter") dropping = true;
      else if (e.payload.type === "leave") dropping = false;
      else if (e.payload.type === "drop") {
        dropping = false;
        const paths = e.payload.paths.filter((p) => extensions.has(p.split(".").pop()?.toLowerCase() ?? ""));
        if (!paths.length) return store.notify("Those files aren't audio Catlify recognises");
        const tracks = await api.probeFiles(paths);
        store.playItems(tracks.map((t) => ({ ...toQueueItem(t), id: null })));
      }
    });
    return () => {
      unlisten.then((f) => f());
    };
  });

  function onKey(e: KeyboardEvent) {
    const target = e.target as HTMLElement;
    if (target.closest("input, select, textarea")) return;
    const mod = e.metaKey || e.ctrlKey;
    const p = store.player;
    switch (true) {
      case e.code === "Space":
        e.preventDefault();
        api.control("toggle");
        break;
      case mod && e.key === "ArrowRight":
        api.control("next");
        break;
      case mod && e.key === "ArrowLeft":
        api.control("previous");
        break;
      case mod && e.key === "ArrowUp":
        e.preventDefault();
        store.setVolume(p.volume + 0.05);
        break;
      case mod && e.key === "ArrowDown":
        e.preventDefault();
        store.setVolume(p.volume - 0.05);
        break;
      case !mod && e.key === "ArrowRight":
        store.seek(Math.min(p.duration, store.position + 5));
        break;
      case !mod && e.key === "ArrowLeft":
        store.seek(Math.max(0, store.position - 5));
        break;
      case mod && e.key.toLowerCase() === "f":
        e.preventDefault();
        sidebar?.focusSearch();
        break;
      case !mod && e.key.toLowerCase() === "l":
        store.panel = store.panel === "lyrics" ? "none" : "lyrics";
        break;
      case !mod && e.key.toLowerCase() === "q":
        store.panel = store.panel === "queue" ? "none" : "queue";
        break;
      case mod && e.key === "[":
        store.back();
        break;
    }
  }

  const v = $derived(store.view);
  // Phone styles key off html.m (app.css and components).
  $effect(() => {
    document.documentElement.classList.toggle("m", store.mobile);
  });
</script>

<svelte:window onkeydown={onKey} />

{#snippet page()}
      {#key v}
        {#if v.name === "home"}<Home />
        {:else if v.name === "billboard"}<Billboard />
        {:else if v.name === "youtube"}{#key v.query}<YouTube query={v.query} />{/key}
        {:else if v.name === "songs"}<Songs />
        {:else if v.name === "liked"}<Songs liked />
        {:else if v.name === "albums"}<Albums />
        {:else if v.name === "album"}<AlbumDetail key={v.key} />
        {:else if v.name === "artists"}<Artists />
        {:else if v.name === "artist"}<ArtistDetail artist={v.artist} />
        {:else if v.name === "playlist"}<PlaylistView id={v.id} />
        {:else if v.name === "search"}<Search query={v.query} />
        {:else if v.name === "settings"}<Settings />
        {:else if v.name === "library"}<Library />
        {:else if v.name === "sync"}<Sync />
        {:else if v.name === "import"}<Import />
        {/if}
      {/key}
{/snippet}

{#snippet sidePanel()}
  {#if store.panel === "queue"}
    <QueuePanel />
  {:else}
    <div class="lyrics-head">
      <h3 class="serif">Lyrics</h3>
    </div>
    <div class="lyrics-body"><LyricsView /></div>
  {/if}
{/snippet}

{#if store.mobile}
  <!-- Phones: content, mini player and bottom tabs; Now Playing and panels are full-screen sheets. -->
  <div class="mshell">
    {#if store.history.length}
      <button class="mback" onclick={() => history.back()} aria-label="Back"><Icon name="back" size={20} /></button>
    {/if}
    <main class="mmain">
      {@render page()}
    </main>
    <MiniPlayer />
    <TabBar />
  </div>
  {#if store.nowPlayingOpen}<NowPlaying />{/if}
  {#if store.panel !== "none"}
    <div class="msheet">
      <div class="msheet-head">
        <button onclick={() => history.back()} aria-label="Close"><Icon name="down" size={22} /></button>
      </div>
      <div class="msheet-body">{@render sidePanel()}</div>
    </div>
  {/if}
{:else}
<div class="shell" class:with-panel={store.panel !== "none"}>
  <Sidebar bind:this={sidebar} />

  <div class="stage">
    <div class="topbar" data-tauri-drag-region>
      {#if store.history.length}
        <button class="nav" onclick={() => store.back()} aria-label="Back"><Icon name="back" size={18} /></button>
      {/if}
    </div>
    <main>
      {@render page()}
    </main>
    {#if store.nowPlayingOpen}<NowPlaying />{/if}
  </div>

  {#if store.panel !== "none"}
    <aside class="panel">
      <div class="drag" data-tauri-drag-region></div>
      {@render sidePanel()}
    </aside>
  {/if}

  <div class="player"><PlayerBar /></div>
</div>
{/if}

{#if dropping}
  <div class="drop">
    <div class="serif">Drop to play</div>
  </div>
{/if}

{#if store.video}{#key store.video}<VideoPlayer />{/key}{/if}
<Overlays />
<UpdateDialog />

<style>
  .shell {
    height: 100%;
    /* Tablets: keep clear of camera cutouts and system bars (all 0 on desktop). */
    padding: var(--safe-top) var(--safe-right) var(--safe-bottom) var(--safe-left);
    display: grid;
    grid-template-columns: var(--sidebar-w) 1fr;
    grid-template-rows: 1fr var(--player-h);
    grid-template-areas:
      "side stage"
      "player player";
  }
  .shell.with-panel {
    grid-template-columns: var(--sidebar-w) 1fr var(--panel-w);
    grid-template-areas:
      "side stage panel"
      "player player player";
  }
  .shell > :global(aside:first-child) {
    grid-area: side;
    min-height: 0;
  }
  .stage {
    grid-area: stage;
    position: relative;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .topbar {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    height: 52px;
    z-index: 10;
    display: flex;
    align-items: center;
    padding: 12px 24px 0;
  }
  .nav {
    width: 30px;
    height: 30px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    background: rgba(0, 0, 0, 0.45);
    color: var(--muted);
  }
  .nav:hover {
    color: var(--text);
  }
  main {
    flex: 1;
    overflow-y: auto;
    padding-top: 28px;
    background:
      radial-gradient(1200px 400px at 10% -10%, rgba(157, 134, 255, 0.09), transparent 60%),
      radial-gradient(900px 400px at 100% 0%, rgba(255, 123, 103, 0.06), transparent 60%);
  }
  .panel {
    grid-area: panel;
    min-height: 0;
    display: flex;
    flex-direction: column;
    background: var(--bg-raised);
    border-left: 1px solid var(--line);
  }
  .panel .drag {
    height: 22px;
    flex-shrink: 0;
  }
  .lyrics-head h3 {
    font-size: 24px;
    margin: 4px 22px 8px;
  }
  .lyrics-body {
    flex: 1;
    min-height: 0;
  }
  .player {
    grid-area: player;
  }
  /* ---- phone layout ---- */
  .mshell {
    position: relative;
    height: 100%;
    display: flex;
    flex-direction: column;
    padding-left: var(--safe-left);
    padding-right: var(--safe-right);
  }
  .mmain {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    overscroll-behavior: contain;
    -webkit-overflow-scrolling: touch;
    padding-top: var(--safe-top);
  }
  .mback {
    position: absolute;
    z-index: 12;
    top: calc(var(--safe-top) + 8px);
    left: calc(var(--safe-left) + 10px);
    width: 36px;
    height: 36px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    background: rgba(30, 27, 37, 0.85);
    backdrop-filter: blur(12px);
    -webkit-backdrop-filter: blur(12px);
  }
  .msheet {
    position: fixed;
    inset: 0;
    z-index: 40;
    display: flex;
    flex-direction: column;
    background: var(--bg-raised);
    padding: var(--safe-top) var(--safe-right) var(--safe-bottom) var(--safe-left);
    animation: sheet 0.3s cubic-bezier(0.2, 0.8, 0.2, 1) both;
  }
  @keyframes sheet {
    from {
      transform: translateY(40px);
      opacity: 0;
    }
  }
  .msheet-head {
    display: flex;
    justify-content: center;
    padding: 6px 0 2px;
  }
  .msheet-head button {
    width: 44px;
    height: 36px;
    display: grid;
    place-items: center;
    color: var(--muted);
  }
  .msheet-body {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .drop {
    position: fixed;
    inset: 12px;
    z-index: 200;
    border-radius: 20px;
    border: 2px dashed var(--violet);
    background: rgba(12, 11, 15, 0.8);
    display: grid;
    place-items: center;
    font-size: 40px;
    pointer-events: none;
  }
</style>
