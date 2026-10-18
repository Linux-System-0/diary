<script lang="ts">
  import { beforeNavigate, goto } from '$app/navigation';
  import { page } from '$app/state';
  import { ApiError, api, type Entry, type Tag } from '#lib/api';
  import DebouncedMarkdown from '#lib/components/DebouncedMarkdown.svelte';
  import MarkdownEditor from '#lib/components/MarkdownEditor.svelte';
  import TagChip from '#lib/components/TagChip.svelte';
  import { deleteDraft, readDraft, writeDraft } from '#lib/db/drafts';
  import { auth } from '#lib/stores/auth';
  import { dayTitle } from '#lib/utils/date';
  import { onMount } from 'svelte';

  const date = $derived(page.params.date ?? '');
  const userId = $derived($auth.status === 'authenticated' ? $auth.user.id : 0);

  let content = $state('');
  let tags = $state<Tag[]>([]);
  let selectedIds = $state<number[]>([]);
  let savedContent = $state('');
  let savedTagIds = $state<number[]>([]);
  let baseUpdatedAt = $state<string | undefined>(undefined);
  let loaded = $state(false);
  let loading = $state(true);
  let saving = $state(false);
  let error = $state<string | null>(null);
  let notice = $state<string | null>(null);
  let draftRestored = $state(false);
  let newTagOpen = $state(false);
  let newTagName = $state('');
  let newTagColor = $state('#f59e0b');
  let creatingTag = $state(false);
  let requestId = 0;

  const dirty = $derived(
    loaded &&
      (content !== savedContent || !sameIds(selectedIds, savedTagIds))
  );
  const byteLength = $derived(new TextEncoder().encode(content).length);
  const heading = $derived(/^\d{4}-\d{2}-\d{2}$/.test(date) ? dayTitle(date) : '日记');
  const selectedTags = $derived(
    selectedIds
      .map((id) => tags.find((tag) => tag.id === id))
      .filter((tag): tag is Tag => Boolean(tag))
  );

  beforeNavigate(({ cancel }) => {
    if (dirty && !confirm('当前日记尚未保存，确定离开吗？')) {
      cancel();
    }
  });

  onMount(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 's') {
        event.preventDefault();
        void save();
      }
    };
    const onBeforeUnload = (event: BeforeUnloadEvent) => {
      if (dirty) {
        event.preventDefault();
        event.returnValue = '';
      }
    };
    window.addEventListener('keydown', onKeyDown);
    window.addEventListener('beforeunload', onBeforeUnload);
    return () => {
      window.removeEventListener('keydown', onKeyDown);
      window.removeEventListener('beforeunload', onBeforeUnload);
    };
  });

  $effect(() => {
    const target = date;
    if (!target) return;
    void loadDay(target);
  });

  $effect(() => {
    if (!loaded || !dirty || !userId || !date) return;
    const snapshot = { content_md: content, tag_ids: [...selectedIds], saved_at: Date.now() };
    const timer = setTimeout(() => {
      void writeDraft(userId, date, snapshot);
    }, 500);
    return () => clearTimeout(timer);
  });

  async function loadDay(target: string) {
    if (!/^\d{4}-\d{2}-\d{2}$/.test(target)) {
      error = '日期格式错误';
      loading = false;
      return;
    }
    const id = ++requestId;
    loading = true;
    loaded = false;
    error = null;
    notice = null;
    draftRestored = false;
    try {
      const [entry, activeTags] = await Promise.all([
        api.getEntry(target).catch((cause) => {
          if (cause instanceof ApiError && cause.code === 'entry_not_found') return null;
          throw cause;
        }),
        api.listTags(false)
      ]);
      if (id !== requestId) return;

      const merged = mergeTags(activeTags, entry);
      tags = merged;
      savedContent = entry?.content_md ?? '';
      content = savedContent;
      savedTagIds = entry?.tags.map((tag) => tag.id) ?? [];
      selectedIds = [...savedTagIds];
      baseUpdatedAt = entry?.updated_at;
      loaded = true;

      if (userId) {
        const draft = await readDraft(userId, target);
        if (id !== requestId) return;
        if (draft) {
          const serverTime = entry ? Date.parse(entry.updated_at) : 0;
          const known = new Set(merged.map((tag) => tag.id));
          const draftIds = draft.tag_ids.filter((tagId) => known.has(tagId));
          const different =
            draft.content_md !== savedContent || !sameIds(draftIds, savedTagIds);
          if (draft.saved_at > serverTime && different) {
            content = draft.content_md;
            selectedIds = draftIds;
            draftRestored = true;
          }
        }
      }
    } catch (cause) {
      if (id === requestId) {
        error = cause instanceof ApiError ? cause.message : '加载日记失败';
      }
    } finally {
      if (id === requestId) {
        loading = false;
      }
    }
  }

  function mergeTags(active: Tag[], entry: Entry | null): Tag[] {
    const merged = [...active];
    for (const tag of entry?.tags ?? []) {
      if (!merged.some((item) => item.id === tag.id)) {
        merged.push({
          id: tag.id,
          name: tag.name,
          color: tag.color,
          emoji: tag.emoji,
          sort_order: 0,
          archived: true
        });
      }
    }
    return merged.sort((a, b) => a.sort_order - b.sort_order || a.name.localeCompare(b.name));
  }

  function toggleTag(tag: Tag) {
    error = null;
    if (selectedIds.includes(tag.id)) {
      selectedIds = selectedIds.filter((id) => id !== tag.id);
      return;
    }
    if (tag.archived) {
      error = '已归档的标签不能再添加到日记';
      return;
    }
    if (selectedIds.length >= 20) {
      error = '每篇日记最多 20 个标签';
      return;
    }
    selectedIds = [...selectedIds, tag.id];
  }

  async function save() {
    if (!loaded || saving || !dirty) return;
    if (byteLength > 1_048_576) {
      error = '正文超过 1 MiB 上限';
      return;
    }
    saving = true;
    error = null;
    notice = null;
    try {
      const entry = await api.putEntry(date, content, selectedIds, baseUpdatedAt);
      baseUpdatedAt = entry.updated_at;
      savedContent = content;
      savedTagIds = selectedIds;
      tags = mergeTags(tags.filter((tag) => !tag.archived), entry);
      selectedIds = [...savedTagIds];
      if (userId) await deleteDraft(userId, date);
      notice = '已保存';
      draftRestored = false;
    } catch (cause) {
      if (cause instanceof ApiError && cause.code === 'stale_write') {
        error = '另一处已修改这篇日记。请复制你的内容后刷新页面。';
      } else {
        error = cause instanceof ApiError ? cause.message : '保存失败';
      }
    } finally {
      saving = false;
    }
  }

  async function removeEntry() {
    if (!confirm('删除这一天的日记？此操作不可撤销。')) return;
    try {
      await api.deleteEntry(date);
      if (userId) await deleteDraft(userId, date);
      await goto('/');
    } catch (cause) {
      error = cause instanceof ApiError ? cause.message : '删除失败';
    }
  }

  async function createTag(event: SubmitEvent) {
    event.preventDefault();
    const name = newTagName.trim();
    if (!name) return;
    if (selectedIds.length >= 20) {
      error = '每篇日记最多 20 个标签';
      return;
    }
    creatingTag = true;
    error = null;
    try {
      const tag = await api.createTag({
        name,
        color: /^#[0-9a-f]{6}$/i.test(newTagColor) ? newTagColor : undefined
      });
      tags = [...tags, tag].sort(
        (a, b) => a.sort_order - b.sort_order || a.name.localeCompare(b.name)
      );
      selectedIds = [...selectedIds, tag.id];
      newTagName = '';
      newTagOpen = false;
    } catch (cause) {
      error = cause instanceof ApiError ? cause.message : '创建标签失败';
    } finally {
      creatingTag = false;
    }
  }

  function sameIds(a: number[], b: number[]): boolean {
    if (a.length !== b.length) return false;
    const left = [...a].sort((x, y) => x - y);
    const right = [...b].sort((x, y) => x - y);
    return left.every((value, index) => value === right[index]);
  }
