import assert from 'node:assert/strict';
import { mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { spawn } from 'node:child_process';
import { setTimeout as delay } from 'node:timers/promises';
import { chromium } from 'playwright';
import AxeBuilder from '@axe-core/playwright';

const root = resolve(import.meta.dirname, '../..');
const evidence = resolve(root, 'docs/evidence/T16');
const base = process.env.T16_BASE_URL ?? 'http://127.0.0.1:5190';
const origin = new URL(base);
assert.ok(['127.0.0.1', 'localhost'].includes(origin.hostname), 'T16 only runs on a local development instance');
const report = { started: new Date().toISOString(), browser: '', layouts: [], checks: [], accessibility: [] };
let developmentServer;
if (!process.env.T16_BASE_URL) {
  developmentServer = spawn(process.execPath, [resolve(root, 'node_modules/vite/bin/vite.js'), '--port', '5190'], { cwd: resolve(root, 'apps/identity-web'), shell: false, stdio: 'ignore' });
  for (let attempt = 0; attempt < 60; attempt += 1) {
    try {
      const response = await fetch(`${base}/dev/components`, { signal: AbortSignal.timeout(1000) });
      if (response.ok) break;
    } catch { /* Actual local startup polling, never authentication expiry. */ }
    await delay(100);
    assert.notEqual(attempt, 59, 'Local T16 Vite server failed to start');
  }
}
const browser = await chromium.launch();
report.browser = browser.version();
await mkdir(evidence, { recursive: true });

try {
  for (const width of [360, 390, 768, 1440]) {
    const context = await browser.newContext({ viewport: { width, height: 1000 }, reducedMotion: 'reduce' });
    const page = await context.newPage();
    const errors = []; page.on('pageerror', (error) => errors.push(error.message));
    await page.goto(`${base}/dev/components`);
    await page.getByRole('heading', { name: '组件与反馈状态' }).waitFor();
    const overflow = await page.evaluate(() => globalThis.document.documentElement.scrollWidth > globalThis.innerWidth);
    assert.equal(overflow, false, `Horizontal overflow at ${width}`);
    const targets = await page.locator('button,input').evaluateAll((nodes) => nodes.map((node) => ({ text: node.textContent, width: node.getBoundingClientRect().width, height: node.getBoundingClientRect().height })));
    assert.ok(targets.every((target) => target.width >= 44 && target.height >= 44), `Click target below44 at ${width}`);
    const axe = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa']).analyze();
    assert.deepEqual(axe.violations.map((violation) => ({ id: violation.id, impact: violation.impact })), [], `axe violations at ${width}`);
    const file = `components-${width}.png`;
    await page.screenshot({ path: resolve(evidence, file), fullPage: true });
    report.layouts.push({ width, horizontalOverflow: overflow, minimumTarget: 44, screenshot: file, result: 'pass' });
    report.accessibility.push({ width, passes: axe.passes.length, violations: axe.violations, incompleteRules: axe.incomplete.map((rule) => rule.id) });
    assert.deepEqual(errors, [], 'Unexpected browser exception');
    await context.close();
  }
  const context = await browser.newContext({ viewport: { width: 390, height: 844 }, reducedMotion: 'reduce' });
  const page = await context.newPage();
  await page.goto(`${base}/dev/components`);
  const heading = page.getByRole('heading', { name: '组件与反馈状态' }); await heading.waitFor();
  await page.getByRole('button', { name: '检查输入', exact: true }).click();
  await page.getByRole('alert').filter({ hasText: '请输入有效邮箱地址' }).waitFor();
  const email = page.getByLabel('邮箱地址', { exact: true });
  assert.equal(await email.getAttribute('aria-invalid'), 'true');
  await email.fill('person@example.test');
  const password = page.getByLabel('密码', { exact: true });
  await password.fill('example long password for controls');
  await page.getByRole('button', { name: '显示密码', exact: true }).click();
  assert.equal(await password.getAttribute('type'), 'text');
  await page.getByLabel('验证码', { exact: true }).fill('123456');
  await page.getByRole('button', { name: '检查输入', exact: true }).click();
  await page.getByRole('status').filter({ hasText: '输入格式符合示例规则' }).waitFor();
  assert.equal(await password.inputValue(), '');
  assert.equal(await page.getByLabel('验证码', { exact: true }).inputValue(), '');
  assert.equal(await email.inputValue(), 'person@example.test');
  report.checks.push('labels/errors/password visibility and clearing secrets while preserving email: pass');
  const trigger = page.getByRole('button', { name: '打开确认对话框', exact: true });
  await trigger.click();
  await page.getByRole('dialog').waitFor();
  const cancel = page.getByRole('button', { name: '取消', exact: true });
  assert.equal(await cancel.evaluate((node) => globalThis.document.activeElement === node), true);
  await page.keyboard.press('Shift+Tab');
  assert.equal(await page.getByRole('button', { name: '完成示例', exact: true }).evaluate((node) => globalThis.document.activeElement === node), true);
  const dialogAxe = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa']).analyze();
  assert.deepEqual(dialogAxe.violations.map((violation) => ({ id: violation.id, impact: violation.impact })), []);
  await page.keyboard.press('Escape');
  await page.getByRole('dialog').waitFor({ state: 'hidden' });
  assert.equal(await trigger.evaluate((node) => globalThis.document.activeElement === node), true);
  report.checks.push('dialog initial cancel/trap/Escape/return focus and axe: pass');
  const previous = page.getByRole('button', { name: '上一页', exact: true });
  const next = page.getByRole('button', { name: '下一页', exact: true });
  assert.equal(await previous.isDisabled(), true); await next.click();
  await page.getByRole('cell', { name: '示例记录 B', exact: true }).waitFor();
  assert.equal(await next.isDisabled(), true); assert.equal(await previous.isDisabled(), false);
  report.checks.push('cursor pagination boundaries: pass');
  await page.evaluate(() => { globalThis.document.documentElement.style.fontSize = '32px'; });
  assert.equal(await page.evaluate(() => globalThis.document.documentElement.scrollWidth > globalThis.innerWidth), false, '200% text zoom overflow');
  await page.screenshot({ path: resolve(evidence, 'components-text-200-percent.png'), fullPage: true });
  report.checks.push('200% text scaling (32px base) and reduced motion: pass; full browser zoom/screen reader remains manual release work');
  await context.close();
  report.completed = new Date().toISOString(); report.exitCode = 0;
  console.log('T16 local browser: four layouts, axe, labels, password, dialog, pagination and 200% text scaling passed.');
} catch (error) {
  report.exitCode = 1; report.failure = error.message;
  console.error(error.message); process.exitCode = 1;
} finally {
  await writeFile(resolve(evidence, report.exitCode === 0 ? 'browser-check.json' : 'browser-failure.json'), `${JSON.stringify(report, null, 2)}\n`);
  await browser.close();
  developmentServer?.kill();
}
