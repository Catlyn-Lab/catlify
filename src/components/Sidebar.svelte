<script lang="ts">
  import logo from "../../src-tauri/icons/128x128@2x.png";
  import { api } from "../lib/api";
  import { store, type View } from "../lib/store.svelte";
  import Icon from "./Icon.svelte";

  let query = $state("");
  let searchEl: HTMLInputElement | undefined = $state();

  const nav: { label: string; icon: string; view: View }[] = [
    { label: "Listen Now", icon: "home", view: { name: "home" } },
    { label: "Billboard", icon: "chart", view: { name: "billboard" } },
    { label: "YouTube", icon: "download", view: { name: "youtube" } },
    { label: "Import music", icon: "plus", view: { name: "import" } },
  ];
  const lib: { label: string; icon: string; view: View }[] = [
    { label: "Songs", icon: "note", view: { name: "songs" } },
    { label: "Albums", icon: "disc", view: { name: "albums" } },
    { label: "Artists", icon: "mic", view: { name: "artists" } },
    { label: "Liked Songs", icon: "heart", view: { name: "liked" } },
    { label: "Sync devices", icon: "refresh", view: { name: "sync" } },
  ];

  function isActive(v: View) {
    const cur = store.view;
    if (v.name === "albums" && cur.name === "album") return true;
    if (v.name === "artists" && cur.name === "artist") return true;
    return cur.name === v.name && (v.name !== "playlist" || (cur as { id: number }).id === (v as { id: number }).id);
  }

  function search() {
    if (query.trim()) store.go({ name: "search", query: query.trim() });
  }

  async function newPlaylist() {
    const name = await store.askText("New playlist");
    if (!name?.trim()) return;
    const id = await api.createPlaylist(name.trim());
    await store.refreshPlaylists();
    store.go({ name: "playlist", id });
  }

  export function focusSearch() {
    searchEl?.focus();
    searchEl?.select();
  }
</script>

<aside>
  <div class="drag" data-tauri-drag-region></div>
  <div class="brand" data-tauri-drag-region>
    <img class="logo" src={logo} alt="" />
    <span class="word">catlify</span>
  </div>

  <form class="search" onsubmit={(e) => (e.preventDefault(), search())}>
    <Icon name="search" size={15} />
    <input
      bind:this={searchEl}
      bind:value={query}
      oninput={() => (query.trim() ? store.go({ name: "search", query: query.trim() }) : null)}
      placeholder="Search"
      spellcheck="false"
    />
  </form>

  <nav>
    {#each nav as item}
      <button class:active={isActive(item.view)} onclick={() => store.go(item.view)}>
        <Icon name={item.icon} size={17} />{item.label}
      </button>
    {/each}

    <div class="group">Library</div>
    {#each lib as item}
      <button class:active={isActive(item.view)} onclick={() => store.go(item.view)}>
        <Icon name={item.icon} size={17} />{item.label}
      </button>
    {/each}

    <div class="group">
      Playlists
      <button class="add" onclick={newPlaylist} aria-label="New playlist"><Icon name="plus" size={15} /></button>
    </div>
    <div class="playlists">
      {#each store.playlists as p (p.id)}
        <button
          class:active={store.view.name === "playlist" && store.view.id === p.id}
          onclick={() => store.go({ name: "playlist", id: p.id })}
        >
          <Icon name="list" size={16} /><span class="ellipsis">{p.name}</span>
        </button>
      {:else}
        <p class="hint">Right-click any song to add it to a playlist.</p>
      {/each}
    </div>
  </nav>

  <div class="foot">
    {#if store.scan && !store.scan.done}
      <div class="scan">
        <div class="bar"><i style:width="{(store.scan.scanned / Math.max(1, store.scan.total)) * 100}%"></i></div>
        <span>Scanning {store.scan.scanned.toLocaleString()} / {store.scan.total.toLocaleString()}</span>
      </div>
    {/if}
    <button class:active={store.view.name === "settings"} onclick={() => store.go({ name: "settings" })}>
      <Icon name="gear" size={17} />Settings
    </button>
  </div>
</aside>

<style>
  aside {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--bg-raised);
    border-right: 1px solid var(--line);
    padding: 0 12px 12px;
    min-height: 0;
  }
  .drag {
    height: 30px;
    flex-shrink: 0;
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 10px 18px;
  }
  .logo {
    width: 28px;
    height: 28px;
    flex: none;
    filter: drop-shadow(0 0 10px rgba(157, 134, 255, 0.35));
    pointer-events: none;
  }
  .word {
    font-family: var(--serif);
    font-size: 25px;
    font-weight: 700;
    letter-spacing: -0.03em;
    pointer-events: none;
  }
  .search {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 34px;
    padding: 0 10px;
    margin: 0 2px 14px;
    border-radius: 9px;
    background: var(--surface);
    border: 1px solid var(--line);
    color: var(--dim);
  }
  .search:focus-within {
    border-color: var(--line-strong);
    color: var(--text);
  }
  .search input {
    flex: 1;
    min-width: 0;
    background: none;
    border: 0;
    outline: none;
    font-size: 13px;
  }
  nav {
    flex: 1;
    overflow-y: auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  nav button,
  .foot > button {
    display: flex;
    align-items: center;
    gap: 12px;
    height: 34px;
    padding: 0 10px;
    border-radius: 8px;
    color: var(--muted);
    font-weight: 500;
    text-align: left;
    flex-shrink: 0;
    min-width: 0;
  }
  nav button:hover,
  .foot > button:hover {
    color: var(--text);
    background: rgba(255, 255, 255, 0.04);
  }
  nav button.active,
  .foot > button.active {
    color: var(--text);
    background: var(--surface-hover);
  }
  nav button.active :global(svg) {
    color: var(--coral);
  }
  .group {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin: 20px 10px 6px;
    font-size: 11px;
    letter-spacing: 0.14em;
    text-transform: uppercase;
    color: var(--dim);
    font-weight: 600;
  }
  .group .add {
    height: 22px;
    width: 22px;
    padding: 0;
    justify-content: center;
    gap: 0;
  }
  .hint {
    font-size: 12px;
    color: var(--dim);
    padding: 0 10px;
    margin: 4px 0;
  }
  .foot {
    border-top: 1px solid var(--line);
    padding-top: 10px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .scan {
    padding: 4px 10px;
    font-size: 11.5px;
    color: var(--muted);
  }
  .bar {
    height: 3px;
    border-radius: 3px;
    background: var(--surface-hover);
    margin-bottom: 6px;
    overflow: hidden;
  }
  .bar i {
    display: block;
    height: 100%;
    background: var(--accent);
    transition: width 0.3s;
  }
</style>
