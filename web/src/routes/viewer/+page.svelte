<script lang="ts">
  import { goto } from '$app/navigation';
  import { ApiError, api, type ImportCommit, type ImportPreview } from '#lib/api';
  import DebouncedMarkdown from '#lib/components/DebouncedMarkdown.svelte';
  import TagChip from '#lib/components/TagChip.svelte';
  import ViewerHeatmap from '#lib/components/ViewerHeatmap.svelte';
  import {
    DiaryFormatError,
    decryptDiary,
    parseDiaryJson,
    parseHeader,
    type DiaryHeader
  } from '#lib/diary/format';
  import type { DiaryEntry, DiaryPayload } from '#lib/diary/schema';
  import { auth } from '#lib/stores/auth';
  import { dayTitle } from '#lib/utils/date';

  const PAGE_SIZE = 30;

  let fileName = $state('');
  let fileBytes = $state<Uint8Array | null>(null);
  let header = $state<DiaryHeader | null>(null);
  let encrypted = $state(false);
  let password = $state('');
  let unlocking = $state(false);
  let payload = $state<DiaryPayload | null>(null);
  let error = $state<string | null>(null);
  let notice = $state<string | null>(null);
  let selectedDate = $state('');
  let visibleCount = $state(PAGE_SIZE);
  let dragging = $state(false);

  let importPreview = $state<ImportPreview | null>(null);
  let importResult = $state<ImportCommit | null>(null);
  let importing = $state(false);

  const sortedEntries = $derived(
    payload
      ? [...payload.entries].sort((a, b) => b.date.localeCompare(a.date))
      : []
  );
  const visibleEntries = $derived(sortedEntries.slice(0, visibleCount));
  const selectedEntry = $derived(
    payload?.entries.find((entry) => entry.date === selectedDate) ?? null
  );
  const tagMap = $derived(
    new Map((payload?.tags ?? []).map((tag) => [tag.name, tag]))
  );

  async function handleFile(file: File | undefined) {
    if (!file) return;
    reset();
    fileName = file.name;
    error = null;
    if (file.size > 67_108_864 + 57) {
      error = '文件超过 64 MiB 上限';
      return;
    }
    try {
      const bytes = new Uint8Array(await file.arrayBuffer());
      const parsedHeader = parseHeader(bytes);
      header = parsedHeader;
      fileBytes = bytes;
      encrypted = (parsedHeader.flags & 1) === 1;
      if (!encrypted) {
        await unlock();
      }
    } catch (cause) {
      error = formatError(cause, '无法读取文件');
      fileBytes = null;
      header = null;
    }
  }

  async function unlock() {
    if (!fileBytes) return;
    unlocking = true;
    error = null;
    try {
      const plaintext = await decryptDiary(fileBytes, encrypted ? password : '');
      try {
        payload = parseDiaryJson(plaintext);
      } finally {
        plaintext.fill(0);
      }
      password = '';
      selectedDate = sortedEntries[0]?.date ?? payload.entries[0]?.date ?? '';
      visibleCount = PAGE_SIZE;
    } catch (cause) {
      error = formatError(cause, '密码错误，或文件已损坏');
    } finally {
      unlocking = false;
    }
  }

  function reset() {
    fileName = '';
    fileBytes = null;
    header = null;
    encrypted = false;
    password = '';
    payload = null;
    error = null;
    notice = null;
    selectedDate = '';
    visibleCount = PAGE_SIZE;
    importPreview = null;
    importResult = null;
  }

  function selectDate(date: string) {
    selectedDate = date;
    const index = sortedEntries.findIndex((entry) => entry.date === date);
    if (index >= visibleCount) {
      visibleCount = index + 1;
    }
  }

  async function startImport() {
    if (!payload) return;
    if ($auth.status !== 'authenticated') {
      await goto('/login?next=/viewer');
      return;
    }
    if (!confirm('将用文件中的全部日期覆盖你的日记（文件中没有的日期不动）。继续？')) {
      return;
    }
    importing = true;
    error = null;
    try {
      importPreview = await api.previewImport({
        entries: payload.entries,
        tags: payload.tags,
        scope: 'all'
      });
    } catch (cause) {
      error = cause instanceof ApiError ? cause.message : '导入预检失败';
    } finally {
      importing = false;
    }
  }

  async function confirmImport() {
    if (!importPreview) return;
    importing = true;
    error = null;
    try {
      importResult = await api.commitImport(importPreview.import_id, 'all');
      importPreview = null;
      notice = `导入完成：新建 ${importResult.created} 篇，覆盖 ${importResult.updated} 篇。`;
    } catch (cause) {
      error = cause instanceof ApiError ? cause.message : '导入失败';
    } finally {
      importing = false;
    }
  }

  function formatError(cause: unknown, fallback: string): string {
    if (cause instanceof DiaryFormatError) {
      if (cause.code === 'decrypt_failed') return '密码错误，或文件已损坏';
      if (cause.code === 'insecure_context') return cause.message;
      return cause.message;
    }
    return fallback;
  }

  function tagsFor(entry: DiaryEntry) {
    return entry.tags.map((name) => tagMap.get(name) ?? { name });
  }

  function onDrop(event: DragEvent) {
    event.preventDefault();
    dragging = false;
    void handleFile(event.dataTransfer?.files?.[0]);
  }
