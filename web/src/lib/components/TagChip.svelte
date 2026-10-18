<script lang="ts">
  type TagLike = { name: string; color?: string | null; emoji?: string | null };
  let {
    tag,
    active = false,
    onclick = undefined
  }: {
    tag: TagLike;
    active?: boolean;
    onclick?: (() => void) | undefined;
  } = $props();

  const tint = $derived(tag.color ? `${tag.color}22` : undefined);
  const border = $derived(tag.color ?? undefined);
</script>

{#if onclick}
  <button
    type="button"
    onclick={onclick}
    class="inline-flex items-center gap-1 rounded-full border px-2.5 py-1 text-xs transition {active
      ? 'ring-2 ring-amber-500/60 font-medium'
      : ''}"
    style:background-color={tint}
    style:border-color={border}
  >
    {#if tag.emoji}<span aria-hidden="true">{tag.emoji}</span>{/if}
    <span>{tag.name}</span>
  </button>
{:else}
  <span
    class="inline-flex items-center gap-1 rounded-full border border-stone-200 px-2.5 py-1 text-xs dark:border-stone-700"
    style:background-color={tint}
    style:border-color={border}
  >
    {#if tag.emoji}<span aria-hidden="true">{tag.emoji}</span>{/if}
    <span>{tag.name}</span>
  </span>
{/if}
