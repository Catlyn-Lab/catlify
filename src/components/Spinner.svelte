<script lang="ts">
  let { size = 18, color = "currentColor" }: { size?: number; color?: string } = $props();
</script>

<span class="spinner" style:width="{size}px" style:height="{size}px" style:--c={color} role="status" aria-label="Loading"></span>

<style>
  .spinner {
    display: inline-block;
    border-radius: 50%;
    border: 2.5px solid color-mix(in srgb, var(--c) 25%, transparent);
    border-top-color: var(--c);
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
