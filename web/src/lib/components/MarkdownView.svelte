<script lang="ts">
  import { bindImagePlaceholders, renderMarkdown } from '#lib/markdown';

  let { source, class: className = '' }: { source: string; class?: string } = $props();
  let container: HTMLDivElement;

  const html = $derived(renderMarkdown(source));

  $effect(() => {
    if (!container) return;
    container.innerHTML = html;
    return bindImagePlaceholders(container);
  });
</script>

<div bind:this={container} class={`prose-diary text-[15px] ${className}`}></div>
