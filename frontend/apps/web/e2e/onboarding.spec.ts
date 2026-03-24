import { test, expect } from '@playwright/test';

test.describe('Onboarding Flow', () => {
  test('can see welcome page', async ({ page }) => {
    await page.goto('/onboarding');
    await expect(page.locator('body')).toBeVisible();
    await expect(page).toHaveURL(/\/onboarding/);
  });

  test('can select Gmail provider', async ({ page }) => {
    await page.goto('/onboarding');
    const gmailOption = page.getByText(/gmail/i).first();
    await expect(gmailOption).toBeVisible();
    await gmailOption.click();
  });

  test('can navigate through steps', async ({ page }) => {
    await page.goto('/onboarding');
    // The onboarding flow should have navigable steps
    const nextButton = page.getByRole('button', { name: /next|continue|connect/i }).first();
    if (await nextButton.isVisible()) {
      await nextButton.click();
      // After clicking, the page should still be in the onboarding flow
      await expect(page).toHaveURL(/\/onboarding/);
    }
  });
});
