import { test, expect } from './fixtures/mailbox';

test('onboarding permits skipping services and choosing manual archiving', async ({
  page,
  mailbox,
}) => {
  mailbox.accounts = [];
  await page.goto('/onboarding');
  await expect(page.getByText('Backend connected', { exact: true })).toBeVisible();
  await page.getByRole('button', { name: /Skip.*connect email later/ }).click();
  await page.getByRole('button', { name: "I'll configure this later in Settings" }).click();
  await page.getByRole('radio', { name: /Manual/ }).check();
  await expect(page.getByRole('radio', { name: /Manual/ })).toBeChecked();
  await page.getByRole('button', { name: 'Continue', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'Setup Complete' })).toBeVisible();
  await page.getByRole('button', { name: /Launch|Get Started|Start Using/ }).click();
  await expect(page).toHaveURL(/command-center/);
});

test('provider selection opens Gmail setup and can return to provider choice', async ({ page }) => {
  await page.goto('/onboarding');
  await page.getByRole('button', { name: /Gmail/ }).click();
  await expect(page.getByRole('heading', { name: /Gmail/ })).toBeVisible();
  await page.getByRole('button', { name: /Back/ }).click();
  await expect(page.getByRole('heading', { name: 'Take control of your inbox' })).toBeVisible();
});

test('reports an offline backend on onboarding', async ({ page, mailbox }) => {
  mailbox.failures.set('vectors/health', 503);
  await page.goto('/onboarding');
  await expect(page.getByRole('alert')).toContainText('Backend offline');
});

test('connects a synthetic IMAP account and advances to connected accounts', async ({
  page,
  mailbox,
}) => {
  mailbox.accounts = [];
  await page.goto('/onboarding');
  await page.getByRole('button', { name: /^IMAP/ }).click();
  await page.getByLabel('Email Address', { exact: true }).fill('imap@example.test');
  await page.getByLabel('Password / App Password', { exact: true }).fill('synthetic-password');
  await page.getByLabel('IMAP Server', { exact: true }).fill('imap.example.test');
  await page.getByLabel('SMTP Server', { exact: true }).fill('smtp.example.test');
  await page.getByRole('button', { name: 'Connect Account' }).click();
  await expect(page.getByText('imap@example.test', { exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Continue', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'AI Configuration' })).toBeVisible();
  expect(mailbox.matching('POST', 'auth/imap/connect')[0]!.postDataJSON()).toMatchObject({
    email: 'imap@example.test',
    imapServer: 'imap.example.test',
    smtpServer: 'smtp.example.test',
  });
});
