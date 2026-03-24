import { test, expect } from '@playwright/test';

test.describe('Email Client', () => {
  test('email client loads with sidebar', async ({ page }) => {
    await page.goto('/email');
    await expect(page).toHaveURL(/\/email/);
    // The sidebar should be visible with folder navigation
    const sidebar = page.getByRole('navigation')
      .or(page.getByText(/inbox|sent|drafts/i).first());
    await expect(sidebar).toBeVisible({ timeout: 5000 });
  });

  test('can compose new email', async ({ page }) => {
    await page.goto('/email');
    const composeButton = page.getByRole('button', { name: /compose|new|write/i }).first();
    await expect(composeButton).toBeVisible({ timeout: 5000 });
    await composeButton.click();
    // After clicking compose, a compose form or modal should appear
    const composeArea = page.getByPlaceholder(/to|recipient|subject/i)
      .or(page.getByText(/compose|new message/i))
      .first();
    await expect(composeArea).toBeVisible({ timeout: 3000 });
  });
});
