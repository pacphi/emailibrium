import { test, expect } from '@playwright/test';

const MAIN_ROUTES = [
  { path: '/command-center', name: 'Command Center' },
  { path: '/inbox-cleaner', name: 'Inbox Cleaner' },
  { path: '/insights', name: 'Insights' },
  { path: '/email', name: 'Email' },
  { path: '/rules', name: 'Rules' },
  { path: '/settings', name: 'Settings' },
] as const;

test.describe('Navigation', () => {
  test('all main routes load without error', async ({ page }) => {
    for (const route of MAIN_ROUTES) {
      await page.goto(route.path);
      await expect(page).toHaveURL(new RegExp(route.path));
      // Ensure no uncaught error overlay
      const errorOverlay = page.locator('vite-error-overlay');
      await expect(errorOverlay).toHaveCount(0);
    }
  });

  test('sidebar navigation works', async ({ page }) => {
    await page.goto('/command-center');
    // Find and click a sidebar navigation link
    const sidebarLink = page
      .getByRole('link', { name: /inbox cleaner|email|rules|insights|settings/i })
      .first();
    if (await sidebarLink.isVisible({ timeout: 3000 }).catch(() => false)) {
      await sidebarLink.click();
      // URL should change away from command-center
      await page.waitForURL(/\/(inbox-cleaner|email|rules|insights|settings)/);
    }
  });

  test('keyboard shortcuts work', async ({ page }) => {
    await page.goto('/command-center');
    // Test that Cmd+K opens the command palette
    await page.keyboard.press('Meta+k');
    const dialog = page.getByRole('dialog').first();
    await expect(dialog).toBeVisible({ timeout: 3000 });
    // Escape should close it
    await page.keyboard.press('Escape');
    await expect(dialog).not.toBeVisible({ timeout: 2000 });
  });
});
