<script lang="ts">
  import MarkdownView from './MarkdownView.svelte';

  let { source, delay = 200 }: { source: string; delay?: number } = $props();
  // svelte-ignore state_referenced_locally
  let debounced = $state(source);

  $effect(() => {
    const value = source;
    const timer = setTimeout(() => {
      debounced = value;
    }, delay);
    return () => clearTimeout(timer);
  });
</script>

<MarkdownView source={debounced} />
