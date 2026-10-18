<script lang="ts">
  import { ApiError, api, type Tag } from '#lib/api';
  import TagChip from '#lib/components/TagChip.svelte';

  let tags = $state<Tag[]>([]);
  let includeArchived = $state(false);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let notice = $state<string | null>(null);
  let newName = $state('');
  let newColor = $state('#f59e0b');
  let newEmoji = $state('');
  let creating = $state(false);
  let editingId = $state<number | null>(null);
  let editName = $state('');
  let editColor = $state('#f59e0b');
  let editEmoji = $state('');
  let editSort = $state(0);
  let savingId = $state<number | null>(null);

  $effect(() => {
    void load(includeArchived);
  });

  async function load(withArchived: boolean) {
    loading = true;
    error = null;
    try {
      tags = await api.listTags(withArchived);
    } catch (cause) {
      error = cause instanceof ApiError ? cause.message : '加载标签失败';
    } finally {
      loading = false;
    }
  }

  async function create(event: SubmitEvent) {
    event.preventDefault();
    const name = newName.trim();
    if (!name) return;
    creating = true;
    error = null;
    notice = null;
    try {
      await api.createTag({
        name,
        color: /^#[0-9a-f]{6}$/i.test(newColor) ? newColor : undefined,
        emoji: newEmoji.trim() || undefined
      });
      newName = '';
      newEmoji = '';
      notice = '标签已创建';
      await load(includeArchived);
    } catch (cause) {
      error = cause instanceof ApiError ? cause.message : '创建标签失败';
    } finally {
      creating = false;
    }
  }

  function startEdit(tag: Tag) {
    editingId = tag.id;
    editName = tag.name;
    editColor = tag.color ?? '#f59e0b';
    editEmoji = tag.emoji ?? '';
    editSort = tag.sort_order;
    error = null;
    notice = null;
  }

  async function saveEdit(tag: Tag) {
    savingId = tag.id;
    error = null;
    notice = null;
    try {
      await api.updateTag(tag.id, {
        name: editName.trim(),
        color: /^#[0-9a-f]{6}$/i.test(editColor) ? editColor : null,
        emoji: editEmoji.trim() || null,
        sort_order: editSort
      });
      editingId = null;
      notice = '标签已更新';
      await load(includeArchived);
    } catch (cause) {
      error = cause instanceof ApiError ? cause.message : '更新标签失败';
    } finally {
      savingId = null;
    }
  }

  async function archive(tag: Tag) {
    if (!confirm(`归档标签「${tag.name}」？历史日记仍会显示它，但不能再新选。`)) return;
    error = null;
    notice = null;
    try {
      await api.deleteTag(tag.id);
      notice = '标签已归档';
      await load(includeArchived);
    } catch (cause) {
      error = cause instanceof ApiError ? cause.message : '归档标签失败';
    }
  }
</script>

<svelte:head><title>标签 · diary</title></svelte:head>

<div class="mb-5 flex flex-wrap items-center justify-between gap-3">
  <div>
    <h1 class="text-2xl font-bold tracking-tight">标签</h1>
    <p class="mt-0.5 text-sm text-stone-500">心情即标签。先建标签，才能在日记里使用。</p>
  </div>
  <label class="flex items-center gap-2 text-sm text-stone-600 dark:text-stone-300">
    <input type="checkbox" bind:checked={includeArchived} class="h-4 w-4 accent-amber-600" />
    显示已归档
  </label>
</div>

