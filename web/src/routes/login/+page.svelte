<script lang="ts">
  import { goto } from '$app/navigation';
  import { page } from '$app/state';
  import { ApiError } from '#lib/api';
  import { completeMfa, loginWithPassword } from '#lib/stores/auth';

  let username = $state('');
  let password = $state('');
  let code = $state('');
  let mfaToken = $state<string | null>(null);
  let error = $state<string | null>(null);
  let loading = $state(false);

  const registered = $derived(page.url.searchParams.get('registered') === '1');
  const passwordChanged = $derived(page.url.searchParams.get('changed') === '1');
  const nextPath = $derived(page.url.searchParams.get('next') || '/');

  async function submitPassword(event: SubmitEvent) {
    event.preventDefault();
    error = null;
    loading = true;
    try {
      const result = await loginWithPassword(username, password);
      if (result.mfa_required && result.mfa_token) {
        mfaToken = result.mfa_token;
        code = '';
        return;
      }
      await goto(nextPath, { replaceState: true });
    } catch (cause) {
      error = cause instanceof ApiError ? cause.message : '登录失败，请稍后再试';
    } finally {
      loading = false;
    }
  }

  async function submitMfa(event: SubmitEvent) {
    event.preventDefault();
    if (!mfaToken) return;
    error = null;
    loading = true;
    try {
      await completeMfa(mfaToken, code);
      await goto(nextPath, { replaceState: true });
    } catch (cause) {
      error = cause instanceof ApiError ? cause.message : '验证失败，请稍后再试';
    } finally {
      loading = false;
    }
  }
</script>

<svelte:head><title>登录 · diary</title></svelte:head>

<div class="mx-auto mt-16 max-w-sm">
  <div class="mb-8 text-center">
    <div class="text-4xl">📔</div>
    <h1 class="mt-3 text-2xl font-bold">diary</h1>
    <p class="mt-1 text-sm text-stone-500">你的私人日记本</p>
  </div>

  {#if registered}
    <p class="mb-4 rounded-lg bg-emerald-50 px-4 py-3 text-sm text-emerald-800 dark:bg-emerald-950 dark:text-emerald-200">
      注册成功，请登录。
    </p>
  {/if}
  {#if passwordChanged}
    <p class="mb-4 rounded-lg bg-emerald-50 px-4 py-3 text-sm text-emerald-800 dark:bg-emerald-950 dark:text-emerald-200">
      密码已修改，所有旧会话已失效，请重新登录。
    </p>
  {/if}

  {#if error}
    <p class="mb-4 rounded-lg bg-rose-50 px-4 py-3 text-sm text-rose-800 dark:bg-rose-950 dark:text-rose-200" role="alert">
      {error}
    </p>
  {/if}

  {#if mfaToken}
    <form onsubmit={submitMfa} class="space-y-4 rounded-2xl border border-stone-200 bg-white p-6 shadow-sm dark:border-stone-800 dark:bg-stone-900">
      <div>
        <h2 class="font-semibold">两步验证</h2>
        <p class="mt-1 text-sm text-stone-500">输入验证器应用中的 6 位验证码。</p>
      </div>
      <label class="block text-sm">
        <span class="mb-1 block text-stone-600 dark:text-stone-300">验证码</span>
        <input
          class="w-full rounded-lg border border-stone-300 bg-white px-3 py-2 text-center font-mono text-2xl tracking-[0.4em] outline-none focus:border-amber-500 dark:border-stone-700 dark:bg-stone-950"
          inputmode="numeric"
          autocomplete="one-time-code"
          maxlength="6"
          bind:value={code}
          required
        />
      </label>
      <button
        type="submit"
        disabled={loading}
        class="w-full rounded-lg bg-amber-600 px-4 py-2 font-medium text-white transition hover:bg-amber-700 disabled:opacity-50"
      >
        {loading ? '验证中…' : '验证并登录'}
      </button>
      <button
        type="button"
        class="w-full text-sm text-stone-500 hover:text-stone-800 dark:hover:text-stone-200"
        onclick={() => { mfaToken = null; error = null; }}
      >
        返回
      </button>
    </form>
  {:else}
    <form onsubmit={submitPassword} class="space-y-4 rounded-2xl border border-stone-200 bg-white p-6 shadow-sm dark:border-stone-800 dark:bg-stone-900">
      <label class="block text-sm">
        <span class="mb-1 block text-stone-600 dark:text-stone-300">用户名</span>
        <input
          class="w-full rounded-lg border border-stone-300 bg-white px-3 py-2 outline-none focus:border-amber-500 dark:border-stone-700 dark:bg-stone-950"
          autocomplete="username"
          bind:value={username}
          required
        />
      </label>
      <label class="block text-sm">
        <span class="mb-1 block text-stone-600 dark:text-stone-300">密码</span>
        <input
          type="password"
          class="w-full rounded-lg border border-stone-300 bg-white px-3 py-2 outline-none focus:border-amber-500 dark:border-stone-700 dark:bg-stone-950"
          autocomplete="current-password"
          bind:value={password}
          required
        />
      </label>
      <button
        type="submit"
        disabled={loading}
        class="w-full rounded-lg bg-amber-600 px-4 py-2 font-medium text-white transition hover:bg-amber-700 disabled:opacity-50"
      >
        {loading ? '登录中…' : '登录'}
      </button>
    </form>
    <p class="mt-4 text-center text-sm text-stone-500">
      还没有账号？<a class="text-amber-700 hover:underline dark:text-amber-400" href="/register">注册</a>
      <span class="mx-2">·</span>
      <a class="text-amber-700 hover:underline dark:text-amber-400" href="/viewer">离线查看 .diary</a>
    </p>
  {/if}
</div>
