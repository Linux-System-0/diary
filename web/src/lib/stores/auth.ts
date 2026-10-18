import { writable } from 'svelte/store';
import { api, type LoginResult, type User } from '../api';

export type AuthState =
  | { status: 'loading' }
  | { status: 'anonymous' }
  | { status: 'authenticated'; user: User };

export const auth = writable<AuthState>({ status: 'loading' });

let bootstrapPromise: Promise<void> | null = null;

export function bootstrapAuth(): Promise<void> {
  if (!bootstrapPromise) {
    bootstrapPromise = (async () => {
      const user = await api.restore();
      auth.set(user ? { status: 'authenticated', user } : { status: 'anonymous' });
    })().catch(() => {
      auth.set({ status: 'anonymous' });
    });
  }
  return bootstrapPromise;
}

export async function loginWithPassword(
  username: string,
  password: string
): Promise<LoginResult> {
  const result = await api.login(username, password);
  if (!result.mfa_required) {
    await loadCurrentUser();
  }
  return result;
}

export async function completeMfa(mfaToken: string, code: string): Promise<void> {
  await api.verifyMfa(mfaToken, code);
  await loadCurrentUser();
}

export async function loadCurrentUser(): Promise<User> {
  const user = await api.me();
  auth.set({ status: 'authenticated', user });
  return user;
}

export async function logout(): Promise<void> {
  try {
    await api.logout();
  } finally {
    auth.set({ status: 'anonymous' });
  }
}

export function isPublicPath(pathname: string): boolean {
  return (
    pathname === '/login' ||
    pathname === '/register' ||
    pathname === '/viewer' ||
    pathname.startsWith('/viewer/')
  );
}
