<script lang="ts">
  import { goto } from '$app/navigation';
  import { ApiError, api, type Setup2fa } from '#lib/api';
  import { auth, loadCurrentUser } from '#lib/stores/auth';
  import QRCode from 'qrcode';

  const user = $derived($auth.status === 'authenticated' ? $auth.user : null);

  let setup = $state<Setup2fa | null>(null);
  let qrDataUrl = $state('');
  let enableCode = $state('');
  let setupLoading = $state(false);
  let enableLoading = $state(false);
  let disablePassword = $state('');
  let disableLoading = $state(false);
  let error = $state<string | null>(null);
  let notice = $state<string | null>(null);

  let currentPassword = $state('');
  let newPassword = $state('');
  let confirmPassword = $state('');
  let passwordTotp = $state('');
  let passwordLoading = $state(false);

  async function startSetup() {
    error = null;
    notice = null;
    setupLoading = true;
    try {
      setup = await api.setup2fa();
      qrDataUrl = await QRCode.toDataURL(setup.otpauth_uri, {
        margin: 1,
        width: 240,
        color: { dark: '#1c1917', light: '#ffffff' }
      });
    } catch (cause) {
      error = cause instanceof ApiError ? cause.message : '获取两步验证密钥失败';
    } finally {
      setupLoading = false;
    }
  }

  async function enable(event: SubmitEvent) {
    event.preventDefault();
    if (!setup) return;
    enableLoading = true;
    error = null;
    notice = null;
    try {
      await api.enable2fa(enableCode);
      await loadCurrentUser();
      setup = null;
      qrDataUrl = '';
      enableCode = '';
      notice = '两步验证已开启。请确认你已保存 otpauth URI。';
    } catch (cause) {
      error = cause instanceof ApiError ? cause.message : '开启两步验证失败';
    } finally {
      enableLoading = false;
    }
  }

  async function disable(event: SubmitEvent) {
    event.preventDefault();
    disableLoading = true;
    error = null;
    notice = null;
    try {
      const result = await api.disable2fa(disablePassword);
      disablePassword = '';
      await loadCurrentUser();
      notice = `已排期关闭两步验证，生效时间：${new Date(result.effective_at).toLocaleString()}`;
    } catch (cause) {
      error = cause instanceof ApiError ? cause.message : '排期失败';
    } finally {
      disableLoading = false;
    }
  }

  async function cancelDisable() {
    error = null;
    notice = null;
    try {
      await api.cancelDisable2fa();
      await loadCurrentUser();
      notice = '已取消关闭两步验证。';
    } catch (cause) {
      error = cause instanceof ApiError ? cause.message : '取消失败';
    }
  }

  async function changePassword(event: SubmitEvent) {
    event.preventDefault();
    error = null;
    notice = null;
    if (newPassword !== confirmPassword) {
      error = '两次输入的新密码不一致';
      return;
    }
    passwordLoading = true;
    try {
      await api.changePassword(
        currentPassword,
        newPassword,
        user?.totp_enabled ? passwordTotp : undefined
      );
      auth.set({ status: 'anonymous' });
      await goto('/login?changed=1', { replaceState: true });
    } catch (cause) {
      error = cause instanceof ApiError ? cause.message : '修改密码失败';
    } finally {
      passwordLoading = false;
    }
  }

  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      notice = '已复制到剪贴板';
    } catch {
      error = '复制失败，请手动选择文本';
    }
  }
</script>

<svelte:head><title>设置 · diary</title></svelte:head>

<h1 class="text-2xl font-bold tracking-tight">设置</h1>
<p class="mt-0.5 text-sm text-stone-500">账号：{user?.username ?? ''}</p>

