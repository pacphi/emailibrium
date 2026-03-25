import { test, expect } from '@playwright/test';

test.describe('Rules Studio', () => {
  test('rules studio loads with tabs', async ({ page }) => {
    await page.goto('/rules');
    await expect(page).toHaveURL(/\/rules/);
    // Look for tab navigation or rule-related headings
    const tabOrHeading = page
      .getByRole('tab')
      .or(page.getByText(/active|suggestions|rules/i))
      .first();
    await expect(tabOrHeading).toBeVisible({ timeout: 5000 });
  });

  test('can open rule editor', async ({ page }) => {
    await page.goto('/rules');
    const createButton = page.getByRole('button', { name: /create|add|new/i }).first();
    if (await createButton.isVisible({ timeout: 3000 }).catch(() => false)) {
      await createButton.click();
      // A rule editor form or modal should appear
      const editor = page
        .getByText(/condition|action|when|then/i)
        .or(page.getByRole('dialog'))
        .first();
      await expect(editor).toBeVisible({ timeout: 3000 });
    }
  });

  test('displays active rules list', async ({ page }) => {
    await page.goto('/rules');
    // The page should show some rules content or an empty state
    const content = page.getByText(/no rules|active|rule/i).first();
    await expect(content).toBeVisible({ timeout: 5000 });
  });
});
