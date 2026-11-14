<script lang="ts">
  /** Lightweight scrubber: commits on release so seeking doesn't spam the engine. */
  let {
    value,
    max = 1,
    oncommit,
    oninput,
    label = "",
  }: { value: number; max?: number; oncommit: (v: number) => void; oninput?: (v: number) => void; label?: string } = $props();

  let el: HTMLDivElement | undefined = $state();
  let dragging = $state<number | null>(null);
  const shown = $derived(dragging ?? value);
  const pct = $derived(max > 0 ? Math.min(100, Math.max(0, (shown / max) * 100)) : 0);

  function at(e: PointerEvent) {
    const r = el!.getBoundingClientRect();
    return Math.min(1, Math.max(0, (e.clientX - r.left) / r.width)) * max;
  }
  function down(e: PointerEvent) {
    if (max <= 0) return;
    el!.setPointerCapture(e.pointerId);
    dragging = at(e);
    oninput?.(dragging);
  }
  function move(e: PointerEvent) {
    if (dragging === null) return;
    dragging = at(e);
    oninput?.(dragging);
  }
  function up(e: PointerEvent) {
    if (dragging === null) return;
    const v = at(e);
    dragging = null;
    oncommit(v);
  }
  function key(e: KeyboardEvent) {
    const step = max / 20;
    if (e.key === "ArrowRight") oncommit(Math.min(max, value + step));
    if (e.key === "ArrowLeft") oncommit(Math.max(0, value - step));
  }
</script>

<div
  class="slider"
  class:dragging={dragging !== null}
  bind:this={el}
  onpointerdown={down}
  onpointermove={move}
  onpointerup={up}
  onkeydown={key}
  role="slider"
  tabindex="0"
  aria-label={label}
  aria-valuemin={0}
  aria-valuemax={max}
  aria-valuenow={shown}
>
  <div class="track"><div class="fill" style:width="{pct}%"></div></div>
  <div class="thumb" style:left="{pct}%"></div>
</div>

<style>
  .slider {
    position: relative;
    height: 16px;
    display: flex;
    align-items: center;
    cursor: pointer;
    touch-action: none;
    flex: 1;
  }
  .track {
    width: 100%;
    height: 4px;
    border-radius: 4px;
    background: rgba(255, 255, 255, 0.12);
    overflow: hidden;
  }
  .fill {
    height: 100%;
    background: var(--text);
  }
  .slider:hover .fill,
  .dragging .fill {
    background: var(--accent);
  }
  .thumb {
    position: absolute;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: #fff;
    transform: translateX(-50%) scale(0);
    transition: transform 0.12s;
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.4);
  }
  .slider:hover .thumb,
  .dragging .thumb {
    transform: translateX(-50%) scale(1);
  }
</style>
