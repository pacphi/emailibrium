import { test, expect } from './fixtures/mailbox';
import type { Page } from '@playwright/test';

async function sessionBoundary(page: Page) {
  const state = { authenticated: false };
  await page.route('**/api/v1/session', async (route) => {
    const request = route.request();
    if (request.method() === 'POST')
      state.authenticated =
        request.headers().authorization === 'Bearer synthetic-local-access-token-32-bytes';
    if (request.method() === 'DELETE') state.authenticated = false;
    await route.fulfill({
      status: state.authenticated || request.method() === 'DELETE' ? 200 : 401,
      json: { authenticated: state.authenticated },
    });
  });
  return state;
}

test('requires an access token before private mailbox screens mount', async ({ page, mailbox }) => {
  await sessionBoundary(page);
  await page.goto('/command-center');
  await expect(page.getByRole('heading', { name: 'Connect to your engine' })).toBeVisible();
  await expect(page.getByRole('link', { name: 'Email', exact: true })).toHaveCount(0);
  expect(mailbox.matching('GET', 'auth/accounts')).toHaveLength(0);
});

test('rejects an incorrect local access token with an actionable error', async ({ page }) => {
  await sessionBoundary(page);
  await page.goto('/command-center');
  await page.getByLabel('Local access token').fill('incorrect-token');
  await page.getByRole('button', { name: 'Connect securely' }).click();
  await expect(page.getByRole('alert')).toContainText('Access token was not accepted');
  await expect(page.getByRole('link', { name: 'Email', exact: true })).toHaveCount(0);
});

test('connects without storing the token, restores a session, and locks it', async ({ page }) => {
  await sessionBoundary(page);
  await page.goto('/command-center');
  await page.getByLabel('Local access token').fill('synthetic-local-access-token-32-bytes');
  await page.getByRole('button', { name: 'Connect securely' }).click();
  await expect(page.getByRole('heading', { name: 'Command Center' })).toBeVisible();
  expect(await page.evaluate(() => JSON.stringify(localStorage))).not.toContain(
    'synthetic-local-access-token',
  );
  expect(page.url()).not.toContain('synthetic-local-access-token');
  await page.reload();
  await expect(page.getByRole('heading', { name: 'Command Center' })).toBeVisible();
  await page.getByRole('button', { name: 'Lock engine' }).click();
  await expect(page.getByRole('heading', { name: 'Connect to your engine' })).toBeVisible();
  await page.reload();
  await expect(page.getByRole('heading', { name: 'Connect to your engine' })).toBeVisible();
});

test('an expired session returns to connection setup on focus', async ({ page }) => {
  const session = await sessionBoundary(page);
  session.authenticated = true;
  await page.goto('/command-center');
  await expect(page.getByRole('heading', { name: 'Command Center' })).toBeVisible();
  session.authenticated = false;
  await page.evaluate(() => window.dispatchEvent(new Event('focus')));
  await expect(page.getByRole('heading', { name: 'Connect to your engine' })).toBeVisible();
});

test('an unreachable engine shows a retry action', async ({ page }) => {
  await page.route('**/api/v1/session', (route) => route.abort('connectionrefused'));
  await page.goto('/command-center');
  await expect(page.getByRole('alert')).toContainText('Cannot reach the local engine');
  await expect(page.getByRole('button', { name: 'Retry connection' })).toBeVisible();
});

test('a late session check cannot reopen the mailbox after locking', async ({ page }) => {
  const session = await sessionBoundary(page);
  session.authenticated = true;
  await page.goto('/command-center');
  await expect(page.getByRole('heading', { name: 'Command Center' })).toBeVisible();
  let delayed: import('@playwright/test').Route | undefined;
  await page.route('**/api/v1/session', async (route) => {
    if (route.request().method() === 'GET') {
      delayed = route;
      return;
    }
    await route.fallback();
  });
  await page.evaluate(() => window.dispatchEvent(new Event('focus')));
  await expect.poll(() => Boolean(delayed)).toBe(true);
  await page.getByRole('button', { name: 'Lock engine' }).click();
  await expect(page.getByRole('heading', { name: 'Connect to your engine' })).toBeVisible();
  const response = page.waitForResponse(
    (r) => r.url().endsWith('/api/v1/session') && r.request().method() === 'GET',
  );
  await delayed!.fulfill({ status: 200, json: { authenticated: true } });
  await (await response).finished();
  // Let the fulfilled fetch and React's following paint complete, without a fixed sleep.
  await page.evaluate(
    () =>
      new Promise<void>((resolve) =>
        requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
      ),
  );
  await expect(page.getByRole('heading', { name: 'Connect to your engine' })).toBeVisible();
  await expect(page.getByRole('heading', { name: 'Command Center' })).toHaveCount(0);
});
