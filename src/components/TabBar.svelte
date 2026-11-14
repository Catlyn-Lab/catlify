<script lang="ts">
  import { store, type View } from "../lib/store.svelte";
  import Icon from "./Icon.svelte";

  const tabs: { label: string; icon: string; view: View; family: string[] }[] = [
    { label: "Home", icon: "home", view: { name: "home" }, family: ["home"] },
    { label: "Billboard", icon: "chart", view: { name: "billboard" }, family: ["billboard"] },
    {
      label: "Library",
      icon: "disc",
      view: { name: "library" },
      family: ["library", "songs", "liked", "albums", "album", "artists", "artist", "playlist", "settings", "youtube", "sync", "import"],
    },
    { label: "Search", icon: "search", view: { name: "search", query: "" }, family: ["search"] },
  ];
</script>

<nav class="tabs">
  {#each tabs as t (t.label)}
    {@const active = t.family.includes(store.view.name)}
    <button class:active onclick={() => (active && store.view.name === t.view.name ? null : store.root(t.view))}>
      <Icon name={t.icon} size={22} />
      <span>{t.label}</span>
    </button>
  {/each}
</nav>

<style>
  .tabs {
    display: flex;
    height: calc(var(--tabbar-h) + var(--safe-bottom));
    padding: 0 calc(8px + var(--safe-right)) var(--safe-bottom) calc(8px + var(--safe-left));
    background: rgba(12, 11, 15, 0.96);
    border-top: 1px solid var(--line);
  }
  button {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 3px;
    color: var(--dim);
    font-size: 10.5px;
    font-weight: 600;
    letter-spacing: 0.02em;
    -webkit-tap-highlight-color: transparent;
  }
  button.active {
    color: var(--text);
  }
  button.active :global(svg) {
    color: var(--coral);
  }
</style>
