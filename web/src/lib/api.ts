export type User = {
  id: number;
  username: string;
  totp_enabled: boolean;
  twofa_disable_at: string | null;
};

export type Tag = {
  id: number;
  name: string;
  color: string | null;
  emoji: string | null;
  sort_order: number;
  archived: boolean;
};

export type EntryTag = {
  id: number;
  name: string;
  color: string | null;
  emoji: string | null;
};

export type Entry = {
  date: string;
  content_md: string;
  tags: EntryTag[];
  created_at: string;
  updated_at: string;
};

export type DiaryPayloadJson = {
  format: string;
  version: number;
  exported_at?: string;
  app?: string;
  timezone?: string;
  tags: Array<{ name: string; color?: string | null; emoji?: string | null }>;
  entries: Array<{
    date: string;
    content_md: string;
    tags: string[];
    created_at?: string;
    updated_at?: string;
  }>;
};

export type ImportPreview = {
  import_id: string;
  summary: { create: number; update: number; tags_create: number };
  tags_to_create: Array<{ name: string; color?: string | null; emoji?: string | null }>;
  dates: { create: string[]; update: string[] };
  warnings: string[];
};

export type ImportCommit = {
  created: number;
  updated: number;
  tags_created: number;
};

export type LoginResult = {
  access_token?: string;
  expires_in: number;
  mfa_required?: boolean;
  mfa_token?: string;
};

export type Setup2fa = {
  otpauth_uri: string;
  secret_base32: string;
};

export class ApiError extends Error {
  constructor(
    public readonly code: string,
    message: string,
    public readonly status: number,
    public readonly details?: unknown
  ) {
    super(message);
    this.name = 'ApiError';
  }
}

const BASE = (import.meta.env.VITE_API_BASE ?? '').replace(/\/$/, '');
const PREFIX = `${BASE}/api/v1`;

let accessToken: string | null = null;
let refreshPromise: Promise<boolean> | null = null;

export function setAccessToken(token: string | null) {
  accessToken = token;
}

export function getAccessToken(): string | null {
  return accessToken;
}

type FetchOptions = RequestInit & { retryOn401?: boolean };

async function request<T>(path: string, init: FetchOptions = {}): Promise<T> {
  const { retryOn401 = true, ...rest } = init;
  const headers = new Headers(rest.headers);
  if (rest.body !== undefined && !headers.has('content-type')) {
    headers.set('content-type', 'application/json');
  }
  if (accessToken) {
    headers.set('authorization', `Bearer ${accessToken}`);
  }

  let response = await fetch(`${PREFIX}${path}`, {
    ...rest,
    headers,
    credentials: 'include'
  });

  if (response.status === 401 && retryOn401 && path !== '/auth/refresh') {
    const refreshed = await refreshAccessToken();
    if (refreshed) {
      const retryHeaders = new Headers(rest.headers);
      if (rest.body !== undefined && !retryHeaders.has('content-type')) {
        retryHeaders.set('content-type', 'application/json');
      }
      if (accessToken) {
        retryHeaders.set('authorization', `Bearer ${accessToken}`);
      }
      response = await fetch(`${PREFIX}${path}`, {
        ...rest,
        headers: retryHeaders,
        credentials: 'include'
      });
    }
  }

  if (!response.ok) {
    throw await toApiError(response);
  }
  if (response.status === 204) {
    return undefined as T;
  }
  const contentType = response.headers.get('content-type') ?? '';
  if (!contentType.includes('application/json')) {
    return (await response.text()) as T;
  }
  return (await response.json()) as T;
}

async function refreshAccessToken(): Promise<boolean> {
  if (refreshPromise) {
    return refreshPromise;
  }
  refreshPromise = (async () => {
    const response = await fetch(`${PREFIX}/auth/refresh`, {
      method: 'POST',
      credentials: 'include'
    });
    if (!response.ok) {
      accessToken = null;
      return false;
    }
    const data = (await response.json()) as { access_token: string };
    accessToken = data.access_token;
    return true;
  })();
  try {
    return await refreshPromise;
  } catch {
    accessToken = null;
    return false;
  } finally {
    refreshPromise = null;
  }
}

async function toApiError(response: Response): Promise<ApiError> {
  try {
    const body = (await response.json()) as {
      error?: { code?: string; message?: string; details?: unknown };
    };
    return new ApiError(
      body.error?.code ?? 'http_error',
      body.error?.message ?? `请求失败 (${response.status})`,
      response.status,
      body.error?.details
    );
  } catch {
    return new ApiError('http_error', `请求失败 (${response.status})`, response.status);
  }
}

function body(value: unknown): string {
  return JSON.stringify(value);
}

