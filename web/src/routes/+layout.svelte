<script lang="ts">
  import '../app.css';
  import { goto } from '$app/navigation';
  import { page } from '$app/state';
  import { onMount } from 'svelte';
  import { auth, bootstrapAuth, isPublicPath, logout } from '#lib/stores/auth';

  let { children } = $props();

  onMount(() => {
    void bootstrapAuth();
  });

  const state = $derived($auth);
  const pathname = $derived(page.url.pathname);

  $effect(() => {
    if (state.status === 'anonymous' && !isPublicPath(pathname)) {
      void goto(`/login?next=${encodeURIComponent(pathname)}`, { replaceState: true });
    }
    if (
      state.status === 'authenticated' &&
      (pathname === '/login' || pathname === '/register')
    ) {
      void goto('/', { replaceState: true });
    }
  });

  const navItems = [
    { href: '/', label: '日历' },
    { href: '/tags', label: '标签' },
    { href: '/data', label: '数据' },
    { href: '/settings', label: '设置' }
  ];

  async function handleLogout() {
    await logout();
    await goto('/login');
  }
</script>

{#if state.status === 'loading'}
  <div class="flex min-h-screen items-center justify-center">
    <div class="flex items-center gap-3 text-stone-500">
      <span class="h-4 w-4 animate-spin rounded-full border-2 border-stone-300 border-t-amber-600"></span>
      正在恢复登录状态…
    </div>
  </div>
{:else}
  {#if state.status === 'authenticated' && pathname !== '/viewer'}
    <header class="border-b border-stone-200 bg-white/80 backdrop-blur dark:border-stone-800 dark:bg-stone-950/80">
      <div class="mx-auto flex max-w-5xl items-center gap-4 px-4 py-3">
        <a href="/" class="flex items-center gap-2 font-semibold tracking-tight">
          <span class="text-xl">📔</span>
          <span>diary</span>
        </a>
        <nav class="flex flex-1 items-center gap-1 text-sm">
          {#each navItems as item}
            <a
              href={item.href}
              class="rounded-lg px-3 py-1.5 transition hover:bg-stone-100 dark:hover:bg-stone-900"
              class:bg-stone-100={pathname === item.href}
              class:dark:bg-stone-900={pathname === item.href}
              class:font-medium={pathname === item.href}
            >
              {item.label}
            </a>
          {/each}
        </nav>
        <div class="flex items-center gap-3 text-sm">
          <span class="hidden text-stone-500 sm:inline">{state.user.username}</span>
          <button
            type="button"
            class="rounded-lg border border-stone-300 px-3 py-1.5 text-stone-600 transition hover:bg-stone-100 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-900"
            onclick={handleLogout}
          >
            退出
          </button>
        </div>
      </div>
    </header>
  {/if}
  <main class="mx-auto w-full max-w-5xl px-4 py-6">
    {@render children()}
  </main>
{/if}
