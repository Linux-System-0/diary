<script lang="ts">
  import { ApiError, api, type Entry } from '#lib/api';
  import {
    addMonths,
    calendarDays,
    isSameMonth,
    isToday,
    monthRange,
    monthTitle,
    startOfMonth,
    subMonths,
    toIso,
    weekdayLabel
  } from '#lib/utils/date';

  let month = $state(startOfMonth(new Date()));
  let entries = $state<Entry[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let requestId = 0;

  const days = $derived(calendarDays(month));
  const byDate = $derived(new Map(entries.map((entry) => [entry.date, entry])));
  const weekdays = $derived(days.slice(0, 7).map(weekdayLabel));

  $effect(() => {
    void loadMonth(month);
  });

  async function loadMonth(target: Date) {
    const id = ++requestId;
    loading = true;
    error = null;
    try {
      const range = monthRange(target);
      const result = await api.listEntries(range.from, range.to);
      if (id === requestId) entries = result;
    } catch (cause) {
      if (id === requestId) {
        error = cause instanceof ApiError ? cause.message : '加载日记失败';
      }
    } finally {
      if (id === requestId) loading = false;
    }
  }
</script>

<svelte:head><title>日历 · diary</title></svelte:head>

<div class="mb-5 flex flex-wrap items-center justify-between gap-3">
  <div>
    <h1 class="text-2xl font-bold tracking-tight">日历</h1>
    <p class="mt-0.5 text-sm text-stone-500">一天一篇，点击日期开始写。</p>
  </div>
  <div class="flex items-center gap-2">
    <button
      type="button"
      class="rounded-lg border border-stone-300 px-2.5 py-1.5 text-sm hover:bg-stone-100 dark:border-stone-700 dark:hover:bg-stone-900"
      onclick={() => (month = subMonths(month, 1))}
      aria-label="上个月"
    >
      ←
    </button>
    <span class="min-w-36 text-center text-sm font-medium">{monthTitle(month)}</span>
    <button
      type="button"
      class="rounded-lg border border-stone-300 px-2.5 py-1.5 text-sm hover:bg-stone-100 dark:border-stone-700 dark:hover:bg-stone-900"
      onclick={() => (month = addMonths(month, 1))}
      aria-label="下个月"
    >
      →
    </button>
    <button
      type="button"
      class="rounded-lg bg-stone-900 px-3 py-1.5 text-sm text-white hover:bg-stone-700 dark:bg-stone-100 dark:text-stone-900 dark:hover:bg-stone-300"
      onclick={() => (month = startOfMonth(new Date()))}
    >
      今天
    </button>
  </div>
</div>

{#if error}
  <p class="mb-4 rounded-lg bg-rose-50 px-4 py-3 text-sm text-rose-800 dark:bg-rose-950 dark:text-rose-200">
    {error}
  </p>
{/if}

<div class="overflow-hidden rounded-2xl border border-stone-200 bg-white shadow-sm dark:border-stone-800 dark:bg-stone-900">
  <div class="grid grid-cols-7 border-b border-stone-200 bg-stone-50 text-center text-xs font-medium text-stone-500 dark:border-stone-800 dark:bg-stone-950">
    {#each weekdays as label}
      <div class="py-2">{label}</div>
    {/each}
  </div>
  <div class="grid grid-cols-7">
    {#each days as day, index (toIso(day))}
      {@const iso = toIso(day)}
      {@const entry = byDate.get(iso)}
      <a
        href={`/day/${iso}`}
        class="group relative min-h-24 border-stone-100 p-2 transition hover:bg-amber-50/70 dark:border-stone-800 dark:hover:bg-amber-950/20 sm:min-h-28 {index %
          7 ===
        6
          ? ''
          : 'border-r'} {index < 35 ? 'border-b' : ''} {isSameMonth(day, month)
          ? ''
          : 'bg-stone-50/60 text-stone-400 dark:bg-stone-950/40'}"
        aria-label={`${iso}${entry ? '，已有日记' : ''}`}
      >
        <span
          class="inline-flex h-6 w-6 items-center justify-center rounded-full text-xs {isToday(day)
            ? 'bg-amber-600 font-semibold text-white'
            : ''}"
        >
          {day.getDate()}
        </span>
        {#if entry}
          <div class="mt-1.5 space-y-1">
            <p class="line-clamp-2 text-[11px] leading-4 text-stone-500 dark:text-stone-400">
              {entry.content_md.replace(/[#>*_`~\-]/g, '').slice(0, 90) || '（空白日记）'}
            </p>
            {#if entry.tags.length}
              <div class="flex flex-wrap gap-1">
                {#each entry.tags.slice(0, 3) as tag}
                  <span
                    class="inline-flex items-center gap-1 rounded-full px-1.5 py-0.5 text-[10px]"
                    style:background-color={tag.color ? `${tag.color}22` : undefined}
                    title={tag.name}
                  >
                    {#if tag.emoji}<span aria-hidden="true">{tag.emoji}</span>{/if}
                    <span class="max-w-14 truncate">{tag.name}</span>
                  </span>
                {/each}
                {#if entry.tags.length > 3}
                  <span class="text-[10px] text-stone-400">+{entry.tags.length - 3}</span>
                {/if}
              </div>
            {/if}
          </div>
        {:else}
          <span
            class="pointer-events-none absolute bottom-2 right-2 hidden text-lg text-amber-600/60 group-hover:block dark:text-amber-400/60"
            aria-hidden="true">＋</span
          >
        {/if}
      </a>
    {/each}
  </div>
</div>

{#if loading}
  <p class="mt-3 text-center text-xs text-stone-400">加载中…</p>
{/if}
