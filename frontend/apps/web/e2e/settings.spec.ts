import { test, expect, account } from './fixtures/mailbox';

test('general and appearance choices persist across a reload', async ({ page, mailbox }) => {
  await page.goto('/settings');
  await page.getByLabel('Sync Frequency').selectOption('15');
  await page.getByRole('button', { name: 'Appearance', exact: true }).click();
  await page.getByText('Dark', { exact: true }).click();
  await page.getByRole('button', { name: 'Right', exact: true }).click();
  await expect.poll(() => mailbox.settings.theme).toBe('dark');
  await page.reload();
  await expect(page.getByLabel('Sync Frequency')).toHaveValue('15');
  await page.getByRole('button', { name: 'Appearance', exact: true }).click();
  await expect(page.getByRole('radio', { name: /Dark Always/ })).toBeChecked();
  expect(mailbox.settings.sidebarPosition).toBe('right');
});

test('accounts and model settings reflect connected data and provider selection', async ({
  page,
  mailbox,
}) => {
  await page.goto('/settings');
  await page.getByRole('button', { name: 'Accounts', exact: true }).click();
  await expect(page.getByText(account.emailAddress, { exact: true })).toBeVisible();
  await page.getByRole('combobox').first().selectOption('instant');
  await expect.poll(() => mailbox.accounts[0]?.archiveStrategy).toBe('instant');
  await page.getByRole('button', { name: 'AI / LLM', exact: true }).click();
  await expect(page.getByLabel('Embedding Model', { exact: true })).toHaveValue('all-MiniLM-L6-v2');
  await expect(page.getByLabel('LLM Model', { exact: true })).toContainText('Fixture Small Model');
  await page.getByRole('radio', { name: /None \(Rule-based\)/ }).check();
  await expect.poll(() => mailbox.settings.llmProvider).toBe('none');
  await expect(page.getByRole('link', { name: 'Chat', exact: true })).toHaveCount(0);
  await page.getByRole('button', { name: 'Privacy', exact: true }).click();
  await expect(page.getByText(/Data Retention/).first()).toBeVisible();
  await page.getByRole('button', { name: 'Consent / GDPR', exact: true }).click();
  await expect(page.getByRole('heading', { name: /Consent/ }).first()).toBeVisible();
});
