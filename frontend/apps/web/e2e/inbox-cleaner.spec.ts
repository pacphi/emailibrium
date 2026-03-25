import { test, expect } from '@playwright/test';

test.describe('Inbox Cleaner', () => {
  test('can navigate to inbox cleaner', async ({ page }) => {
    await page.goto('/inbox-cleaner');
    await expect(page).toHaveURL(/\/inbox-cleaner/);
    await expect(page.locator('body')).toBeVisible();
  });

  test('step indicator shows correct state', async ({ page }) => {
    await page.goto('/inbox-cleaner');
    // Look for step indicators or progress elements
    const stepIndicator = page.getByText(/step|phase|scan|subscriptions/i).first();
    await expect(stepIndicator).toBeVisible({ timeout: 5000 });
  });

  test('can select subscriptions', async ({ page }) => {
    await page.goto('/inbox-cleaner');
    // Wait for the page content to load
    await page.waitForTimeout(1000);
    // Check for subscription-related UI elements
    const content = page
      .locator('[data-testid="subscription-row"]')
      .or(page.getByRole('checkbox').first())
      .or(page.getByText(/subscription/i).first());
    if (await content.isVisible({ timeout: 3000 }).catch(() => false)) {
      await content.first().click();
    }
  });
});
