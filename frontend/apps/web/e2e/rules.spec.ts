import { test, expect } from './fixtures/mailbox';

test('creates a rule after testing its condition and validating it', async ({ page, mailbox }) => {
  await page.goto('/rules');
  await page.getByRole('button', { name: 'New Rule', exact: true }).click();
  await page.getByLabel('Rule Name').fill('Archive Atlas');
  await page.getByLabel('Condition field').selectOption('subject');
  await page.getByLabel('Condition value').fill('Atlas');
  await page.getByLabel('Action type').selectOption('archive');
  await page.getByRole('button', { name: /Test Rule/ }).click();
  await expect(page.getByText(/This rule would match 1 email/)).toBeVisible();
  await page.getByRole('button', { name: 'Create', exact: true }).click();
  await expect(page.getByRole('cell', { name: 'Archive Atlas', exact: true })).toBeVisible();
  expect(mailbox.matching('POST', 'rules/validate')).toHaveLength(1);
  expect(mailbox.rules[0]).toMatchObject({
    name: 'Archive Atlas',
    conditions: [{ field: 'subject', operator: 'contains', value: 'Atlas' }],
    actions: [{ type: 'archive' }],
  });
});

test('opens a template and can cancel without saving a rule', async ({ page, mailbox }) => {
  await page.goto('/rules');
  await page.getByRole('tab', { name: 'Templates' }).click();
  await page.getByRole('button', { name: 'Use Template' }).first().click();
  await expect(page.getByLabel('Rule Name')).not.toHaveValue('');
  await page.getByRole('button', { name: 'Cancel', exact: true }).click();
  await expect(page.getByLabel('Rule Name')).toHaveCount(0);
  expect(mailbox.rules).toEqual([]);
});

test('rules load failures are explicit', async ({ page, mailbox }) => {
  mailbox.failures.set('rules', 400);
  await page.goto('/rules');
  await expect(page.getByText(/Failed to load rules/)).toBeVisible();
});
