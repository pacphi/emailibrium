import { test, expect, subscription } from './fixtures/mailbox';
import type { Page } from '@playwright/test';

async function buildPlan(page: Page) {
  await page.goto('/inbox-cleaner');
  await page.getByRole('button', { name: 'Begin Ingestion' }).click();
  await page.getByRole('button', { name: 'Never Opened 1' }).click();
  await page.getByRole('combobox').selectOption('unsubscribe');
  await expect(page.getByText(subscription.senderAddress, { exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Next', exact: true }).click();
  await page.getByRole('button', { name: 'Next', exact: true }).click();
  await page.getByRole('button', { name: 'Continue to Review' }).click();
}

test('builds a reviewable plan before applying low-risk operations', async ({ page, mailbox }) => {
  await buildPlan(page);
  await expect(page.getByRole('heading', { name: /Review/ }).first()).toBeVisible();
  expect(mailbox.matching('POST', 'cleanup/apply/fixture-plan')).toHaveLength(0);
  expect(mailbox.matching('POST', 'cleanup/plan')[0]!.postDataJSON().subscriptions).toEqual([
    { sender: subscription.senderAddress, accountId: 'fixture-account' },
  ]);
  await page.getByRole('button', { name: /Apply Low only/i }).click();
  await expect(page.getByText('Cleanup complete', { exact: true })).toBeVisible();
  await expect(page.getByText(/Applied 1/)).toBeVisible();
  expect(
    new URL(mailbox.matching('POST', 'cleanup/apply/fixture-plan')[0]!.url()).searchParams.get(
      'riskMax',
    ),
  ).toBe('low');
});

test('plan creation failure stays in wizard and never applies', async ({ page, mailbox }) => {
  mailbox.failures.set('cleanup/plan', 400);
  await buildPlan(page);
  await expect(page.getByRole('alert')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Continue to Review' })).toBeEnabled();
  expect(mailbox.matching('POST', 'cleanup/apply/fixture-plan')).toHaveLength(0);
});

test('history opens a read-only plan and its apply history', async ({ page }) => {
  await page.goto('/cleanup/history');
  await page.getByRole('link', { name: /^Plan .*status Ready/ }).click();
  await expect(page).toHaveURL(/\/cleanup\/history\/fixture-plan/);
  await expect(page.getByRole('heading', { name: 'Apply history' })).toBeVisible();
  await expect(page.getByRole('button', { name: /Apply Low only/i })).toHaveCount(0);
});

test('high-risk plan cannot apply without the required acknowledgments', async ({
  page,
  mailbox,
}) => {
  mailbox.cleanupPlan.risk = { low: 0, medium: 0, high: 1 };
  mailbox.cleanupPlan.operations[0]!.risk = 'high';
  mailbox.cleanupPlan.operations[0]!.action = { type: 'delete', permanent: true };
  await buildPlan(page);
  await expect(page.getByRole('heading', { name: 'Acknowledge risky operations' })).toBeVisible();
  await expect(page.getByRole('button', { name: /Apply all/i })).toBeDisabled();
  expect(mailbox.matching('POST', 'cleanup/apply/fixture-plan')).toHaveLength(0);
});
