import { expect, test, type Download, type Page } from '@playwright/test';
import crypto from 'node:crypto';
import fs from 'node:fs/promises';

const PASSWORD = 'correct horse battery staple';
const EXPORT_PASSWORD = 'another correct horse battery';
const ENTRY_DATE = '2025-01-15';
const ENTRY_TEXT = '今天写了 E2E';

function base32Bytes(input: string): Buffer {
  const alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567';
  const clean = input.replace(/=+$/, '').toUpperCase();
  let bits = 0;
  let value = 0;
  const out: number[] = [];
  for (const char of clean) {
    const index = alphabet.indexOf(char);
    if (index < 0) throw new Error(`bad base32: ${char}`);
    value = (value << 5) | index;
    bits += 5;
    if (bits >= 8) {
      bits -= 8;
      out.push((value >>> bits) & 0xff);
    }
  }
  return Buffer.from(out);
}

function totp(secret: string, step: number): string {
  const key = base32Bytes(secret);
  const counter = Buffer.alloc(8);
  counter.writeBigUInt64BE(BigInt(step));
  const digest = crypto.createHmac('sha1', key).update(counter).digest();
  const offset = digest[digest.length - 1] & 0x0f;
  const code =
    ((digest[offset] & 0x7f) << 24) |
    ((digest[offset + 1] & 0xff) << 16) |
    ((digest[offset + 2] & 0xff) << 8) |
    (digest[offset + 3] & 0xff);
  return String(code % 1_000_000).padStart(6, '0');
}

function currentStep(): number {
  return Math.floor(Date.now() / 30_000);
}

async function registerAndLogin(page: Page, username: string) {
  await page.goto('/register');
  await page.getByLabel('用户名').fill(username);
  await page.getByLabel('密码', { exact: false }).first().fill(PASSWORD);
  await page.getByLabel('确认密码').fill(PASSWORD);
  await page.getByRole('button', { name: '创建账号' }).click();
  await expect(page).toHaveURL(/\/login/);

  await page.getByLabel('用户名').fill(username);
  await page.getByLabel('密码').fill(PASSWORD);
  await page.getByRole('button', { name: '登录' }).click();
  await expect(page.getByRole('heading', { name: '日历' })).toBeVisible();
}

async function downloadExport(page: Page): Promise<string> {
  await page.goto('/data');
  await page.getByLabel('全部日记').check();
  const exportSection = page.locator('section', {
    has: page.getByRole('heading', { name: '导出' })
  });
  await exportSection.locator('input[type="password"]').nth(0).fill(EXPORT_PASSWORD);
  await exportSection.locator('input[type="password"]').nth(1).fill(EXPORT_PASSWORD);
  const [download] = await Promise.all([
    page.waitForEvent('download'),
    exportSection.getByRole('button', { name: '导出 .diary' }).click()
  ]);
  const path = await download.path();
  if (!path) throw new Error('download path missing');
  return path;
}

async function unlockViewer(page: Page, filePath: string) {
  await page.goto('/viewer');
  await page.locator('input[type="file"]').first().setInputFiles(filePath);
  const password = page.locator('input[type="password"]').first();
  await expect(password).toBeVisible();
  await password.fill(EXPORT_PASSWORD);
  await page.getByRole('button', { name: '解密' }).click();
  await expect(page.getByText('时间线')).toBeVisible();
  await expect(page.getByText(ENTRY_TEXT).first()).toBeVisible();
}

test('register, write, export, offline view, import, 2FA login', async ({ page }, testInfo) => {
  const username = `e2e${Date.now().toString(36)}`;
  await registerAndLogin(page, username);

  await page.goto('/tags');
  await page.getByPlaceholder('例如：开心').fill('开心');
  await page.getByRole('button', { name: '新建标签' }).click();
  await expect(page.getByText('标签已创建')).toBeVisible();

  await page.goto(`/day/${ENTRY_DATE}`);
  const editor = page.locator('.cm-content');
  await editor.click();
  await page.keyboard.type(ENTRY_TEXT);
  await page.getByRole('button', { name: /开心/ }).click();
  await page.getByRole('button', { name: '保存' }).click();
  await expect(page.getByText('已保存', { exact: false })).toBeVisible();

  const downloadPath = await downloadExport(page);
  const saved = testInfo.outputPath('diary-export.diary');
  await fs.copyFile(downloadPath, saved);

  // Change the entry after export, then verify import restores the exported version.
  await page.goto(`/day/${ENTRY_DATE}`);
  await page.locator('.cm-content').click();
  await page.keyboard.press('Control+A');
  await page.keyboard.type('changed after export');
  await page.getByRole('button', { name: '保存' }).click();
  await expect(page.getByText('已保存', { exact: false })).toBeVisible();

  await page.getByRole('button', { name: '退出' }).click();
  await expect(page).toHaveURL(/\/login/);

  await unlockViewer(page, saved);

  // The viewer keeps the selected file in memory across the login redirect,
  // then can import it directly (after a confirmation dialog).
  await page.getByRole('button', { name: '登录后导入' }).click();
  await page.getByLabel('用户名').fill(username);
  await page.getByLabel('密码').fill(PASSWORD);
  await page.getByRole('button', { name: '登录' }).click();
  await expect(page.getByRole('heading', { name: '离线查看 .diary' })).toBeVisible();
  const restoredPassword = page.locator('input[type="password"]').first();
  await expect(restoredPassword).toBeVisible();
  await restoredPassword.fill(EXPORT_PASSWORD);
  await page.getByRole('button', { name: '解密' }).click();
  await expect(page.getByText('时间线')).toBeVisible();

  page.once('dialog', (dialog) => dialog.accept());
  await page.getByRole('button', { name: '导入到我的日记' }).click();
  await expect(page.getByText('预检结果')).toBeVisible();
  await page.getByRole('button', { name: '确认导入' }).click();
  await expect(page.getByText(/导入完成/)).toBeVisible();

  await page.goto(`/day/${ENTRY_DATE}`);
  await expect(page.locator('.cm-content')).toContainText(ENTRY_TEXT);

  // Enable TOTP, log out, then log back in with a verification code.
  await page.goto('/settings');
  await page.getByRole('button', { name: '开启两步验证' }).click();
  const secret = (await page.locator('code').first().textContent())?.trim() ?? '';
  expect(secret).toMatch(/^[A-Z2-7]+=*$/);
  const enableStep = currentStep();
  await page.getByLabel('6 位验证码').fill(totp(secret, enableStep));
  await page.getByRole('button', { name: '验证并开启' }).click();
  await expect(page.getByText('已开启', { exact: true })).toBeVisible();

  await page.getByRole('button', { name: '退出' }).click();
  await expect(page).toHaveURL(/\/login/);
  await page.getByLabel('用户名').fill(username);
  await page.getByLabel('密码').fill(PASSWORD);
  await page.getByRole('button', { name: '登录' }).click();
  await expect(page.getByText('两步验证')).toBeVisible();
  await page.getByLabel('验证码').fill(totp(secret, enableStep + 1));
  await page.getByRole('button', { name: '验证并登录' }).click();
  await expect(page.getByRole('heading', { name: '日历' })).toBeVisible();
});