</script>

<svelte:head><title>{heading} · diary</title></svelte:head>

{#if loading}
  <p class="py-16 text-center text-sm text-stone-500">加载中…</p>
{:else}
  <div class="mb-4 flex flex-wrap items-start justify-between gap-3">
    <div>
      <a href="/" class="text-sm text-stone-500 hover:text-stone-800 dark:hover:text-stone-200">← 返回日历</a>
      <h1 class="mt-1 text-2xl font-bold tracking-tight">{date ? dayTitle(date) : '日记'}</h1>
      <p class="mt-0.5 text-xs text-stone-500">
        {#if saving}
          正在保存…
        {:else if dirty}
          有未保存的修改 · {byteLength} 字节
        {:else if notice}
          {notice}
        {:else if loaded}
          已保存
        {/if}
        <span class="mx-1">·</span>Ctrl/Cmd+S 保存
      </p>
    </div>
    <div class="flex items-center gap-2">
      {#if baseUpdatedAt}
        <button
          type="button"
          class="rounded-lg border border-rose-200 px-3 py-1.5 text-sm text-rose-700 hover:bg-rose-50 dark:border-rose-900 dark:text-rose-300 dark:hover:bg-rose-950"
          onclick={removeEntry}
        >
          删除
        </button>
      {/if}
      <button
        type="button"
        class="rounded-lg bg-amber-600 px-4 py-1.5 text-sm font-medium text-white transition hover:bg-amber-700 disabled:opacity-50"
        onclick={save}
        disabled={saving || !dirty}
      >
        保存
      </button>
    </div>
  </div>

  {#if error}
    <p class="mb-4 rounded-lg bg-rose-50 px-4 py-3 text-sm text-rose-800 dark:bg-rose-950 dark:text-rose-200" role="alert">
      {error}
    </p>
  {/if}
  {#if draftRestored}
    <p class="mb-4 rounded-lg bg-amber-50 px-4 py-3 text-sm text-amber-800 dark:bg-amber-950 dark:text-amber-200">
      已恢复上次未保存的本地草稿。
    </p>
  {/if}

  <section class="mb-4 rounded-xl border border-stone-200 bg-white p-3 dark:border-stone-800 dark:bg-stone-900">
    <div class="flex flex-wrap items-center gap-1.5">
      {#each tags as tag (tag.id)}
        <TagChip
          {tag}
          active={selectedIds.includes(tag.id)}
          onclick={() => toggleTag(tag)}
        />
      {/each}
      <button
        type="button"
        class="rounded-full border border-dashed border-stone-300 px-2.5 py-1 text-xs text-stone-500 hover:border-amber-500 hover:text-amber-700 dark:border-stone-700 dark:hover:text-amber-400"
        onclick={() => (newTagOpen = !newTagOpen)}
      >
        ＋ 新建标签
      </button>
      {#if selectedTags.length}
        <span class="ml-auto text-xs text-stone-400">{selectedTags.length}/20</span>
      {/if}
    </div>
    {#if newTagOpen}
      <form onsubmit={createTag} class="mt-3 flex flex-wrap items-center gap-2 border-t border-stone-100 pt-3 dark:border-stone-800">
        <input
          class="min-w-40 flex-1 rounded-lg border border-stone-300 bg-white px-3 py-1.5 text-sm outline-none focus:border-amber-500 dark:border-stone-700 dark:bg-stone-950"
          placeholder="标签名（1–32 字符）"
          maxlength="32"
          bind:value={newTagName}
          required
        />
        <input
          type="color"
          class="h-9 w-12 cursor-pointer rounded border border-stone-300 bg-transparent dark:border-stone-700"
          bind:value={newTagColor}
          aria-label="标签颜色"
        />
        <button
          type="submit"
          disabled={creatingTag}
          class="rounded-lg bg-stone-900 px-3 py-1.5 text-sm text-white disabled:opacity-50 dark:bg-stone-100 dark:text-stone-900"
        >
          {creatingTag ? '创建中…' : '创建并选中'}
        </button>
      </form>
    {/if}
  </section>

  <div class="grid gap-4 lg:grid-cols-2">
    <section>
      <h2 class="mb-2 text-xs font-medium uppercase tracking-wide text-stone-400">编辑</h2>
      <MarkdownEditor bind:value={content} />
    </section>
    <section>
      <h2 class="mb-2 text-xs font-medium uppercase tracking-wide text-stone-400">预览</h2>
      <div class="min-h-[55vh] rounded-xl border border-stone-200 bg-white p-5 dark:border-stone-800 dark:bg-stone-900">
        {#if content.trim()}
          <DebouncedMarkdown source={content} />
        {:else}
          <p class="text-sm text-stone-400">预览会显示在这里。</p>
        {/if}
      </div>
    </section>
  </div>
{/if}