export const api = {
  async register(username: string, password: string): Promise<void> {
    await request('/auth/register', {
      method: 'POST',
      body: body({ username, password }),
      retryOn401: false
    });
  },

  async login(username: string, password: string): Promise<LoginResult> {
    const result = await request<LoginResult>('/auth/login', {
      method: 'POST',
      body: body({ username, password }),
      retryOn401: false
    });
    if (result.access_token) {
      accessToken = result.access_token;
    }
    return result;
  },

  async verifyMfa(mfaToken: string, code: string): Promise<LoginResult> {
    const result = await request<LoginResult>('/auth/2fa/verify', {
      method: 'POST',
      body: body({ mfa_token: mfaToken, code }),
      retryOn401: false
    });
    if (result.access_token) {
      accessToken = result.access_token;
    }
    return result;
  },

  async restore(): Promise<User | null> {
    if (!(await refreshAccessToken())) {
      return null;
    }
    try {
      return await request<User>('/auth/me', { retryOn401: false });
    } catch {
      accessToken = null;
      return null;
    }
  },

  async refresh(): Promise<boolean> {
    return refreshAccessToken();
  },

  async me(): Promise<User> {
    return request<User>('/auth/me');
  },

  async logout(): Promise<void> {
    try {
      await request('/auth/logout', { method: 'POST', retryOn401: false });
    } finally {
      accessToken = null;
    }
  },

  async changePassword(
    currentPassword: string,
    newPassword: string,
    totpCode?: string
  ): Promise<void> {
    await request('/auth/password', {
      method: 'POST',
      body: body({
        current_password: currentPassword,
        new_password: newPassword,
        totp_code: totpCode || undefined
      }),
      retryOn401: false
    });
    accessToken = null;
  },

  async setup2fa(): Promise<Setup2fa> {
    return request<Setup2fa>('/auth/2fa/setup');
  },

  async enable2fa(code: string): Promise<void> {
    await request('/auth/2fa/enable', { method: 'POST', body: body({ code }) });
  },

  async disable2fa(password: string): Promise<{ effective_at: string }> {
    return request<{ effective_at: string }>('/auth/2fa/disable', {
      method: 'POST',
      body: body({ password })
    });
  },

  async cancelDisable2fa(): Promise<void> {
    await request('/auth/2fa/disable/cancel', { method: 'POST' });
  },

  async listTags(includeArchived = false): Promise<Tag[]> {
    return request<Tag[]>(`/tags?include_archived=${includeArchived ? 'true' : 'false'}`);
  },

  async createTag(input: {
    name: string;
    color?: string | null;
    emoji?: string | null;
    sort_order?: number;
  }): Promise<Tag> {
    return request<Tag>('/tags', { method: 'POST', body: body(input) });
  },

  async updateTag(
    id: number,
    input: {
      name?: string;
      color?: string | null;
      emoji?: string | null;
      sort_order?: number;
    }
  ): Promise<Tag> {
    return request<Tag>(`/tags/${id}`, { method: 'PATCH', body: body(input) });
  },

  async deleteTag(id: number): Promise<void> {
    await request(`/tags/${id}`, { method: 'DELETE' });
  },

  async listEntries(from: string, to: string): Promise<Entry[]> {
    return request<Entry[]>(`/entries?from=${from}&to=${to}`);
  },

  async getEntry(date: string): Promise<Entry> {
    return request<Entry>(`/entries/${date}`);
  },

  async putEntry(
    date: string,
    contentMd: string,
    tagIds: number[],
    baseUpdatedAt?: string
  ): Promise<Entry> {
    return request<Entry>(`/entries/${date}`, {
      method: 'PUT',
      body: body({
        content_md: contentMd,
        tag_ids: tagIds,
        base_updated_at: baseUpdatedAt || undefined
      })
    });
  },

  async deleteEntry(date: string): Promise<void> {
    await request(`/entries/${date}`, { method: 'DELETE' });
  },

  async prepareExport(
    scope: 'all' | 'dates',
    dates?: string[]
  ): Promise<DiaryPayloadJson> {
    return request<DiaryPayloadJson>('/export/prepare', {
      method: 'POST',
      body: body({ scope, dates })
    });
  },

  async previewImport(payload: {
    entries: DiaryPayloadJson['entries'];
    tags: DiaryPayloadJson['tags'];
    scope: 'all' | 'dates';
    dates?: string[];
  }): Promise<ImportPreview> {
    return request<ImportPreview>('/import/preview', {
      method: 'POST',
      body: body(payload)
    });
  },

  async commitImport(
    importId: string,
    scope: 'all' | 'dates',
    dates?: string[]
  ): Promise<ImportCommit> {
    return request<ImportCommit>('/import/commit', {
      method: 'POST',
      body: body({ import_id: importId, scope, dates })
    });
  }
};