{#if error}
  <p class="mt-4 rounded-lg bg-rose-50 px-4 py-3 text-sm text-rose-800 dark:bg-rose-950 dark:text-rose-200" role="alert">{error}</p>
{/if}
{#if notice}
  <p class="mt-4 rounded-lg bg-emerald-50 px-4 py-3 text-sm text-emerald-800 dark:bg-emerald-950 dark:text-emerald-200">{notice}</p>
{/if}

<section class="mt-6 rounded-2xl border border-stone-200 bg-white p-5 dark:border-stone-800 dark:bg-stone-900">
  <div class="flex flex-wrap items-start justify-between gap-3">
    <div>
      <h2 class="font-semibold">两步验证（TOTP）</h2>
      <p class="mt-1 text-sm text-stone-500">
        30 秒一步、6 位验证码，允许 ±30 秒。验证器密钥只在服务器加密保存。
      </p>
    </div>
    <span class="rounded-full px-2.5 py-1 text-xs {user?.totp_enabled
      ? 'bg-emerald-100 text-emerald-800 dark:bg-emerald-950 dark:text-emerald-200'
      : 'bg-stone-100 text-stone-600 dark:bg-stone-800 dark:text-stone-300'}">
      {user?.totp_enabled ? '已开启' : '未开启'}
    </span>
  </div>

  {#if user?.twofa_disable_at}
    <div class="mt-4 rounded-xl border border-amber-200 bg-amber-50 p-4 text-sm dark:border-amber-900 dark:bg-amber-950/40">
      <p class="font-medium text-amber-900 dark:text-amber-100">两步验证已排期关闭</p>
      <p class="mt-1 text-amber-800 dark:text-amber-200">
        生效时间：{new Date(user.twofa_disable_at).toLocaleString()}。在此之前登录仍需验证码。
      </p>
      <button
        type="button"
        class="mt-3 rounded-lg border border-amber-300 px-3 py-1.5 text-sm text-amber-900 hover:bg-amber-100 dark:border-amber-800 dark:text-amber-100 dark:hover:bg-amber-900"
        onclick={cancelDisable}
      >
        取消关闭
      </button>
    </div>
  {/if}

  {#if !user?.totp_enabled}
    {#if setup}
      <div class="mt-5 grid gap-5 md:grid-cols-[240px_1fr]">
        <div class="rounded-xl bg-white p-2">
          {#if qrDataUrl}
            <img src={qrDataUrl} alt="TOTP 二维码" class="h-60 w-60" />
          {/if}
        </div>
        <div class="min-w-0 text-sm">
          <p class="text-stone-600 dark:text-stone-300">
            用验证器应用扫描二维码，或手动输入密钥。开启后请务必保存下面的 URI，它是唯一备份。
          </p>
          <div class="mt-3">
            <p class="text-xs text-stone-400">Base32 密钥</p>
            <div class="mt-1 flex items-center gap-2">
              <code class="min-w-0 flex-1 break-all rounded-lg bg-stone-100 px-3 py-2 text-xs dark:bg-stone-950">{setup.secret_base32}</code>
              <button type="button" class="rounded-lg border border-stone-300 px-2 py-1 text-xs dark:border-stone-700" onclick={() => copy(setup!.secret_base32)}>复制</button>
            </div>
          </div>
          <div class="mt-3">
            <p class="text-xs text-stone-400">otpauth URI</p>
            <div class="mt-1 flex items-start gap-2">
              <code class="min-w-0 flex-1 break-all rounded-lg bg-stone-100 px-3 py-2 text-xs dark:bg-stone-950">{setup.otpauth_uri}</code>
              <button type="button" class="rounded-lg border border-stone-300 px-2 py-1 text-xs dark:border-stone-700" onclick={() => copy(setup!.otpauth_uri)}>复制</button>
            </div>
          </div>
          <form onsubmit={enable} class="mt-4 flex items-end gap-2">
            <label class="text-sm">
              <span class="mb-1 block text-stone-600 dark:text-stone-300">6 位验证码</span>
              <input
                class="w-40 rounded-lg border border-stone-300 bg-white px-3 py-2 text-center font-mono text-xl tracking-[0.3em] outline-none focus:border-amber-500 dark:border-stone-700 dark:bg-stone-950"
                inputmode="numeric"
                maxlength="6"
                bind:value={enableCode}
                required
              />
            </label>
            <button
              type="submit"
              disabled={enableLoading}
              class="rounded-lg bg-amber-600 px-4 py-2 text-sm font-medium text-white hover:bg-amber-700 disabled:opacity-50"
            >
              {enableLoading ? '验证中…' : '验证并开启'}
            </button>
          </form>
        </div>
      </div>
    {:else}
      <button
        type="button"
        class="mt-4 rounded-lg bg-amber-600 px-4 py-2 text-sm font-medium text-white hover:bg-amber-700 disabled:opacity-50"
        disabled={setupLoading}
        onclick={startSetup}
      >
        {setupLoading ? '生成中…' : '开启两步验证'}
      </button>
    {/if}
  {:else}
    <form onsubmit={disable} class="mt-5 flex flex-wrap items-end gap-3 border-t border-stone-100 pt-4 dark:border-stone-800">
      <label class="min-w-52 flex-1 text-sm">
        <span class="mb-1 block text-stone-600 dark:text-stone-300">关闭两步验证需要密码（7 天冷静期）</span>
        <input
          type="password"
          class="w-full rounded-lg border border-stone-300 bg-white px-3 py-2 outline-none focus:border-amber-500 dark:border-stone-700 dark:bg-stone-950"
          autocomplete="current-password"
          bind:value={disablePassword}
          required
        />
      </label>
      <button
        type="submit"
        disabled={disableLoading || Boolean(user?.twofa_disable_at)}
        class="rounded-lg border border-rose-300 px-4 py-2 text-sm text-rose-700 hover:bg-rose-50 disabled:opacity-50 dark:border-rose-900 dark:text-rose-300 dark:hover:bg-rose-950"
      >
        {disableLoading ? '提交中…' : '排期关闭'}
      </button>
    </form>
    <p class="mt-2 text-xs text-stone-400">没有恢复码。丢手机后只能凭密码等待 7 天后自动关闭。</p>
  {/if}
</section>

<section class="mt-6 rounded-2xl border border-stone-200 bg-white p-5 dark:border-stone-800 dark:bg-stone-900">
  <h2 class="font-semibold">修改密码</h2>
  <p class="mt-1 text-sm text-stone-500">修改后所有会话立即失效，需要重新登录。</p>
  <form onsubmit={changePassword} class="mt-4 grid gap-3 sm:grid-cols-2">
    <label class="text-sm sm:col-span-2">
      <span class="mb-1 block text-stone-600 dark:text-stone-300">当前密码</span>
      <input
        type="password"
        class="w-full rounded-lg border border-stone-300 bg-white px-3 py-2 outline-none focus:border-amber-500 dark:border-stone-700 dark:bg-stone-950"
        autocomplete="current-password"
        bind:value={currentPassword}
        required
      />
    </label>
    <label class="text-sm">
      <span class="mb-1 block text-stone-600 dark:text-stone-300">新密码</span>
      <input
        type="password"
        class="w-full rounded-lg border border-stone-300 bg-white px-3 py-2 outline-none focus:border-amber-500 dark:border-stone-700 dark:bg-stone-950"
        autocomplete="new-password"
        minlength="8"
        maxlength="128"
        bind:value={newPassword}
        required
      />
    </label>
    <label class="text-sm">
      <span class="mb-1 block text-stone-600 dark:text-stone-300">确认新密码</span>
      <input
        type="password"
        class="w-full rounded-lg border border-stone-300 bg-white px-3 py-2 outline-none focus:border-amber-500 dark:border-stone-700 dark:bg-stone-950"
        autocomplete="new-password"
        bind:value={confirmPassword}
        required
      />
    </label>
    {#if user?.totp_enabled}
      <label class="text-sm">
        <span class="mb-1 block text-stone-600 dark:text-stone-300">当前 TOTP 验证码</span>
        <input
          class="w-full rounded-lg border border-stone-300 bg-white px-3 py-2 font-mono tracking-widest outline-none focus:border-amber-500 dark:border-stone-700 dark:bg-stone-950"
          inputmode="numeric"
          maxlength="6"
          bind:value={passwordTotp}
          required
        />
      </label>
    {/if}
    <div class="sm:col-span-2">
      <button
        type="submit"
        disabled={passwordLoading}
        class="rounded-lg bg-stone-900 px-4 py-2 text-sm text-white hover:bg-stone-700 disabled:opacity-50 dark:bg-stone-100 dark:text-stone-900 dark:hover:bg-stone-300"
      >
        {passwordLoading ? '修改中…' : '修改密码'}
      </button>
    </div>
  </form>
</section>

<section class="mt-6 rounded-2xl border border-stone-200 bg-white p-5 text-sm text-stone-500 dark:border-stone-800 dark:bg-stone-900">
  <h2 class="font-semibold text-stone-900 dark:text-stone-100">关于账号恢复</h2>
  <p class="mt-2">没有邮箱通道，因此没有密码重置，也没有恢复码。数据库冷备份是唯一兜底。</p>
</section>
