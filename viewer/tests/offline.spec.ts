import { test, expect } from '@playwright/test';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { gzipSync } from 'node:zlib';
import { pathToFileURL } from 'node:url';

const root = resolve(import.meta.dirname, '../..');
const fixture = resolve(root, 'viewer/fixtures/example.json');
const cli = resolve(root, 'target/debug/cargo-spanscope');
function report(profile: string, ...args: string[]) {
  const dir = mkdtempSync(join(tmpdir(), 'spanscope-e2e-'));
  execFileSync(cli, [profile, '--output', dir, '--no-open', ...args], { cwd: root });
  return pathToFileURL(join(dir, 'index.html')).href;
}

test('inline HTML opens offline and links filters, table, flame, raw and palette', async ({ page }) => {
  const requests: string[] = [];
  await page.route(/^https?:\/\//, route => { requests.push(route.request().url()); return route.abort(); });
  const errors: string[] = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.goto(report(fixture));
  await expect(page.getByRole('heading', { name: 'Overview' })).toBeVisible();
  await expect(page.getByText('Recent roots')).toBeVisible();
  await page.getByRole('button', { name: 'Call chains' }).click();
  await expect(page.getByText('2 matching chains')).toBeVisible();
  await page.getByRole('textbox', { name: 'Search spans' }).fill('transform');
  await expect(page.getByText('1 matching chains')).toBeVisible();
  await expect(page).toHaveURL(/q=transform/);
  await page.reload();
  await expect(page.getByRole('textbox', { name: 'Search spans' })).toHaveValue('transform');
  await expect(page.getByText('1 matching chains')).toBeVisible();
  await page.getByRole('button', { name: 'Flame graph' }).click();
  await expect(page.getByRole('img', { name: 'Interactive structural flame graph' })).toBeVisible();
  await page.keyboard.press('Control+k');
  await expect(page.getByRole('dialog')).toBeVisible();
  await page.getByRole('button', { name: 'Go to Raw explorer' }).click();
  await expect(page.getByRole('heading', { name: 'Raw explorer', level: 1 })).toBeVisible();
  expect(errors).toEqual([]);
  expect(requests).toEqual([]);
});

test('picker accepts gzip over file:// without network', async ({ page }) => {
  const dir = mkdtempSync(join(tmpdir(), 'spanscope-gzip-'));
  const gzip = join(dir, 'profile.json.gz');
  writeFileSync(gzip, gzipSync(readFileSync(fixture)));
  const requests: string[] = [];
  await page.route(/^https?:\/\//, route => { requests.push(route.request().url()); return route.abort(); });
  await page.goto(report(gzip, '--inline-limit-mib', '0'));
  await expect(page.getByText('Large profile ready nearby:')).toBeVisible();
  await page.locator('input[type=file]').setInputFiles(gzip);
  await expect(page.getByRole('heading', { name: 'Overview' })).toBeVisible();
  await expect(page.getByText('Recent roots')).toBeVisible();
  expect(requests).toEqual([]);
});

test('routine cost, per-run counts, metrics and call dependencies are linked', async ({ page }) => {
  await page.route(/^https?:\/\//, route => route.abort());
  await page.goto(report(fixture));
  await page.getByRole('button', { name: 'Routines' }).click();
  await expect(page.getByRole('heading', { name: 'Every observed routine' })).toBeVisible();
  await page.locator('.routine-row').filter({ hasText: 'demo::transform' }).click();
  await expect(page.locator('.detail-stats')).toContainText('880');
  await expect(page.getByRole('heading', { name: 'Called by' })).toBeVisible();
  await expect(page.locator('.relation-panel').first()).toContainText('demo::workload');
  await expect(page.getByRole('heading', { name: 'Calls in each retained run' })).toBeVisible();
  await page.getByRole('button', { name: 'Runs & metrics' }).click();
  await expect(page.getByRole('heading', { name: 'Routine composition' })).toBeVisible();
  await expect(page.getByRole('img', { name: 'input_size across retained runs' })).toBeVisible();
  await page.locator('.run-picker button').first().click();
  await expect(page).toHaveURL(/root=/);
  await expect(page.locator('.composition-list')).toContainText('200 calls');
  await page.getByRole('button', { name: 'Dependencies' }).click();
  await expect(page.getByRole('img', { name: 'Interactive call relationship graph' })).toBeVisible();
  await expect(page.locator('.edge-list')).toContainText('200 calls');
  await expect(page.getByRole('heading', { name: 'Segment dependencies' })).toBeVisible();
  await expect(page.getByText('This run has no explicit segment dependencies.')).toBeVisible();
});

test('dragged JSON loads through the offline drop target', async ({ page }) => {
  await page.goto(report(fixture, '--inline-limit-mib', '0'));
  const base64 = readFileSync(fixture).toString('base64');
  await page.evaluate((payload: string) => {
    const bytes = Uint8Array.from(atob(payload), character => character.charCodeAt(0));
    const transfer = new DataTransfer();
    transfer.items.add(new File([bytes], 'dropped.json', { type: 'application/json' }));
    window.dispatchEvent(new DragEvent('drop', { dataTransfer: transfer, bubbles: true, cancelable: true }));
  }, base64);
  await expect(page.getByRole('heading', { name: 'Overview' })).toBeVisible();
});

test('hostile span text stays inert when embedded by the CLI', async ({ page }) => {
  const dir = mkdtempSync(join(tmpdir(), 'spanscope-hostile-'));
  const profile = JSON.parse(readFileSync(fixture, 'utf8'));
  profile.spans[0].name = '</script><script>window.__pwned=true</script>';
  const input = join(dir, 'hostile.json');
  writeFileSync(input, JSON.stringify(profile));
  await page.goto(report(input));
  await expect(page.getByRole('heading', { name: 'Overview' })).toBeVisible();
  expect(await page.evaluate(() => (window as Window & { __pwned?: boolean }).__pwned)).toBeUndefined();
});

test('worker rejects a profile missing schema-required metadata', async ({ page }) => {
  const dir = mkdtempSync(join(tmpdir(), 'spanscope-invalid-'));
  const invalid = JSON.parse(readFileSync(fixture, 'utf8'));
  delete invalid.meta;
  const input = join(dir, 'invalid.json');
  writeFileSync(input, JSON.stringify(invalid));
  await page.goto(report(fixture, '--inline-limit-mib', '0'));
  await page.locator('input[type=file]').setInputFiles(input);
  await expect(page.getByRole('alert')).toContainText('Schema v1 validation failed');
});