</script>

<svelte:head><title>查看 .diary · diary</title></svelte:head>

<div class="mb-5 flex flex-wrap items-center justify-between gap-3">
  <div>
    <h1 class="text-2xl font-bold tracking-tight">离线查看 .diary</h1>
    <p class="mt-0.5 text-sm text-stone-500">文件不会上传；解密与渲染都在这个浏览器里完成。</p>
  </div>
  <div class="flex items-center gap-3 text-sm">
    {#if $auth.status === 'authenticated'}
      <a href="/" class="text-stone-500 hover:text-stone-800 dark:hover:text-stone-200">返回日记</a>
    {:else}
      <a href="/login" class="text-amber-700 hover:underline dark:text-amber-400">登录</a>
    {/if}
  </div>
</div>

{#if !payload}
  <div
    class="rounded-2xl border-2 border-dashed p-10 text-center transition {dragging
      ? 'border-amber-500 bg-amber-50 dark:bg-amber-950/30'
      : 'border-stone-300 bg-white dark:border-stone-700 dark:bg-stone-900'}"
    role="region"
    aria-label="选择或拖入 .diary 文件"
    ondragover={(event) => { event.preventDefault(); dragging = true; }}
    ondragleave={() => (dragging = false)}
    ondrop={onDrop}
  >
    <div class="text-4xl">📔</div>
    <p class="mt-3 font-medium">选择或拖入 .diary 文件</p>
    <p class="mt-1 text-xs text-stone-500">加密或未加密文件都支持，最大 64 MiB。</p>
    <label class="mt-4 inline-flex cursor-pointer rounded-lg bg-amber-600 px-4 py-2 text-sm font-medium text-white hover:bg-amber-700">
      选择文件
      <input
        type="file"
        accept=".diary,application/octet-stream"
        class="hidden"
        onchange={(event) => handleFile((event.currentTarget as HTMLInputElement).files?.[0])}
      />
    </label>
    {#if fileName}
      <p class="mt-3 text-xs text-stone-500">{fileName}</p>
    {/if}
  </div>

  {#if error}
    <p class="mt-4 rounded-lg bg-rose-50 px-4 py-3 text-sm text-rose-800 dark:bg-rose-950 dark:text-rose-200" role="alert">{error}</p>
  {/if}

  {#if fileBytes && encrypted}
    <form onsubmit={(event) => { event.preventDefault(); void unlock(); }} class="mx-auto mt-4 flex max-w-md flex-wrap items-end gap-2">
      <label class="min-w-52 flex-1 text-sm">
        <span class="mb-1 block text-stone-600 dark:text-stone-300">文件密码</span>
        <input
          type="password"
          class="w-full rounded-lg border border-stone-300 bg-white px-3 py-2 outline-none focus:border-amber-500 dark:border-stone-700 dark:bg-stone-950"
          bind:value={password}
          required
        />
      </label>
      <button
        type="submit"
        disabled={unlocking}
        class="rounded-lg bg-stone-900 px-4 py-2 text-sm text-white disabled:opacity-50 dark:bg-stone-100 dark:text-stone-900"
      >
        {unlocking ? '解密中…' : '解密'}
      </button>
    </form>
  {/if}
{:else}
  <div class="mb-4 flex flex-wrap items-center justify-between gap-2 rounded-xl border border-stone-200 bg-white px-4 py-3 text-xs text-stone-500 dark:border-stone-800 dark:bg-stone-900">
    <span>
      {fileName} · {payload.entries.length} 篇 · {payload.tags.length} 个标签 ·
      {encrypted ? '已加密' : '未加密'}
    </span>
    <button type="button" class="text-amber-700 hover:underline dark:text-amber-400" onclick={reset}>
      换一个文件
    </button>
  </div>

  {#if error}
    <p class="mb-4 rounded-lg bg-rose-50 px-4 py-3 text-sm text-rose-800 dark:bg-rose-950 dark:text-rose-200" role="alert">{error}</p>
  {/if}
  {#if notice}
    <p class="mb-4 rounded-lg bg-emerald-50 px-4 py-3 text-sm text-emerald-800 dark:bg-emerald-950 dark:text-emerald-200">{notice}</p>
  {/if}

  <div class="mb-4 flex flex-wrap items-center gap-2">
    <button
      type="button"
      class="rounded-lg bg-stone-900 px-4 py-2 text-sm text-white hover:bg-stone-700 disabled:opacity-50 dark:bg-stone-100 dark:text-stone-900 dark:hover:bg-stone-300"
      disabled={importing}
      onclick={startImport}
    >
      {$auth.status === 'authenticated' ? '导入到我的日记' : '登录后导入'}
    </button>
    {#if $auth.status !== 'authenticated'}
      <span class="text-xs text-stone-400">导入需要登录；文件仍只在本地，点击后才会发送结构化数据。</span>
    {/if}
  </div>

  {#if importPreview}
    <div class="mb-4 rounded-xl border border-amber-200 bg-amber-50/60 p-4 text-sm dark:border-amber-900 dark:bg-amber-950/30">
      <p class="font-medium">预检结果</p>
      <p class="mt-1">
        新建 {importPreview.summary.create} 篇 · 覆盖 {importPreview.summary.update} 篇 · 新建标签 {importPreview.summary.tags_create} 个
      </p>
      <p class="mt-1 text-xs text-stone-500">文件中没有的日期不会被删除或修改。</p>
      <div class="mt-3 flex gap-2">
        <button
          type="button"
          class="rounded-lg bg-stone-900 px-4 py-2 text-sm text-white disabled:opacity-50 dark:bg-stone-100 dark:text-stone-900"
          disabled={importing}
          onclick={confirmImport}
        >
          {importing ? '导入中…' : '确认导入'}
        </button>
        <button
          type="button"
          class="rounded-lg border border-stone-300 px-4 py-2 text-sm dark:border-stone-700"
          onclick={() => (importPreview = null)}
        >
          取消
        </button>
      </div>
    </div>
  {/if}

  <ViewerHeatmap entries={payload.entries} tags={payload.tags} {selectedDate} onselect={selectDate} />

  <div class="mt-4 grid gap-4 lg:grid-cols-[1fr_1.2fr]">
    <div>
      <h2 class="mb-2 text-xs font-medium uppercase tracking-wide text-stone-400">时间线</h2>
      <div class="space-y-2">
        {#each visibleEntries as entry (entry.date)}
          <button
            type="button"
            class="w-full rounded-xl border p-3 text-left transition {selectedDate === entry.date
              ? 'border-amber-500 bg-amber-50/50 dark:bg-amber-950/20'
              : 'border-stone-200 bg-white hover:border-stone-300 dark:border-stone-800 dark:bg-stone-900 dark:hover:border-stone-700'}"
            onclick={() => (selectedDate = entry.date)}
          >
            <div class="flex items-center justify-between gap-2">
              <span class="text-sm font-medium">{entry.date}</span>
              <div class="flex flex-wrap justify-end gap-1">
                {#each tagsFor(entry) as tag}
                  <TagChip {tag} />
                {/each}
              </div>
            </div>
            <p class="mt-1 line-clamp-2 text-xs text-stone-500 dark:text-stone-400">
              {entry.content_md.replace(/[#>*_`~\-]/g, '').slice(0, 120) || '（空白日记）'}
            </p>
          </button>
        {/each}
      </div>
      {#if visibleCount < sortedEntries.length}
        <button
          type="button"
          class="mt-3 w-full rounded-lg border border-stone-300 py-2 text-sm text-stone-600 hover:bg-stone-100 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800"
          onclick={() => (visibleCount += PAGE_SIZE)}
        >
          加载更多（剩余 {sortedEntries.length - visibleCount}）
        </button>
      {/if}
    </div>

    <div class="lg:sticky lg:top-4 lg:self-start">
      <h2 class="mb-2 text-xs font-medium uppercase tracking-wide text-stone-400">详情</h2>
      <div class="min-h-64 rounded-xl border border-stone-200 bg-white p-5 dark:border-stone-800 dark:bg-stone-900">
        {#if selectedEntry}
          <p class="text-sm text-stone-400">{dayTitle(selectedEntry.date)}</p>
          {#if tagsFor(selectedEntry).length}
            <div class="mt-2 flex flex-wrap gap-1">
              {#each tagsFor(selectedEntry) as tag}
                <TagChip {tag} />
              {/each}
            </div>
          {/if}
          <div class="mt-4">
            {#if selectedEntry.content_md.trim()}
              <DebouncedMarkdown source={selectedEntry.content_md} delay={0} />
            {:else}
              <p class="text-sm text-stone-400">（空白日记）</p>
            {/if}
          </div>
        {:else}
          <p class="text-sm text-stone-400">从时间线或热力图中选择一天。</p>
        {/if}
      </div>
    </div>
  </div>
{/if}
