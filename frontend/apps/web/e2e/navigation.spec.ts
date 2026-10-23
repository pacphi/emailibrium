import { test, expect } from './fixtures/mailbox';

test('sidebar navigates to each main feature with usable content', async ({ page }) => {
  await page.goto('/command-center');
  for (const [link, heading] of [
    ['Insights', 'Insights Explorer'],
    ['Rules', 'Rules Studio'],
    ['Settings', 'Settings'],
    ['Chat', 'Email Assistant'],
    ['Inbox Cleaner', 'Inbox Cleaner'],
    ['Cleanup History', 'Cleanup History'],
  ]) {
    await page.getByRole('link', { name: link!, exact: true }).click();
    await expect(page.getByRole('heading', { name: heading!, exact: true })).toBeVisible();
  }
});
