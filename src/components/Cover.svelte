<script lang="ts">
  import { coverSrc } from "../lib/util";

  let {
    src,
    size = "100%",
    radius = "var(--radius)",
    seed = "",
  }: { src: string | null | undefined; size?: string; radius?: string; seed?: string } = $props();

  let failed = $state(false);
  const url = $derived(failed ? null : coverSrc(src));

  // Deterministic gradient placeholder so albums without art still look distinct.
  const hue = $derived([...seed].reduce((h, c) => (h * 31 + c.charCodeAt(0)) % 360, 17));
  $effect(() => {
    src;
    failed = false;
  });
</script>

<div class="cover" style:width={size} style:border-radius={radius}>
  {#if url}
    <img src={url} alt="" loading="lazy" decoding="async" draggable="false" onerror={() => (failed = true)} />
  {:else}
    <div class="ph" style:background="linear-gradient(140deg, hsl({hue} 45% 28%), hsl({(hue + 50) % 360} 55% 18%))">
      <svg viewBox="0 0 24 24" width="34%" fill="none" stroke="rgba(255,255,255,.35)" stroke-width="1.4">
        <circle cx="12" cy="12" r="9" /><circle cx="12" cy="12" r="2.5" />
      </svg>
    </div>
  {/if}
</div>

<style>
  .cover {
    aspect-ratio: 1;
    overflow: hidden;
    background: var(--surface);
    flex-shrink: 0;
    position: relative;
    box-shadow: 0 1px 0 rgba(255, 255, 255, 0.04) inset;
  }
  img,
  .ph {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .ph {
    display: grid;
    place-items: center;
  }
</style>
