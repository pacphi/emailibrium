import { test, expect } from '@playwright/test';

test.describe('Search & Command Palette', () => {
  test('command palette opens with Cmd+K', async ({ page }) => {
    await page.goto('/command-center');
    await page.keyboard.press('Meta+k');
    // The command palette dialog should appear
    const dialog = page.getByRole('dialog').first();
    await expect(dialog).toBeVisible({ timeout: 3000 });
  });

  test('can type search query', async ({ page }) => {
    await page.goto('/command-center');
    await page.keyboard.press('Meta+k');
    const searchInput = page
      .getByRole('combobox')
      .or(page.getByPlaceholder(/search/i))
      .first();
    await searchInput.fill('test query');
    await expect(searchInput).toHaveValue('test query');
  });

  test('search results display', async ({ page }) => {
    await page.goto('/command-center');
    await page.keyboard.press('Meta+k');
    const searchInput = page
      .getByRole('combobox')
      .or(page.getByPlaceholder(/search/i))
      .first();
    await searchInput.fill('inbox');
    // Allow time for results to load
    await page.waitForTimeout(500);
    // The command palette should still be visible with results or empty state
    const dialog = page.getByRole('dialog').first();
    await expect(dialog).toBeVisible();
  });
});
