import { test, expect, subscription } from './fixtures/mailbox';

test('insights renders metrics and filters senders with an explicit empty state', async ({
  page,
}) => {
  await page.goto('/insights');
  await expect(page.getByRole('heading', { name: 'Inbox Health Score' })).toBeVisible();
  await page.getByRole('tab', { name: 'Subscriptions', exact: true }).click();
  await expect(page.getByText('Total Subscriptions', { exact: true })).toBeVisible();
  await expect(page.getByText(subscription.senderAddress, { exact: true })).toBeVisible();
  await page.getByRole('tab', { name: 'Senders', exact: true }).click();
  await page.getByPlaceholder('Search senders...').fill('missing-sender');
  await expect(page.getByText('No senders match your search.')).toBeVisible();
  await page.getByPlaceholder('Search senders...').fill('digest');
  await expect(page.getByRole('cell', { name: /digest@example.test/ })).toBeVisible();
  await page.getByRole('tab', { name: 'Topics', exact: true }).click();
  await expect(page.getByText(/No topic clusters available/)).toBeVisible();
  await page.getByRole('tab', { name: 'Trends', exact: true }).click();
  await expect(page.getByRole('heading', { name: /Volume/ }).first()).toBeVisible();
});
