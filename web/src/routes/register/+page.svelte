<script lang="ts">
  import { goto } from '$app/navigation';
  import { ApiError, api } from '#lib/api';
  import { passwordHint } from '#lib/utils/password';

  let username = $state('');
  let password = $state('');
  let confirm = $state('');
  let error = $state<string | null>(null);
  let loading = $state(false);
  const hint = $derived(passwordHint(password));

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    error = null;
    if (password !== confirm) {
      error = '两次输入的密码不一致';
      return;
    }
    loading = true;
    try {
      await api.register(username, password);
      await goto('/login?registered=1', { replaceState: true });
    } catch (cause) {
      error = cause instanceof ApiError ? cause.message : '注册失败，请稍后再试';
    } finally {
      loading = false;
    }
  }
</script>

<svelte:head><title>注册 · diary</title></svelte:head>

<div class="mx-auto mt-16 max-w-sm">
  <div class="mb-8 text-center">
    <div class="text-4xl">📔</div>
    <h1 class="mt-3 text-2xl font-bold">创建账号</h1>
    <p class="mt-1 text-sm text-stone-500">没有邮箱，也没有密码重置，请妥善保管密码。</p>
  </div>

  {#if error}
    <p class="mb-4 rounded-lg bg-rose-50 px-4 py-3 text-sm text-rose-800 dark:bg-rose-950 dark:text-rose-200" role="alert">
      {error}
    </p>
  {/if}

  <form onsubmit={submit} class="space-y-4 rounded-2xl border border-stone-200 bg-white p-6 shadow-sm dark:border-stone-800 dark:bg-stone-900">
    <label class="block text-sm">
      <span class="mb-1 block text-stone-600 dark:text-stone-300">用户名</span>
      <input
        class="w-full rounded-lg border border-stone-300 bg-white px-3 py-2 outline-none focus:border-amber-500 dark:border-stone-700 dark:bg-stone-950"
        autocomplete="username"
        minlength="3"
        maxlength="32"
        pattern="[A-Za-z0-9_.\-]+"
        title="3–32 位字母、数字、下划线、点或短横线"
        bind:value={username}
        required
      />
      <span class="mt-1 block text-xs text-stone-500">3–32 位字母、数字、下划线、点或短横线</span>
    </label>
    <label class="block text-sm">
      <span class="mb-1 block text-stone-600 dark:text-stone-300">密码</span>
      <input
        type="password"
        class="w-full rounded-lg border border-stone-300 bg-white px-3 py-2 outline-none focus:border-amber-500 dark:border-stone-700 dark:bg-stone-950"
        autocomplete="new-password"
        minlength="8"
        maxlength="128"
        bind:value={password}
        required
      />
      <span class="mt-1 block text-xs text-stone-500">
        至少 8 个字符，不能是常见弱口令{#if hint}<span class="ml-2 {hint.tone}">强度：{hint.label}</span>{/if}
      </span>
    </label>
    <label class="block text-sm">
      <span class="mb-1 block text-stone-600 dark:text-stone-300">确认密码</span>
      <input
        type="password"
        class="w-full rounded-lg border border-stone-300 bg-white px-3 py-2 outline-none focus:border-amber-500 dark:border-stone-700 dark:bg-stone-950"
        autocomplete="new-password"
        bind:value={confirm}
        required
      />
    </label>
    <button
      type="submit"
      disabled={loading}
      class="w-full rounded-lg bg-amber-600 px-4 py-2 font-medium text-white transition hover:bg-amber-700 disabled:opacity-50"
    >
      {loading ? '创建中…' : '创建账号'}
    </button>
  </form>
  <p class="mt-4 text-center text-sm text-stone-500">
    已有账号？<a class="text-amber-700 hover:underline dark:text-amber-400" href="/login">登录</a>
  </p>
</div>