{#if error}
  <p class="mb-4 rounded-lg bg-rose-50 px-4 py-3 text-sm text-rose-800 dark:bg-rose-950 dark:text-rose-200" role="alert">{error}</p>
{/if}
{#if notice}
  <p class="mb-4 rounded-lg bg-emerald-50 px-4 py-3 text-sm text-emerald-800 dark:bg-emerald-950 dark:text-emerald-200">{notice}</p>
{/if}

<form onsubmit={create} class="mb-6 flex flex-wrap items-end gap-3 rounded-2xl border border-stone-200 bg-white p-4 dark:border-stone-800 dark:bg-stone-900">
  <label class="min-w-44 flex-1 text-sm">
    <span class="mb-1 block text-stone-600 dark:text-stone-300">标签名</span>
    <input
      class="w-full rounded-lg border border-stone-300 bg-white px-3 py-2 outline-none focus:border-amber-500 dark:border-stone-700 dark:bg-stone-950"
      maxlength="32"
      bind:value={newName}
      placeholder="例如：开心"
      required
    />
  </label>
  <label class="text-sm">
    <span class="mb-1 block text-stone-600 dark:text-stone-300">颜色</span>
    <input
      type="color"
      class="h-10 w-14 cursor-pointer rounded border border-stone-300 bg-transparent dark:border-stone-700"
      bind:value={newColor}
    />
  </label>
  <label class="w-24 text-sm">
    <span class="mb-1 block text-stone-600 dark:text-stone-300">Emoji</span>
    <input
      class="w-full rounded-lg border border-stone-300 bg-white px-3 py-2 text-center outline-none focus:border-amber-500 dark:border-stone-700 dark:bg-stone-950"
      maxlength="4"
      bind:value={newEmoji}
      placeholder="可选"
    />
  </label>
  <button
    type="submit"
    disabled={creating}
    class="rounded-lg bg-amber-600 px-4 py-2 text-sm font-medium text-white hover:bg-amber-700 disabled:opacity-50"
  >
    {creating ? '创建中…' : '新建标签'}
  </button>
</form>

{#if loading}
  <p class="py-12 text-center text-sm text-stone-500">加载中…</p>
{:else if tags.length === 0}
  <p class="rounded-2xl border border-dashed border-stone-300 px-6 py-12 text-center text-sm text-stone-500 dark:border-stone-700">
    还没有标签，先在上方创建一个。
  </p>
{:else}
  <p class="mb-2 text-xs text-stone-400">{tags.length} 个标签</p>
  <ul class="divide-y divide-stone-100 overflow-hidden rounded-2xl border border-stone-200 bg-white dark:divide-stone-800 dark:border-stone-800 dark:bg-stone-900">
    {#each tags as tag (tag.id)}
      <li class="p-3">
        {#if editingId === tag.id}
          <form
            onsubmit={(event) => { event.preventDefault(); void saveEdit(tag); }}
            class="flex flex-wrap items-center gap-2"
          >
            <input
              class="min-w-40 flex-1 rounded-lg border border-stone-300 bg-white px-3 py-1.5 text-sm outline-none focus:border-amber-500 dark:border-stone-700 dark:bg-stone-950"
              maxlength="32"
              bind:value={editName}
              required
            />
            <input
              type="color"
              class="h-9 w-12 cursor-pointer rounded border border-stone-300 bg-transparent dark:border-stone-700"
              bind:value={editColor}
            />
            <input
              class="w-20 rounded-lg border border-stone-300 bg-white px-2 py-1.5 text-center text-sm dark:border-stone-700 dark:bg-stone-950"
              maxlength="4"
              bind:value={editEmoji}
              placeholder="Emoji"
            />
            <input
              type="number"
              class="w-20 rounded-lg border border-stone-300 bg-white px-2 py-1.5 text-sm dark:border-stone-700 dark:bg-stone-950"
              bind:value={editSort}
              aria-label="排序"
            />
            <button
              type="submit"
              disabled={savingId === tag.id}
              class="rounded-lg bg-stone-900 px-3 py-1.5 text-sm text-white disabled:opacity-50 dark:bg-stone-100 dark:text-stone-900"
            >
              保存
            </button>
            <button
              type="button"
              class="rounded-lg border border-stone-300 px-3 py-1.5 text-sm dark:border-stone-700"
              onclick={() => (editingId = null)}
            >
              取消
            </button>
          </form>
        {:else}
          <div class="flex flex-wrap items-center gap-3">
            <TagChip {tag} />
            {#if tag.archived}
              <span class="rounded-full bg-stone-100 px-2 py-0.5 text-[11px] text-stone-500 dark:bg-stone-800 dark:text-stone-400">
                已归档
              </span>
            {:else}
              <span class="text-xs text-stone-400">排序 {tag.sort_order}</span>
            {/if}
            <div class="ml-auto flex items-center gap-2">
              {#if !tag.archived}
                <button
                  type="button"
                  class="rounded-lg border border-stone-300 px-3 py-1 text-xs hover:bg-stone-100 dark:border-stone-700 dark:hover:bg-stone-800"
                  onclick={() => startEdit(tag)}
                >
                  编辑
                </button>
                <button
                  type="button"
                  class="rounded-lg border border-rose-200 px-3 py-1 text-xs text-rose-700 hover:bg-rose-50 dark:border-rose-900 dark:text-rose-300 dark:hover:bg-rose-950"
                  onclick={() => archive(tag)}
                >
                  归档标签
                </button>
              {/if}
            </div>
          </div>
        {/if}
      </li>
    {/each}
  </ul>
{/if}
