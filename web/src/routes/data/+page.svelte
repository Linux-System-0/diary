<script lang="ts">
  import { eachDayOfInterval } from 'date-fns';
  import { ApiError, api, type DiaryPayloadJson, type ImportCommit, type ImportPreview } from '#lib/api';
  import {
    DiaryFormatError,
    decryptDiary,
    encryptDiary,
    isEncrypted,
    parseDiaryJson,
    parseHeader,
    sealPlaintextDiary,
    type DiaryHeader
  } from '#lib/diary/format';
  import type { DiaryPayload } from '#lib/diary/schema';
  import { fromIso, toIso } from '#lib/utils/date';
  import { passwordHint } from '#lib/utils/password';

  let exportScope = $state<'all' | 'range'>('all');
  let exportFrom = $state(toIso(new Date()));
  let exportTo = $state(toIso(new Date()));
  let exportPassword = $state('');
  let exportConfirm = $state('');
  let exportUnencrypted = $state(false);
  let exporting = $state(false);
  let exportError = $state<string | null>(null);
  let exportNotice = $state<string | null>(null);

  let fileName = $state('');
  let fileBytes = $state<Uint8Array | null>(null);
  let fileHeader = $state<DiaryHeader | null>(null);
  let encryptedFile = $state(false);
  let importPassword = $state('');
  let unlocking = $state(false);
  let importPayload = $state<DiaryPayload | null>(null);
  let importError = $state<string | null>(null);
  let importNotice = $state<string | null>(null);
  let importScopeAll = $state(true);
  let selected = $state<Record<string, boolean>>({});
  let previewing = $state(false);
  let preview = $state<ImportPreview | null>(null);
  let committing = $state(false);
  let commitResult = $state<ImportCommit | null>(null);

  const fileDates = $derived(
    importPayload ? [...new Set(importPayload.entries.map((entry) => entry.date))].sort() : []
  );
  const selectedDates = $derived(
    fileDates.filter((date) => selected[date])
  );
  const importScope = $derived(importScopeAll ? 'all' : 'dates');
  const exportHint = $derived(passwordHint(exportPassword));

  async function doExport() {
    exportError = null;
    exportNotice = null;
    if (!exportUnencrypted) {
      if (exportPassword.length < 8) {
        exportError = '导出密码至少 8 个字符';
        return;
      }
      if (exportPassword !== exportConfirm) {
        exportError = '两次输入的导出密码不一致';
        return;
      }
    }

    let dates: string[] | undefined;
    if (exportScope === 'range') {
      try {
        const start = fromIso(exportFrom);
        const end = fromIso(exportTo);
        if (start > end) {
          exportError = '起始日期不能晚于结束日期';
          return;
        }
        const days = eachDayOfInterval({ start, end });
        if (days.length > 100_000) {
          exportError = '日期范围过大';
          return;
        }
        dates = days.map(toIso);
      } catch {
        exportError = '日期格式错误';
        return;
      }
    }

    exporting = true;
    try {
      const payload = await api.prepareExport(exportScope === 'all' ? 'all' : 'dates', dates);
      const plaintext = new TextEncoder().encode(JSON.stringify(payload));
      let file: Uint8Array;
      try {
        file = exportUnencrypted
          ? sealPlaintextDiary(plaintext)
          : await encryptDiary(plaintext, exportPassword);
      } finally {
        plaintext.fill(0);
      }
      const blob = new Blob([file as BufferSource], { type: 'application/octet-stream' });
      const url = URL.createObjectURL(blob);
      const link = document.createElement('a');
      link.href = url;
      link.download = `diary-${toIso(new Date())}.diary`;
      link.click();
      setTimeout(() => URL.revokeObjectURL(url), 1000);
      exportPassword = '';
      exportConfirm = '';
      exportNotice = exportUnencrypted
        ? '已导出未加密文件。任何人拿到都能阅读，请尽快妥善保管。'
        : '已导出加密文件。请记住导出密码：忘记后无法恢复。';
    } catch (cause) {
      exportError = cause instanceof ApiError ? cause.message : '导出失败';
    } finally {
      exporting = false;
    }
  }

  async function pickFile(event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    resetImport();
    if (!file) return;
    fileName = file.name;
    if (file.size > 67_108_864 + 57) {
      importError = '文件超过 64 MiB 上限';
      return;
    }
    try {
      const bytes = new Uint8Array(await file.arrayBuffer());
      fileHeader = parseHeader(bytes);
      fileBytes = bytes;
      encryptedFile = isEncrypted(fileHeader);
      if (!encryptedFile) {
        await unlock();
      }
    } catch (cause) {
      importError =
        cause instanceof DiaryFormatError ? cause.message : '无法读取文件';
      fileBytes = null;
      fileHeader = null;
    }
  }

  async function unlock() {
    if (!fileBytes) return;
    importError = null;
    importNotice = null;
    unlocking = true;
    try {
      const plaintext = await decryptDiary(fileBytes, importPassword);
      try {
        importPayload = parseDiaryJson(plaintext);
      } finally {
        plaintext.fill(0);
      }
      selected = Object.fromEntries(importPayload.entries.map((entry) => [entry.date, true]));
      importPassword = '';
      importNotice = `已解密：${importPayload.entries.length} 篇日记，${importPayload.tags.length} 个标签定义。`;
    } catch (cause) {
      importError =
        cause instanceof DiaryFormatError ? cause.message : '解密失败';
    } finally {
      unlocking = false;
    }
  }

  async function runPreview() {
    if (!importPayload) return;
    importError = null;
    importNotice = null;
    commitResult = null;
    if (!importScopeAll && selectedDates.length === 0) {
      importError = '请至少选择一个日期';
      return;
    }
    previewing = true;
    try {
      preview = await api.previewImport({
        entries: importPayload.entries,
        tags: importPayload.tags,
        scope: importScope,
        dates: importScopeAll ? undefined : selectedDates
      });
    } catch (cause) {
      importError = cause instanceof ApiError ? cause.message : '预检失败';
    } finally {
      previewing = false;
    }
  }

  async function commit() {
    if (!preview) return;
    committing = true;
    importError = null;
    try {
      commitResult = await api.commitImport(
        preview.import_id,
        importScope,
        importScopeAll ? undefined : selectedDates
      );
      preview = null;
      importNotice = null;
    } catch (cause) {
      importError = cause instanceof ApiError ? cause.message : '导入失败';
    } finally {
      committing = false;
    }
  }

  function toggleDate(date: string) {
    selected = { ...selected, [date]: !selected[date] };
    preview = null;
  }

  function toggleAllDates() {
    const allSelected = fileDates.every((date) => selected[date]);
    selected = Object.fromEntries(fileDates.map((date) => [date, !allSelected]));
    preview = null;
  }

  function resetImport() {
    fileName = '';
    fileBytes = null;
    fileHeader = null;
    encryptedFile = false;
    importPassword = '';
    importPayload = null;
    importError = null;
    importNotice = null;
    preview = null;
    commitResult = null;
    selected = {};
  }
