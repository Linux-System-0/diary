<script lang="ts">
  import { eachDayOfInterval, endOfMonth, startOfMonth } from 'date-fns';
  import type { DiaryEntry, DiaryTag } from '#lib/diary/schema';
  import { toIso } from '#lib/utils/date';

  let {
    entries,
    tags,
    selectedDate,
    onselect
  }: {
    entries: DiaryEntry[];
    tags: DiaryTag[];
    selectedDate: string;
    onselect: (date: string) => void;
  } = $props();

  const byDate = $derived(new Map(entries.map((entry) => [entry.date, entry])));
  const tagColors = $derived(new Map(tags.map((tag) => [tag.name, tag.color ?? null])));
  const years = $derived(
    [...new Set(entries.map((entry) => entry.date.slice(0, 4)))].sort().reverse()
  );
  // svelte-ignore state_referenced_locally
  let year = $state(years[0] ?? String(new Date().getFullYear()));

  $effect(() => {
    if (years.length && !years.includes(year)) {
      year = years[0];
    }
  });

  const months = $derived(
    Array.from({ length: 12 }, (_, index) => {
      const start = new Date(Number(year), index, 1);
      const days = eachDayOfInterval({ start: startOfMonth(start), end: endOfMonth(start) });
      const leading = (start.getDay() + 6) % 7;
      return {
        index,
        label: `${index + 1} 月`,
        cells: [
          ...Array.from({ length: leading }, () => null),
          ...days.map((day) => ({ date: toIso(day), day: day.getDate() }))
        ]
      };
    })
  );
</script>

<div class="rounded-xl border border-stone-200 bg-white p-3 dark:border-stone-800 dark:bg-stone-900">
  <div class="mb-2 flex items-center justify-between gap-2">
    <span class="text-xs font-medium text-stone-500">日历热力</span>
    <div class="flex gap-1">
      {#each years as item}
        <button
          type="button"
          class="rounded px-2 py-0.5 text-xs {year === item
            ? 'bg-stone-900 text-white dark:bg-stone-100 dark:text-stone-900'
            : 'text-stone-500 hover:bg-stone-100 dark:hover:bg-stone-800'}"
          onclick={() => (year = item)}
        >
          {item}
        </button>
      {/each}
    </div>
  </div>
  <div class="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
    {#each months as month}
      <div>
        <p class="mb-1 text-[11px] text-stone-400">{month.label}</p>
        <div class="grid grid-cols-7 gap-0.5">
          {#each ['一', '二', '三', '四', '五', '六', '日'] as weekday}
            <span class="text-center text-[9px] text-stone-300 dark:text-stone-600">{weekday}</span>
          {/each}
          {#each month.cells as cell}
            {#if cell}
              {@const entry = byDate.get(cell.date)}
              {@const color = entry?.tags.length ? tagColors.get(entry.tags[0]) : null}
              <button
                type="button"
                title={`${cell.date}${entry ? ` · ${entry.tags.join('、')}` : ''}`}
                class="aspect-square rounded-[3px] text-[8px] leading-none transition {entry
                  ? color
                    ? 'hover:brightness-110'
                    : 'bg-amber-500/70 hover:bg-amber-600'
                  : 'bg-stone-100 hover:bg-stone-200 dark:bg-stone-800 dark:hover:bg-stone-700'} {selectedDate ===
                cell.date
                  ? 'ring-2 ring-stone-900 dark:ring-stone-100'
                  : ''}"
                style:background-color={entry && color ? color : undefined}
                onclick={() => onselect(cell.date)}
              >
                <span class="sr-only">{cell.date}</span>
              </button>
            {:else}
              <span></span>
            {/if}
          {/each}
        </div>
      </div>
    {/each}
  </div>
</div>
