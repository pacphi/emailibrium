import { test, expect, messages } from './fixtures/mailbox';

test('reads a thread and sends a reply to the selected message', async ({ page, mailbox }) => {
  await page.goto('/email');
  await page.getByText(messages[0]!.subject, { exact: true }).click();
  await expect(page.getByRole('heading', { name: messages[0]!.subject })).toBeVisible();
  await expect(page.getByText(messages[0]!.bodyText!, { exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Click to reply...' }).click();
  await page.getByRole('textbox', { name: 'Reply message body' }).fill('Atlas review completed.');
  await page.getByRole('button', { name: 'Send reply' }).click();
  await expect(page.getByRole('button', { name: 'Click to reply...' })).toBeVisible();
  await expect.poll(() => mailbox.matching('POST', 'emails/mail-1/reply').length).toBe(1);
  expect(mailbox.matching('POST', 'emails/mail-1/reply')[0]!.postDataJSON().bodyText).toContain(
    'Atlas review completed.',
  );
});

test('shows a thread error without displaying another message', async ({ page, mailbox }) => {
  mailbox.failures.set('emails/thread/thread-1', 400);
  await page.goto('/email');
  await page.getByText(messages[0]!.subject, { exact: true }).click();
  await expect(page.getByText('Failed to load thread.')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Click to reply...' })).toHaveCount(0);
});