</script>

<svelte:head><title>数据 · diary</title></svelte:head>

<h1 class="text-2xl font-bold tracking-tight">数据</h1>
<p class="mt-0.5 text-sm text-stone-500">导出加密归档，或从 .diary 文件恢复。加解密只在浏览器中完成。</p>

<section class="mt-6 rounded-2xl border border-stone-200 bg-white p-5 dark:border-stone-800 dark:bg-stone-900">
  <h2 class="font-semibold">导出</h2>
  <p class="mt-1 text-sm text-stone-500">
    Argon2id（64 MiB / t=3）+ AES-256-GCM。导出密码不会离开浏览器。
  </p>

  {#if exportError}
    <p class="mt-3 rounded-lg bg-rose-50 px-4 py-3 text-sm text-rose-800 dark:bg-rose-950 dark:text-rose-200" role="alert">{exportError}</p>
  {/if}
  {#if exportNotice}
    <p class="mt-3 rounded-lg bg-emerald-50 px-4 py-3 text-sm text-emerald-800 dark:bg-emerald-950 dark:text-emerald-200">{exportNotice}</p>
  {/if}

  <form onsubmit={(event) => { event.preventDefault(); void doExport(); }} class="mt-4 space-y-4">
    <div class="flex flex-wrap items-center gap-4 text-sm">
      <label class="flex items-center gap-2">
        <input type="radio" name="export-scope" value="all" bind:group={exportScope} class="accent-amber-600" />
        全部日记
      </label>
      <label class="flex items-center gap-2">
        <input type="radio" name="export-scope" value="range" bind:group={exportScope} class="accent-amber-600" />
        日期范围
      </label>
      {#if exportScope === 'range'}
        <div class="flex items-center gap-2">
          <input type="date" class="rounded-lg border border-stone-300 bg-white px-2 py-1.5 dark:border-stone-700 dark:bg-stone-950" bind:value={exportFrom} aria-label="起始日期" />
          <span class="text-stone-400">–</span>
          <input type="date" class="rounded-lg border border-stone-300 bg-white px-2 py-1.5 dark:border-stone-700 dark:bg-stone-950" bind:value={exportTo} aria-label="结束日期" />
        </div>
      {/if}
    </div>

    <label class="flex items-center gap-2 text-sm text-stone-600 dark:text-stone-300">
      <input type="checkbox" bind:checked={exportUnencrypted} class="h-4 w-4 accent-rose-600" />
      不加密导出（危险：任何人拿到都能阅读）
    </label>

    {#if !exportUnencrypted}
      <div class="grid gap-3 sm:grid-cols-2">
        <label class="text-sm">
          <span class="mb-1 block text-stone-600 dark:text-stone-300">导出密码</span>
          <input
            type="password"
            class="w-full rounded-lg border border-stone-300 bg-white px-3 py-2 outline-none focus:border-amber-500 dark:border-stone-700 dark:bg-stone-950"
            autocomplete="new-password"
            minlength="8"
            bind:value={exportPassword}
            required
          />
          {#if exportHint}
            <span class="mt-1 block text-xs {exportHint.tone}">强度：{exportHint.label}</span>
          {/if}
        </label>
        <label class="text-sm">
          <span class="mb-1 block text-stone-600 dark:text-stone-300">确认密码</span>
          <input
            type="password"
            class="w-full rounded-lg border border-stone-300 bg-white px-3 py-2 outline-none focus:border-amber-500 dark:border-stone-700 dark:bg-stone-950"
            autocomplete="new-password"
            minlength="8"
            bind:value={exportConfirm}
            required
          />
        </label>
      </div>
    {/if}

    <button
      type="submit"
      disabled={exporting}
      class="rounded-lg bg-amber-600 px-4 py-2 text-sm font-medium text-white hover:bg-amber-700 disabled:opacity-50"
    >
      {exporting ? '导出中…' : '导出 .diary'}
    </button>
  </form>
</section>

<section class="mt-6 rounded-2xl border border-stone-200 bg-white p-5 dark:border-stone-800 dark:bg-stone-900">
  <h2 class="font-semibold">导入</h2>
  <p class="mt-1 text-sm text-stone-500">
    文件中出现的日期会整体覆盖（正文 + 标签）；文件里没有的日期不会改动。
  </p>

  {#if importError}
    <p class="mt-3 rounded-lg bg-rose-50 px-4 py-3 text-sm text-rose-800 dark:bg-rose-950 dark:text-rose-200" role="alert">{importError}</p>
  {/if}
  {#if importNotice}
    <p class="mt-3 rounded-lg bg-emerald-50 px-4 py-3 text-sm text-emerald-800 dark:bg-emerald-950 dark:text-emerald-200">{importNotice}</p>
  {/if}
  {#if commitResult}
    <p class="mt-3 rounded-lg bg-emerald-50 px-4 py-3 text-sm text-emerald-800 dark:bg-emerald-950 dark:text-emerald-200">
      导入完成：新建 {commitResult.created} 篇，覆盖 {commitResult.updated} 篇，新建标签 {commitResult.tags_created} 个。
    </p>
  {/if}

  <div class="mt-4">
    <label class="inline-flex cursor-pointer items-center gap-2 rounded-lg border border-dashed border-stone-300 px-4 py-2 text-sm text-stone-600 hover:border-amber-500 dark:border-stone-700 dark:text-stone-300">
      <span>选择 .diary 文件</span>
      <input type="file" accept=".diary,application/octet-stream" class="hidden" onchange={pickFile} />
    </label>
    {#if fileName}
      <span class="ml-3 text-xs text-stone-500">{fileName}{encryptedFile ? ' · 已加密' : ' · 未加密'}</span>
    {/if}
  </div>

  {#if fileBytes && encryptedFile && !importPayload}
    <form onsubmit={(event) => { event.preventDefault(); void unlock(); }} class="mt-4 flex flex-wrap items-end gap-2">
      <label class="min-w-52 flex-1 text-sm">
        <span class="mb-1 block text-stone-600 dark:text-stone-300">文件密码</span>
        <input
          type="password"
          class="w-full rounded-lg border border-stone-300 bg-white px-3 py-2 outline-none focus:border-amber-500 dark:border-stone-700 dark:bg-stone-950"
          bind:value={importPassword}
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

  {#if importPayload}
    <div class="mt-4 rounded-xl border border-stone-200 p-3 dark:border-stone-800">
      <div class="flex flex-wrap items-center justify-between gap-2 text-sm">
        <div class="flex flex-wrap items-center gap-4">
          <label class="flex items-center gap-2">
            <input type="radio" name="import-scope" bind:group={importScopeAll} value={true} class="accent-amber-600" onchange={() => (preview = null)} />
            全部日期
          </label>
          <label class="flex items-center gap-2">
            <input type="radio" name="import-scope" bind:group={importScopeAll} value={false} class="accent-amber-600" onchange={() => (preview = null)} />
            仅选中的日期
          </label>
        </div>
        <span class="text-xs text-stone-400">{fileDates.length} 个日期</span>
      </div>

      {#if !importScopeAll}
        <div class="mt-3">
          <div class="mb-2 flex items-center gap-3 text-xs">
            <button type="button" class="text-amber-700 hover:underline dark:text-amber-400" onclick={toggleAllDates}>
              全选 / 全不选
            </button>
            <span class="text-stone-400">已选 {selectedDates.length} 天</span>
          </div>
          <div class="max-h-48 overflow-auto rounded-lg border border-stone-100 p-2 dark:border-stone-800">
            <div class="grid gap-1 sm:grid-cols-3">
              {#each fileDates as date (date)}
                <label class="flex items-center gap-1.5 rounded px-1 py-0.5 text-xs hover:bg-stone-50 dark:hover:bg-stone-800">
                  <input type="checkbox" checked={Boolean(selected[date])} onchange={() => toggleDate(date)} class="h-3.5 w-3.5 accent-amber-600" />
                  {date}
                </label>
              {/each}
            </div>
          </div>
        </div>
      {/if}

      <button
        type="button"
        class="mt-3 rounded-lg bg-amber-600 px-4 py-2 text-sm font-medium text-white hover:bg-amber-700 disabled:opacity-50"
        disabled={previewing}
        onclick={runPreview}
      >
        {previewing ? '预检中…' : '预检导入'}
      </button>
    </div>
  {/if}

  {#if preview}
    <div class="mt-4 rounded-xl border border-amber-200 bg-amber-50/60 p-4 text-sm dark:border-amber-900 dark:bg-amber-950/30">
      <p class="font-medium">预检结果</p>
      <p class="mt-1">
        新建 <strong>{preview.summary.create}</strong> 篇 · 覆盖 <strong>{preview.summary.update}</strong> 篇 · 新建标签 <strong>{preview.summary.tags_create}</strong> 个
      </p>
      {#if preview.tags_to_create.length}
        <p class="mt-2 text-xs text-stone-600 dark:text-stone-300">
          将创建标签：{preview.tags_to_create.map((tag) => tag.name).join('、')}
        </p>
      {/if}
      {#if preview.dates.create.length}
        <details class="mt-2 text-xs">
          <summary class="cursor-pointer text-stone-600 dark:text-stone-300">新建日期（{preview.dates.create.length}）</summary>
          <p class="mt-1 break-all text-stone-500">{preview.dates.create.join('、')}</p>
        </details>
      {/if}
      {#if preview.dates.update.length}
        <details class="mt-2 text-xs">
          <summary class="cursor-pointer text-stone-600 dark:text-stone-300">覆盖日期（{preview.dates.update.length}）</summary>
          <p class="mt-1 break-all text-stone-500">{preview.dates.update.join('、')}</p>
        </details>
      {/if}
      {#if preview.warnings.length}
        <ul class="mt-2 list-disc pl-5 text-xs text-amber-800 dark:text-amber-200">
          {#each preview.warnings as warning}
            <li>{warning}</li>
          {/each}
        </ul>
      {/if}
      <button
        type="button"
        disabled={committing}
        class="mt-3 rounded-lg bg-stone-900 px-4 py-2 text-sm text-white disabled:opacity-50 dark:bg-stone-100 dark:text-stone-900"
        onclick={commit}
      >
        {committing ? '导入中…' : '确认导入'}
      </button>
    </div>
  {/if}
</section>
