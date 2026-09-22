import { test, expect } from './fixtures/mailbox';

test('command palette can navigate with keyboard and dismiss with Escape', async ({ page }) => {
  await page.goto('/email');
  await page.keyboard.press('ControlOrMeta+k');
  const palette = page.getByRole('dialog', { name: 'Command palette' });
  await expect(palette).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(palette).toBeHidden();
  await page.keyboard.press('ControlOrMeta+k');
  await palette.getByText('Manage Rules', { exact: true }).click();
  await expect(page.getByRole('heading', { name: 'Rules Studio' })).toBeVisible();
});

test('dashboard search changes modes and distinguishes empty results from service errors', async ({
  page,
  mailbox,
}) => {
  await page.goto('/command-center');
  await page.getByRole('button', { name: 'Search', exact: true }).click();
  await page.getByRole('textbox', { name: 'Search emails', exact: true }).fill('Atlas');
  await expect(page.getByRole('list', { name: 'Search results' })).toContainText(
    'Project Atlas review',
  );
  await page.getByRole('button', { name: 'Keyword', exact: true }).click();
  await expect(page.getByRole('button', { name: 'Keyword', exact: true })).toHaveAttribute(
    'aria-pressed',
    'true',
  );
  await page.getByRole('textbox', { name: 'Search emails', exact: true }).fill('missing');
  await expect(page.getByRole('heading', { name: 'No results found' })).toBeVisible();
  mailbox.failures.set('vectors/search/hybrid', 400);
  await page.getByRole('textbox', { name: 'Search emails', exact: true }).fill('unavailable');
  await expect(page.getByRole('alert')).toContainText('Failed to load search results');
});
