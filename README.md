# diary

一个自托管的日记本：**Rust 后端 + SvelteKit 前端**，SQLite 单文件存储（SQLite 静态编译进二进制，不依赖目标系统的 sqlite），支持账号密码登录、TOTP 两步验证、自定义标签、Markdown 正文，以及前端加密的 `.diary` 导入/导出。

> **状态：设计规格（v0.1），尚未实现。**
> 本文档是实现的唯一事实来源（single source of truth）。代码与本文档冲突时，先改文档。
> 未标记「待定」的条目均已确定，实现时不应自行更改。

---

## 目录

1. [目标与非目标](#1-目标与非目标)
2. [技术栈](#2-技术栈)
3. [架构与部署拓扑](#3-架构与部署拓扑)
4. [配置](#4-配置)
5. [数据模型](#5-数据模型)
6. [认证与安全](#6-认证与安全)
7. [标签](#7-标签)
8. [日记与 Markdown](#8-日记与-markdown)
9. [`.diary` 文件格式规范 v1](#9-diary-文件格式规范-v1)
10. [导出](#10-导出)
11. [查看 `.diary`](#11-查看-diary)
12. [导入](#12-导入)
13. [API 参考](#13-api-参考)
14. [错误与限流](#14-错误与限流)
15. [安全清单](#15-安全清单)
16. [已知限制](#16-已知限制)
17. [测试计划](#17-测试计划)
18. [里程碑](#18-里程碑)
19. [决策记录](#19-决策记录)

---

## 1. 目标与非目标

### 目标

- 单用户/小规模多用户的私人日记，日历视图，一天一篇。
- 正文为 **Markdown 源码**，编辑时实时预览，展示时渲染并严格清洗。
- 用**自定义标签**代替固定心情量表（先建标签，才能在日记上使用）。
- 账号密码登录 + **TOTP 两步验证**（30s 步长、6 位、允许 ±30s）。
- 导出 `.diary` 归档文件，**在浏览器端**用密码做 AES-256 加密；密码永不出浏览器。
- 可离线查看 `.diary`（不上传、不登录）。
- 导入支持"文件中出现过的日期整体覆盖"，文件中没有的日期不动。

### 非目标（v1 明确不做）

- ❌ 全文搜索（中文分词成本高于收益；个人日记量级下不值得）。
- ❌ 多设备会话管理 UI、设备列表、"登出所有设备"按钮。
- ❌ 恢复码、邮箱验证、密码重置（没有邮件通道）。
- ❌ 附件/图片上传。Markdown 里只能引用外链，且**默认不自动加载**（点击才加载，防追踪像素与 IP 泄漏）。
- ❌ 多用户共享/协作、公开日记、评论。
- ❌ 服务端渲染（SEO 无意义，纯 SPA）。
- ❌ 数据库文件加密（SQLCipher 需要额外 C 依赖，与"零依赖分发"冲突；靠文件权限 + 备份保护）。
- ❌ 标签改名兼容（旧名视为新标签，见 §19）。

---

## 2. 技术栈

### 后端（`Cargo.toml`, package `diary`, edition 2024）

| 用途 | crate | 备注 |
|---|---|---|
| Web | `axum` 0.8 | + `tokio`、`tower`、`tower-http` |
| HTTP 中间件 | `tower-http` | `trace`, `cors`, `limit`, `timeout` |
| 数据库 | `sqlx` 0.8 (`runtime-tokio`, `sqlite`, `macros`, `migrate`, `time`) | **必须静态编译 SQLite**，见下 |
| 密码哈希 | `argon2` + `password-hash` | Argon2id，PHC 字符串存库 |
| JWT | `jsonwebtoken` | HS256（单服务） |
| TOTP | `totp-rs` | `Algorithm::SHA1, digits=6, step=30, skew=1` |
| 对称加密（TOTP secret） | `chacha20poly1305` (XChaCha20-Poly1305) | 服务器主密钥加密 |
| 机密内存 | `secrecy` + `zeroize` + `subtle` | 常量时间比较、用完擦除 |
| 随机 | `rand` (`OsRng`) / `getrandom` | salt / nonce / secret |
| 序列化 | `serde`, `serde_json` | `time` 用 RFC3339 |
| 限流 | `tower_governor` | 登录 / 2FA / 注册 |
| 时间 | `time` | `OffsetDateTime`，UTC 存储 |
| 杂项 | `uuid`, `base64`, `thiserror`, `anyhow`, `tracing`, `tracing-subscriber`, `validator` | |

**"不依赖目标系统 sqlite" 的落实**

```toml
sqlx = { version = "0.8", default-features = false, features = ["runtime-tokio", "sqlite", "macros", "migrate", "time"] }
# 确认 bundled：cargo tree -i libsqlite3-sys  →  features 中必须出现 "bundled"
```

- 构建时用 C 编译器把 SQLite 编进二进制，**运行时不需要系统 libsqlite3**。
- 完全静态分发：`cargo build --release --target x86_64-unknown-linux-musl`（`+crt-static`）。
- CI 里加一条断言：发布产物在**最小容器**（scratch/distroless）中能启动并跑迁移。

### 前端

| 用途 | 选择 |
|---|---|
| 框架 | SvelteKit + `adapter-static`（`ssr = false`，纯 SPA） |
| 语言 | TypeScript（`strict`） |
| 样式 | Tailwind CSS |
| Markdown | `markdown-it` + `markdown-it-task-lists` + `dompurify` |
| 编辑器 | CodeMirror 6（Markdown 模式） |
| 浏览器加密 | `hash-wasm`（Argon2id）+ WebCrypto（AES-GCM） |
| 二维码 | `qrcode`（本地生成，secret 不上传） |
| 日期 | `date-fns` |
| 校验 | `zod`（与后端错误码对齐） |
| 测试 | Vitest（含 `.diary` 测试向量往返） |

---

## 3. 架构与部署拓扑

```
┌──────────────────────────┐         ┌────────────────────────────┐
│ SvelteKit SPA            │  HTTPS  │ Rust (axum)  /api/v1       │
│ · UI / MD 编辑与预览      │◄───────►│ · 认证 / JWT / TOTP        │
│ · Argon2id + AES-256-GCM │  JSON   │ · 日记 / 标签 CRUD          │
│ · .diary 解析与加解密     │ Cookie  │ · 导入预检 / 导出打包       │
│ · 只读查看器（离线）      │         │ · SQLite (bundled, 单文件)  │
└──────────────────────────┘         └────────────────────────────┘
       静态托管（CDN/Nginx）                  独立进程
```

### 部署硬性要求

1. **必须 HTTPS**。`crypto.subtle` 只在 secure context 可用；局域网 HTTP 会让导出/查看全部失效。
2. **前后端必须 same-site**：
   - ✅ `app.example.com` + `api.example.com`（同 eTLD+1 → same-site，`SameSite=Strict` 的刷新 Cookie 可正常工作，只需配 CORS）。
   - ❌ `xxx.vercel.app` / `*.pages.dev` + `api.mydomain.com`（cross-site → 必须 `SameSite=None`，且 **Safari ITP / Firefox 第三方 Cookie 限制会拦截**，刷新会随机失效，极难排查）。
   - 若无法 same-site，替代方案：刷新令牌不放 Cookie，仅存内存 → 代价是刷新页面需重新登录（详见 §6.5）。
3. **CORS 白名单精确匹配**（禁止 `*` 配 credentials），并校验 `Origin`。
4. **开发期**：Vite `server.proxy` 把 `/api` 代理到后端，避免 CORS/Cookie 差异。
5. **CSP**：`default-src 'self'; script-src 'self'; connect-src 'self' 'https://api.example.com'; img-src 'self' data:; object-src 'none'; base-uri 'none'`，并用 `click-to-load` 放行外链图片域名（或干脆 `img-src 'self' data:` + 点击后由用户确认取消限制）。
6. **API 版本前缀 `/api/v1`**，新增字段保持向后兼容。

---

## 4. 配置

全部通过环境变量（可选 `.env`）。启动时校验：缺少必填项直接退出，密钥长度不足直接退出。

| 变量 | 必填 | 默认 | 说明 |
|---|---|---|---|
| `DIARY_BIND` | 否 | `127.0.0.1:8787` | 监听地址 |
| `DIARY_DB_PATH` | 是 | — | SQLite 文件路径（父目录需可写） |
| `DIARY_JWT_SECRET` | 是 | — | base64，**≥32 字节**；HS256 密钥 |
| `DIARY_MASTER_KEY` | 是 | — | base64，**32 字节**；加密 TOTP secret |
| `DIARY_CORS_ORIGINS` | 是 | — | 逗号分隔的完整 origin 白名单 |
| `DIARY_ALLOW_REGISTRATION` | 否 | `true` | 开放注册开关 |
| `DIARY_SECURE_COOKIE` | 否 | `true` | 生产必须 `true` |
| `DIARY_ACCESS_TTL_SECS` | 否 | `900` | access token 有效期（15 min） |
| `DIARY_REFRESH_TTL_SECS` | 否 | `2592000` | refresh token 有效期（30 天） |
| `DIARY_MFA_TOKEN_TTL_SECS` | 否 | `300` | mfa_token 有效期（5 min） |
| `DIARY_TOTP_SKEW` | 否 | `1` | ±1 step = ±30s |
| `DIARY_TWOFA_DISABLE_DELAY_DAYS` | 否 | `7` | 关闭 2FA 的冷静期 |
| `DIARY_MAX_ENTRY_BYTES` | 否 | `1048576` | 单篇正文上限（1 MiB） |
| `DIARY_MAX_EXPORT_BYTES` | 否 | `67108864` | 单次导出明文上限（64 MiB） |
| `DIARY_MAX_IMPORT_BYTES` | 否 | `67108864` | 单次导入明文上限（64 MiB） |
| `DIARY_MAX_TAGS` | 否 | `200` | 每用户标签上限 |
| `DIARY_MAX_TAGS_PER_ENTRY` | 否 | `20` | 每篇标签上限 |
| `DIARY_RUST_LOG` | 否 | `info` | `tracing` 过滤规则 |

**密钥轮换**：v1 只支持单一 `DIARY_MASTER_KEY`。换钥需一次性重加密脚本（`diary rekey-master`），不在 v1 范围内，但 `totp_secret_enc` 的密文前缀预留 `key_id` 字节（v1 恒为 `0`），以便将来扩展。

---

## 5. 数据模型

```sql
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;

CREATE TABLE users (
    id               INTEGER PRIMARY KEY,
    username         TEXT    NOT NULL,
    username_norm    TEXT    NOT NULL UNIQUE,   -- lower(trim(username))
    password_hash    TEXT    NOT NULL,          -- Argon2id PHC 字符串
    totp_secret_enc  BLOB,                      -- key_id(1) || XChaCha20-Poly1305 密文
    totp_enabled     INTEGER NOT NULL DEFAULT 0,
    totp_last_step   INTEGER NOT NULL DEFAULT 0,-- 防重放：已成功使用过的最大时间片序号
    totp_pending_at  INTEGER,                   -- 2FA 绑定待确认时的时间（未启用前暂存）
    twofa_disable_at INTEGER,                   -- 冷静期到期时间（NULL = 无待处理请求）
    token_version    INTEGER NOT NULL DEFAULT 0,-- 改密 / 强制下线时 +1
    created_at       INTEGER NOT NULL,
    updated_at       INTEGER NOT NULL
);

CREATE TABLE refresh_tokens (
    id          TEXT    PRIMARY KEY,            -- jti (uuid v4)
    user_id     INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash  TEXT    NOT NULL,               -- SHA-256(token) 十六进制
    expires_at  INTEGER NOT NULL,
    revoked_at  INTEGER,
    replaced_by TEXT,                           -- 轮转链：指向新 jti，用于复用检测
    created_at  INTEGER NOT NULL
);
CREATE INDEX idx_refresh_user ON refresh_tokens (user_id, expires_at);

CREATE TABLE tags (
    id         INTEGER PRIMARY KEY,
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name       TEXT    NOT NULL,
    name_norm  TEXT    NOT NULL,                -- lower(trim(name))
    color      TEXT,                            -- '#RRGGBB'，可空
    emoji      TEXT,                            -- 单个 grapheme，可空
    sort_order INTEGER NOT NULL DEFAULT 0,
    deleted_at INTEGER,                         -- 软删除，历史引用仍可解析
    created_at INTEGER NOT NULL,
    UNIQUE (user_id, name_norm)
);

CREATE TABLE entries (
    id          INTEGER PRIMARY KEY,
    user_id     INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    entry_date  TEXT    NOT NULL
                CHECK (entry_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    content_md  TEXT    NOT NULL DEFAULT '',
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL,
    imported_at INTEGER,                        -- 最近一次由导入写入的时间
    UNIQUE (user_id, entry_date)
);
CREATE INDEX idx_entries_user_date ON entries (user_id, entry_date);

CREATE TABLE entry_tags (
    entry_id INTEGER NOT NULL REFERENCES entries(id) ON DELETE CASCADE,
    tag_id   INTEGER NOT NULL REFERENCES tags(id)    ON DELETE CASCADE,
    PRIMARY KEY (entry_id, tag_id)
);
CREATE INDEX idx_entry_tags_tag ON entry_tags (tag_id);
```

约定：

- **时间**：所有 `*_at` 为 Unix 秒（UTC）。API 层用 RFC3339 字符串收发。
- **日期**：`entry_date` 是"日历日"字符串 `YYYY-MM-DD`，由前端按用户本地日期生成。后端只校验格式与合理性（拒绝早于 `1970-01-01`、晚于 UTC 今天 + 1 天）。**不做时区换算**——日记是日历日，不是时间点。
- 没有 `mood` 字段：心情即标签。
- 迁移用 `sqlx::migrate!("./migrations")`，编译期嵌入，启动时自动执行。

---

## 6. 认证与安全

### 6.1 注册

- `DIARY_ALLOW_REGISTRATION=true` 时开放（默认开启）。
- 用户名：`^[A-Za-z0-9_.-]{3,32}$`，`username_norm = lower(trim())` 唯一。
- 密码：8 ≤ 长度 ≤ 128，且不在内置弱口令 Top-1000 列表里（不引入 zxcvbn 后端依赖，前端可给强度提示）。
- 哈希：Argon2id，`m=19456 KiB, t=2, p=1`（OWASP 2024 基线）。PHC 字符串入库，登录成功时若参数过旧则透明重哈希。
- 注册成功后**不自动登录**，跳转登录页。
- 限流：每 IP 5 次/小时，全局 50 次/小时。
- 无邮箱 → **没有密码重置**（见 §16）。

### 6.2 登录（两阶段）

```
POST /api/v1/auth/login  { username, password }
├─ 失败 → 401 {"error":{"code":"invalid_credentials"}}   ← 不区分"用户不存在/密码错"
├─ 成功，totp_enabled = 0
│    → 200 { access_token, expires_in } + Set-Cookie(refresh)
└─ 成功，totp_enabled = 1
     → 200 { mfa_required: true, mfa_token, expires_in }   ← 不发 access token！

POST /api/v1/auth/2fa/verify  { mfa_token, code }
→ 200 { access_token, expires_in } + Set-Cookie(refresh)
```

**硬性约束**：第一步成功时绝不签发业务可用的 access token。`mfa_token` 必须是独立类型：

```jsonc
// access
{ "sub":"42","typ":"access","ver":0,"jti":"…","iat":…,"exp":…,"iss":"diary","aud":"diary-web" }
// mfa
{ "sub":"42","typ":"mfa","scope":"mfa","jti":"…","exp":…,"iss":"diary","aud":"diary-mfa" }
```

中间件只接受 `typ=access` 且 `aud=diary-web`；`/2fa/verify` 只接受 `typ=mfa` 且 `aud=diary-mfa`。显式指定算法（禁止 `none`），允许 60s 时钟偏移。

### 6.3 TOTP

- 参数：HMAC-SHA1、`step=30s`、`digits=6`、secret 为 Base32 160-bit。
- **允许误差 ±30s** = `skew=1`，即接受 `t-1, t, t+1` 三个时间片。

```rust
let totp = TOTP::new(Algorithm::SHA1, 6, 1, 30, secret_bytes)?;
if !totp.check_current(&code)? { return Err(invalid_code); }
```

- **防重放（必做）**：成功后写 `totp_last_step = step`，并要求 `step > totp_last_step`，否则 409 `code_already_used`。没有这条，同一个码在 90 秒窗口内可被重复使用。
- 输入归一化：去空格、只保留数字、长度必须 6，否则直接拒绝（不消耗限流额度之外的重试）。
- **secret 静态加密**：`secret_enc = [key_id=0] || XChaCha20-Poly1305(DIARY_MASTER_KEY, nonce, secret)`，nonce 12 字节随机、随密文前缀存储。DB 泄露 ≠ 2FA 被绕过。

**绑定流程**

```
GET  /api/v1/auth/2fa/setup     (需 access)
  → 200 { otpauth_uri, secret_base32 }
     服务端生成 secret，存入 totp_secret_enc 但 totp_enabled 仍为 0
     前端用 qrcode 本地渲染二维码，secret 不出浏览器

POST /api/v1/auth/2fa/enable    (需 access)  { code }
  → 校验通过：totp_enabled = 1, totp_last_step = 当前 step
  → 200 { ok: true }
  → 前端 **必须**明文展示一次 otpauth URI，提示用户自行保存（secret 是唯一备份）
```

### 6.4 关闭 2FA（方案 B：7 天冷静期）

**没有恢复码，也没有邮箱**，所以"密码 + 当前 TOTP 码"才能关 2FA 会导致丢手机 = 永久锁号。采用冷静期方案：

```
POST /api/v1/auth/2fa/disable  (需 access)  { password }
  → 校验密码 → twofa_disable_at = now + 7 天
  → 200 { effective_at: RFC3339 }
  → 期间 2FA 仍然生效（正常登录仍要验证码）

POST /api/v1/auth/2fa/disable/cancel (需 access)
  → twofa_disable_at = NULL（任何正常登录的用户都能取消，攻击者做不到）
```

生效方式：**惰性检查**——任何认证请求 / 登录时若 `now >= twofa_disable_at`，则关闭 2FA（`totp_enabled=0`、清空 secret、`token_version += 1`），并强制该用户重新登录。

安全性质：仅凭密码的攻击者最多能把 2FA 排期，但在 7 天里合法用户只要登录一次（看到横幅）就能取消。这是"无恢复码"约束下的最优折中。

### 6.5 令牌与 Cookie

| 令牌 | 存储 | 有效期 | 说明 |
|---|---|---|---|
| access | 前端**内存**（Svelte store） | 15 min | 不放 localStorage；刷新页面靠 refresh 恢复 |
| refresh | `HttpOnly; Secure; SameSite=Strict; Path=/api/v1/auth` | 30 天 | 数据库存 SHA-256 哈希 |
| mfa | 前端内存 | 5 min | 仅用于 `/2fa/verify` |

**刷新令牌轮转**：

- 每次 `POST /auth/refresh` 签发新令牌，旧记录 `revoked_at = now`、`replaced_by = 新 jti`。
- 若收到一个**已被轮转**的令牌（`replaced_by IS NOT NULL`）→ 判定为令牌窃取：吊销该用户全部刷新令牌 + `token_version += 1`，返回 401。
- access token 校验时比对 `ver` 与 `users.token_version`（不一致即失效）——这让"改密码 / 强制下线"即时生效，**无需**黑名单表。
- **不做**设备列表、不做"登出所有设备"按钮；`token_version` 是唯一的批量下线机制（改密码时自动触发）。

**跨站 Cookie 的兜底**：若无法 same-site 部署，改为 `SameSite=None; Secure` 并对 `/auth/refresh` 做 `Origin` 校验 + 双提交 CSRF Token；若浏览器仍拦截，最终退化为"refresh 存内存、刷新页面重登"。

### 6.6 改密码

```
POST /api/v1/auth/password  (需 access)
{ current_password, new_password, totp_code? }   // totp_enabled=1 时 totp_code 必填
```

成功后：重哈希密码、`token_version += 1`、吊销全部 refresh token、返回 204 并要求重新登录。

---

## 7. 标签

### 规则

1. **先建后用**：`PUT /entries/:date` 只能引用**已存在且属于当前用户**的 `tag_id`。未知 ID → 400 `unknown_tag`。
2. 每用户 ≤ 200 个标签，每篇 ≤ 20 个标签。
3. 标签名 1–32 字符，`name_norm` 判重（`开心` 与 `开心 ` 视为同名）。
4. 颜色 `^#[0-9a-f]{6}$`（小写归一），emoji 限制单个 grapheme（≤ 8 字节）。
5. `sort_order` 升序，相同时按 `name` 排序。
6. **删除 = 软删除**（`deleted_at`）：从选择器隐藏、不能新建引用；已有 `entry_tags` 保留，历史日记与旧 `.diary` 仍能显示标签名。删除按钮文案为"归档标签"。

### 修改

- 改名：更新 `name` / `name_norm`（可能撞唯一键 → 409）。`entry_tags` 不受影响（引用的是 ID）。
- **不做别名兼容**：改名后，旧 `.diary` 里的旧名在导入时会被视为**新标签**（自动创建）。

### 前端交互（重要）

选择器内的「＋新建」**不跳页**：弹小框输名字 → `POST /tags` → 拿到 `id` → 立即附加到当前草稿。也就是一次交互内完成"新建 → 使用"，服务端只要求落库时刻标签已存在。

---

## 8. 日记与 Markdown

### 接口语义

- `PUT /api/v1/entries/:date` 是 **upsert**：不存在则创建，存在则**整体覆盖** `content_md` 与标签集合。
- 可选乐观并发：请求带 `base_updated_at`（RFC3339）。服务端若 `entries.updated_at != base_updated_at` → 409 `stale_write`，返回服务端当前版本，前端提示"另一处已修改"。
- 单篇 `content_md` ≤ 1 MiB（按 UTF-8 字节）。
- `entry_date` 校验：格式 + 范围（`1970-01-01` ~ UTC 今天 + 1 天）。

### Markdown 渲染（前端）

```ts
const md = new MarkdownIt({ html: false, linkify: true, breaks: true });
const html = DOMPurify.sanitize(md.render(src), {
  ALLOWED_TAGS: [/* 白名单，不含 iframe/object/style */],
  ALLOWED_ATTR: ['href', 'title', 'class', 'alt', 'src'],
  FORBID_TAGS: ['script', 'style', 'iframe', 'form', 'input'],
  FORBID_ATTR: ['onerror', 'onload', 'onclick', 'style']
});
```

- `html: false`：Markdown 内的原始 HTML 一律转义（不是"允许后清洗"，而是"根本不允许"）。
- 链接：`target="_blank"` + `rel="noopener noreferrer nofollow"`；`javascript:` / `data:` 协议直接剥除。
- **图片不自动加载**：渲染时把 `<img>` 替换为占位按钮，用户点击后才注入 `src`。避免打开日记就向第三方泄漏 IP 与"你哪天写了什么"的时间相关性。

### 编辑器

- CodeMirror 6（Markdown），左编辑右预览，预览 debounce 200 ms。
- 快捷键 `Ctrl/Cmd+S` 保存；未保存时离开路由弹确认。
- 本地草稿（IndexedDB，按 `userId + date`），保存成功后清除。崩溃/断网不丢内容。

---

## 9. `.diary` 文件格式规范 v1

**设计目标**：自描述、带版本、可向后兼容扩展；未加密时是可直接查看的 JSON；加密时只加密 payload，头部（含 KDF 参数）作为 AEAD 的 AAD 一并保护。

### 9.1 字节布局

```
偏移  长度  字段            说明
0     8     magic           b"DIARY\x1a\x01\x00"
8     2     version         小端 u16，v1 = 1
10    1     flags           bit0 = ENCRYPTED；bit1..7 保留，v1 必须为 0（非 0 即拒绝）
11    1     kdf             0 = none，1 = argon2id；v1 只允许这两个值
12    4     m_cost          KiB，小端 u32；kdf=0 时为 0
16    4     t_cost          迭代次数，小端 u32；kdf=0 时为 0
20    1     p_cost          parallelism，u8；kdf=0 时为 0
21    16    salt            随机；kdf=0 时全 0
37    12    nonce           AES-GCM nonce；未加密时全 0
49    8     payload_len     小端 u64 = payload 字节数
57    N     payload         ENCRYPTED ? 密文||16B tag : 明文 JSON（UTF-8）
```

头部固定 57 字节。**`aad = bytes[0..57]`**，加密与解密都必须原样传入，因此版本号、flags、KDF 参数被一并认证——无法把 `ENCRYPTED` 位改掉来做降级攻击。

### 9.2 加密参数（v1 固定）

| 项 | 值 |
|---|---|
| KDF | Argon2id，`m = 65536` KiB（64 MiB），`t = 3`，`p = 1`，输出 **32 字节 raw key** |
| salt | 16 字节，`OsRng` / `crypto.getRandomValues`，每次导出都新生成 |
| 加密 | AES-256-GCM，nonce 12 字节随机，tag 16 字节 |
| 输出布局 | `ciphertext \|\| tag`（WebCrypto 与 Rust `aes_gcm::Aes256Gcm::encrypt` 均为该布局，可直接互通） |
| 明文 | UTF-8 JSON（§9.3），不做压缩（bit1 保留给将来 zstd） |
| `payload_len` | 加密时 = 密文字节数（含 tag），即 `plaintext_len + 16` |

**nonce 复用安全**：每次导出都生成新的 salt → 派生出的 key 每次不同，因此 (key, nonce) 不可能重复。禁止"复用同一 salt 导出多次"。

**解密前的防护（必做）**：

- 校验 `magic` 与 `version ≤ 1`；
- 拒绝 `flags & !0x01 != 0`；
- 拒绝 `m_cost > 262144`（256 MiB）或 `t_cost > 10`（防恶意文件把浏览器/手机打爆，OOM 或长时间阻塞）；
- 拒绝 `payload_len > DIARY_MAX_IMPORT_BYTES`（64 MiB）；
- 拒绝 `payload_len` 与实际文件长度不符（截断检测）。

**未加密导出**：`kdf = 0`、salt/nonce 全 0、payload 为明文 JSON。UI 必须明确警告"此文件未加密，任何人拿到都能阅读"。

### 9.3 payload JSON（v1）

```jsonc
{
  "format": "diary",
  "version": 1,
  "exported_at": "2025-03-01T12:00:00Z",
  "app": "diary/0.1.0",
  "timezone": "Asia/Shanghai",          // 导出者本地时区，仅供参考，不参与解析
  "tags": [
    { "name": "开心", "color": "#f59e0b", "emoji": "😄" },
    { "name": "加班", "color": "#6b7280" }
  ],
  "entries": [
    {
      "date": "2025-02-14",
      "content_md": "# 今天\n\n写了一下午代码。",
      "tags": ["开心", "加班"],
      "created_at": "2025-02-14T22:10:00Z",
      "updated_at": "2025-02-14T22:30:00Z"
    }
  ]
}
```

**规范（MUST）**

1. `tags` 只列出被 `entries` 引用到的标签（不导出未被使用的标签）。
2. `entries[].tags` 引用 `tags[].name`，**不出现 ID**（跨账号 ID 无意义）。
3. `entries[].date` 全局唯一。解析到重复 → 整包拒绝（不做静默取舍）。
4. 不含 `user_id`、不含密码哈希等任何账号信息。
5. 未知字段必须忽略（向前兼容）；`version > 1` → 拒绝并提示升级。
6. 上限：`entries ≤ 100000`、`tags ≤ 500`、单条 `content_md ≤ 1 MiB`、总 `payload ≤ 64 MiB`。
7. `color` 不合法 → 忽略该字段（不报错）；`emoji` 非单个 grapheme → 忽略。

**为何用 JSON**：未加密时可用任何编辑器检查、可人工 diff、`jq` 可处理。体积代价在个人日记量级可忽略。

### 9.4 测试向量（必须存在）

`docs/test-vectors.json` 至少包含：

- 固定 password / salt / nonce / 明文的**加密**用例 → 期望密文 hex（含 tag）；
- 同一用例的**解密**往返；
- 截断密文、翻转一个比特的密文 → 必须失败；
- 篡改头部（version / flags / m_cost）→ 必须失败（AAD 生效）；
- 明文（未加密）用例；
- 中文 + emoji + 换行的正文往返。

**Rust 与 TypeScript 两侧测试都必须读这个文件**，保证跨实现字节级一致。Rust 侧实现不参与生产链路（前端加解密），但必须存在并纳入 CI，作为格式的"第三方验证"。

---

## 10. 导出

```
POST /api/v1/export/prepare       (需 access)
{ "scope": "all" }
{ "scope": "dates", "dates": ["2025-01-01", "2025-01-15"] }
```

**服务端行为**：

1. 校验 `dates` 格式、去重、数量 ≤ 上限；空数组 → 400。
2. 查询条目（`scope=all` 即该用户全部），流式构造 payload JSON。
3. **返回明文 JSON**，绝不落盘、不写临时文件、不记日志、不缓存：

```
200 OK
Content-Type: application/json
Cache-Control: no-store
X-Content-Type-Options: nosniff
```

4. 超过 `DIARY_MAX_EXPORT_BYTES` → 413 `export_too_large`，提示缩小日期范围。

**前端行为**：

1. 收齐 JSON 字节 → 用户输入密码（≥ 8 字符，含"确认密码"，带强度提示）。
2. `hash-wasm` 派生 key（64 MiB / t=3 / p=1）→ WebCrypto `AES-GCM` 加密（AAD = 57 字节头）。
3. 组装 `.diary` → `Blob` → `URL.createObjectURL` → `<a download="diary-YYYY-MM-DD.diary">`。
4. **立即 `zeroize`**：清空明文 `Uint8Array`、密码变量、派生出的 key（`key.fill(0)`）；丢弃 JSON 引用。
5. 内存上限：前端也要检查 `payload ≤ 64 MiB`，超出提示分批导出。

**关于信任边界**：导出密码永不出浏览器、服务端也不知道它是多少。服务端只看到"要哪些日期"和它本来就有权限读的明文内容。前端加解密的固有代价是 XSS/供应链污染可窃取密码——用严格 CSP 与锁死依赖缓解。

---

## 11. 查看 `.diary`

**完全离线，不上传，不登录**（独立路由 `/viewer`，登录态无关）。

```
选择/拖入文件
  → 读 57 字节头
  → 校验 magic / version / flags / KDF 参数上限 / 长度一致
  ├─ flags.ENCRYPTED = 0 → 直接解析 JSON → 渲染
  └─ flags.ENCRYPTED = 1 → 弹出密码框
        → Argon2id(salt, m, t, p) → AES-GCM 解密（AAD = 头）
        → 成功：解析 JSON
        → 失败：统一提示「密码错误，或文件已损坏」
```

- 密码错误与文件损坏**不区分提示**（GCM tag 校验失败无法区分，且避免泄露信息）。
- 成功后展示：日期时间线 + 日历热力（按标签颜色）+ 单日详情（与正常日记同样的 Markdown 清洗流程）。
- 提供「导入到我的日记」按钮 → 转入 §12 流程（此时才需要登录）。
- 大文件：解析后按日期分页/虚拟列表渲染，避免一次挂载上千篇 DOM。

---

## 12. 导入

**解密在前端完成**，服务端只接收结构化明文条目。这样导出密码永不出浏览器，后端接口也更简单。

### 12.1 两阶段：预检 → 提交

```
POST /api/v1/import/preview   (需 access)
{ "entries":[...], "tags":[...], "scope": "all" | "dates", "dates":[...] }
→ 200 {
    "import_id": "uuid",
    "summary": { "create": 30, "update": 12, "tags_create": 2 },
    "tags_to_create": [ {"name":"开心","color":"#f59e0b","emoji":"😄"} ],
    "dates": { "create": [...], "update": [...] }
  }
  （服务端把校验后的 payload 暂存内存：TTL 10 min、单用户仅 1 份、≤64 MiB）

POST /api/v1/import/commit    (需 access)
{ "import_id": "uuid", "scope": "all" | "dates", "dates":[...] }
→ 200 { "created": 30, "updated": 12, "tags_created": 2 }
```

**为什么用 `import_id` 而不是让前端上传两次**：文件可达 64 MiB，重复上传既慢又浪费内存。预检时暂存、提交时按 ID 取用。若进程重启导致暂存丢失 → 404 `import_expired`，前端重新预检。

### 12.2 覆盖语义（已定）

**以文件中出现的日期集合为准，逐条覆盖；文件中没有的日期一律不动，也不删除任何东西。**

- 无"按更新时间跳过"策略（用户明确选择"覆盖"）。
- 一条记录**整体替换**：`content_md` + 标签集合一起换掉。文件中某天标签为空数组 → 清空该天标签。
- `created_at`：本地已有则保留，否则用文件值；`updated_at` = **导入时刻**；`imported_at` = 导入时刻。
- **幂等**：同一文件连导两次，第二次 `create=0`、`update=N`，数据完全等价。

### 12.3 标签解析（已定：自动创建）

1. 用文件 `tags[]` 构建 `name_norm → {name, color, emoji}` 映射。
2. 对每个被引用的标签名：
   - 本地已有（`name_norm` 命中且未软删除）→ 复用其 `id`（**不覆盖**本地颜色/emoji）；
   - 不存在 → 自动创建（颜色/emoji 沿用文件值），计入 `tags_create`。
3. 文件里引用了 `tags[]` 中未声明的名字 → 仍自动创建，`name` 用原文，`color/emoji` 为空，并在响应里以 `warnings` 提示。
4. 并发同名创建：`INSERT ... ON CONFLICT(user_id, name_norm) DO NOTHING` 后重查一次。
5. 撞上已软删除的同名标签 → 恢复（`deleted_at = NULL`）而不是新建，避免唯一键冲突。
6. **不做别名/改名兼容**：改名后的旧名即新标签。

### 12.4 事务

```sql
BEGIN IMMEDIATE;

-- 1) 解析/创建标签，得到 name_norm -> tag_id
-- 2) 逐日期 upsert
INSERT INTO entries (user_id, entry_date, content_md, created_at, updated_at, imported_at)
VALUES (?, ?, ?, ?, ?, ?)
ON CONFLICT (user_id, entry_date) DO UPDATE SET
    content_md  = excluded.content_md,
    updated_at  = excluded.updated_at,
    imported_at = excluded.imported_at;
-- 3) entry_tags：先删该 entry 全部，再插入本次集合（整体替换）
-- 4) 不触碰文件中未出现的日期

COMMIT;
```

- 整批要么全成、要么全不成（任何一条校验失败 → 回滚 + 400，指出第几条哪个字段）。
- 导入前校验：日期格式/范围、`content_md` 字节数、标签数量、条目总数、`tags` 声明一致性。
- 导入的条目**一律归属当前登录用户**（payload 里的任何 user 字段都被忽略）。

### 12.5 不导入的东西

- 不导入 `id`、`updated_at` 的历史值、`exported_at`、`app`、`timezone`。
- 不导入未被任何条目引用的标签。
- 不删除、不重命名本地已有标签。

---

## 13. API 参考

统一前缀 `/api/v1`。`Authorization: Bearer <access_token>`（除注册/登录/刷新）。

### 认证

| 方法 | 路径 | 认证 | 说明 |
|---|---|---|---|
| POST | `/auth/register` | — | `{username, password}` → 201 |
| POST | `/auth/login` | — | → `{access_token, expires_in}` 或 `{mfa_required, mfa_token, expires_in}` |
| POST | `/auth/2fa/verify` | mfa token | `{code}` → `{access_token, expires_in}` |
| POST | `/auth/refresh` | Cookie | → `{access_token, expires_in}` + 新 Cookie |
| POST | `/auth/logout` | access | 吊销当前 refresh token → 204 |
| POST | `/auth/password` | access | `{current_password, new_password, totp_code?}` → 204 |
| GET | `/auth/2fa/setup` | access | → `{otpauth_uri, secret_base32}` |
| POST | `/auth/2fa/enable` | access | `{code}` → 200 |
| POST | `/auth/2fa/disable` | access | `{password}` → `{effective_at}` |
| POST | `/auth/2fa/disable/cancel` | access | → 204 |
| GET | `/auth/me` | access | → `{id, username, totp_enabled, twofa_disable_at}` |

### 标签

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/tags?include_archived=false` | 列表（按 `sort_order, name`） |
| POST | `/tags` | `{name, color?, emoji?, sort_order?}` → 201 / 409 `tag_exists` |
| PATCH | `/tags/:id` | `{name?, color?, emoji?, sort_order?}` |
| DELETE | `/tags/:id` | 软删除（归档） → 204 |

### 日记

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/entries?from=&to=` | 列表：`{date, content_md, tags:[{id,name,color,emoji}], created_at, updated_at}` |
| GET | `/entries/:date` | 单篇 |
| PUT | `/entries/:date` | `{content_md, tag_ids:[], base_updated_at?}` → upsert |
| DELETE | `/entries/:date` | → 204 |

### 导入 / 导出

| 方法 | 路径 | 说明 |
|---|---|---|
| POST | `/export/prepare` | `{scope, dates?}` → 明文 JSON（`no-store`） |
| POST | `/import/preview` | `{entries, tags, scope, dates?}` → `{import_id, summary, ...}` |
| POST | `/import/commit` | `{import_id, scope, dates?}` → 结果统计 |

### 健康检查

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/healthz` | 进程存活 |
| GET | `/readyz` | DB 可查询（执行 `SELECT 1`） |

---

## 14. 错误与限流

统一错误体：

```json
{ "error": { "code": "invalid_credentials", "message": "用户名或密码错误", "details": null } }
```

| code | HTTP | 场景 |
|---|---|---|
| `invalid_credentials` | 401 | 登录失败（不区分用户是否存在） |
| `mfa_required` | 401 | 访问业务接口但令牌是 mfa 类型 |
| `invalid_totp` | 401 | 验证码错误 |
| `code_already_used` | 409 | TOTP 重放 |
| `token_reused` | 401 | 刷新令牌复用 → 已吊销全部 |
| `tag_exists` | 409 | 标签重名 |
| `unknown_tag` | 400 | `tag_ids` 含不存在/他人的标签 |
| `stale_write` | 409 | 乐观并发冲突 |
| `entry_too_large` | 413 | 正文超限 |
| `export_too_large` / `import_too_large` | 413 | 超过 64 MiB |
| `import_expired` | 404 | `import_id` 失效 |
| `bad_file_format` | 400 | 导入 payload 结构/日期非法 |
| `rate_limited` | 429 | 触发限流（带 `Retry-After`） |

**限流（按 IP + 按用户双维度）**

| 端点 | 限制 |
|---|---|
| `POST /auth/register` | 5 / 小时 / IP |
| `POST /auth/login` | 10 / 5 分钟 / IP；失败 20 次 / 小时 / 账号 → 临时锁定 15 分钟 |
| `POST /auth/2fa/verify` | 10 / 5 分钟 / IP；失败 10 次 / 小时 / 账号 |
| `POST /auth/2fa/disable` | 3 / 天 / 用户 |
| 其他写入 | 300 / 分钟 / 用户 |
| 全局请求体 | ≤ 2 MiB（导入/导出单独放宽至 64 MiB） |

TOTP 不强制开启，因此**登录与 2FA 端点限流是主要防线**，6 位码在没有限流时可在数小时内被爆破。

---

## 15. 安全清单

- [x] Argon2id（登录密码）参数写入 PHC 字符串，登录成功时透明升级旧参数。
- [x] TOTP secret 静态加密（XChaCha20-Poly1305 + 服务器主密钥）。
- [x] TOTP 防重放（`totp_last_step`）；±30s 由 `skew=1` 实现。
- [x] `mfa_token` 与 `access_token` 类型/受众严格隔离。
- [x] refresh token 哈希入库 + 轮转 + 复用检测；`token_version` 支持即时下线。
- [x] 登录/2FA/注册限流；统一错误文案防用户枚举。
- [x] Markdown：`html:false` + DOMPurify 白名单 + 协议过滤；图片默认不加载。
- [x] `.diary`：AAD 绑定头部；KDF 参数上限；长度一致性；解密失败统一提示。
- [x] 前端加解密后显式清零明文、密码、派生 key。
- [x] CORS 精确白名单 + `Origin` 校验；Cookie `HttpOnly/Secure/SameSite=Strict`。
- [x] 严格 CSP（`script-src 'self'`，无内联、无 CDN）。
- [x] 请求体大小上限、超时（`tower-http::timeout`）、`nosniff`。
- [x] 密码与密钥使用 `secrecy` / `zeroize`；日志脱敏（禁止打印密码、令牌、secret）。
- [x] DB 文件权限 0600；WAL 模式；`busy_timeout`。

---

## 16. 已知限制

1. **没有密码重置**。无邮箱通道 → 忘记密码 = 永久失去账号。**数据库冷备份是唯一兜底**，请务必配置定时备份（`VACUUM INTO`，或连同 `-wal`/`-shm` 一起复制）。
2. **没有恢复码**。丢手机后只能走"7 天冷静期关闭 2FA"（需要密码）。绑定 2FA 时务必保存 `otpauth` URI。
3. **导出密码与登录密码是两套凭据**，互不影响；`.diary` 密码忘了 = 该文件不可恢复（设计如此，没有后门）。
4. **前端加密的信任边界**：XSS 或依赖被投毒时，密码与明文可能被窃取。缓解：CSP + 依赖锁定 + 无外部 CDN。
5. **不做搜索**：只能按日期浏览。中文 FTS 的分词成本在 v1 不划算。
6. **无附件**：图片只能用外链，且默认不加载。
7. **并发编辑**：同一篇多标签页同时编辑时靠 `base_updated_at` 检测冲突，不做实时协同。
8. **服务端仍可见明文**：它是数据源。本方案保护的是"导出文件的密码"和"归档文件的离线分发"，不是"服务端零知识"。
9. **必须 HTTPS**：否则浏览器加密功能不可用。

---

## 17. 测试计划

### 后端（`cargo test` + `sqlx::test`）

- 注册/登录/2FA 状态机：未启用 2FA、已启用、`mfa_token` 不能访问业务接口。
- TOTP 边界：`t-31s` 拒 / `t-30s` 收 / `t` 收 / `t+30s` 收 / `t+31s` 拒；同一码第二次 → 409。
- 2FA 冷静期：排期后 2FA 仍生效；到期后自动关闭且 `token_version` 自增；可取消。
- 刷新令牌轮转与复用检测；改密码后旧令牌全失效。
- 标签：重名 409、跨用户引用 400、每篇上限、软删除后不可引用、归档标签恢复。
- 导入：新增/覆盖/幂等/重复日期拒绝/未知标签自动创建/标签整体替换/未出现日期不动/整批回滚。
- 导出：`scope=dates` 只含指定日期；超限 413；响应头 `no-store`。
- 限流与请求体上限。

### 前端（Vitest）

- `.diary` 测试向量：加密产物与 Rust 侧逐字节相同；解密往返；篡改任一字节必须失败。
- 未加密文件解析；版本过高拒绝；KDF 参数超限拒绝。
- Markdown 清洗用例集：`<script>`、`onerror`、`javascript:`、`<iframe>`、`<style>`、`data:` 图片。
- 导入预检统计与提交幂等（mock API）。

### E2E（Playwright）

注册 → 开 2FA → 登录 → 写日记 + 标签 → 导出（加密）→ 退出登录 → 查看器离线打开 → 导入 → 校验覆盖结果。

### CI 断言

- `cargo tree -i libsqlite3-sys` 含 `bundled`。
- 发布产物能在 `scratch` 容器里启动并通过 `/readyz`。

---

## 18. 里程碑

| 阶段 | 内容 | 完成标准 |
|---|---|---|
| M1 | 骨架：axum + sqlx(bundled) + 迁移 + 配置 + `/healthz` | 空库启动、迁移成功 |
| M2 | 注册 / 登录 / JWT access+refresh / `token_version` | 认证测试全绿 |
| M3 | 日记 CRUD + 标签 CRUD + 日历与编辑器 UI | 能写、能改、能打标签 |
| M4 | TOTP：绑定 / 验证 / 防重放 / 冷静期关闭 / 限流 | 2FA 测试全绿 |
| M5 | `.diary` 导出（前端加密）+ 查看器 + 测试向量 | 产物与 Rust 侧字节一致 |
| M6 | 导入：预检 / 提交 / 自动建标签 / 事务 / 幂等 | E2E 通过 |
| M7 | 加固与交付：CSP、CORS、限流、备份脚本、日志脱敏、静态构建 | 安全清单全勾 |

---

## 19. 决策记录

| # | 决策 | 结论 | 理由 |
|---|---|---|---|
| 1 | 导出/导入加解密位置 | **前端** | 导出密码永不出浏览器；后端只做结构化 CRUD |
| 2 | 导入语义 | **按文件中出现的日期整体覆盖，未出现的不动** | 符合"选择性恢复"直觉；不删除任何数据 |
| 3 | 导入时缺失标签 | **自动创建**（1a） | 一次导入不被卡住；导入报告列出新建项 |
| 4 | 标签改名兼容 | **不做**（2） | 个人项目；旧名视为新标签 |
| 5 | 关闭 2FA 条件 | **仅密码 + 7 天冷静期**（3B） | 无恢复码也无邮箱时的唯一可用恢复路径 |
| 6 | 注册 | **开放**（`DIARY_ALLOW_REGISTRATION=true`，默认开启） | 可随时用配置关闭 |
| 7 | 改密码 | **需要当前密码 + TOTP 码**（5） | 防会话被盗后改密锁号；同时 `token_version += 1` |
| 8 | Markdown 图片 | **仅外链，且默认不自动加载**（6a） | 无附件存储；防追踪像素 |
| 9 | 全文搜索 | **不做**（7） | 中文分词成本 > 收益 |
| 10 | 体积上限 | 单篇 **1 MiB**；单次导入/导出明文 **64 MiB** | 单条 AES-GCM 需要整体在内存；64 MiB 覆盖十年个人日记 |
| 11 | 数据库 | SQLite **bundled**（静态编译） | 运行时不依赖系统 libsqlite3 |
| 12 | 文件格式 | 二进制头（57 B，作 AAD）+ UTF-8 JSON payload | 未加密时可读可 diff；头部被认证 |
| 13 | 会话管理 | 不做设备列表/全端登出 UI；用 `token_version` 做逻辑下线 | 保持界面简单，同时保留"改密即下线"能力 |
| 14 | 部署拓扑 | 前后端分离但**必须 same-site** | 否则刷新 Cookie 在 Safari/Firefox 会被拦截 |
| 15 | 导入预检 | 保留（`import_id` 暂存，避免重复上传） | 覆盖前让用户看清影响范围 |

### 待定（不阻塞 M1–M3）

- `DIARY_MASTER_KEY` 轮换脚本（`diary rekey-master`）的实现形态。
- 是否需要在 M7 增加"登录审计日志"页面（记录 IP / UA / 失败次数）——目前只写 `tracing`，不做 UI。
