import { test, expect, account, messages } from './fixtures/mailbox';

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

test('composes and sends using the selected account', async ({ page, mailbox }) => {
  await page.goto('/email');
  await page.getByRole('button', { name: 'Compose new email', exact: true }).click();
  const composer = page.getByRole('dialog', { name: 'Compose email' });
  await expect(composer.getByRole('button', { name: /^Send/ })).toBeDisabled();
  await composer.getByLabel('To', { exact: true }).fill('recipient@example.test');
  await composer.getByLabel('Subj', { exact: true }).fill('Synthetic review');
  await composer
    .getByRole('textbox', { name: /body/i })
    .fill('This message never leaves the fixture.');
  await composer.getByRole('button', { name: /^Send/ }).click();
  await expect(composer).toBeHidden();
  expect(mailbox.matching('POST', 'emails/send')[0]!.postDataJSON()).toMatchObject({
    accountId: account.id,
    to: 'recipient@example.test',
    subject: 'Synthetic review',
    bodyText: 'This message never leaves the fixture.',
  });
});

test('retains a composed message when sending fails', async ({ page, mailbox }) => {
  mailbox.failures.set('emails/send', 503);
  await page.goto('/email');
  await page.getByRole('button', { name: 'Compose new email', exact: true }).click();
  const composer = page.getByRole('dialog', { name: 'Compose email' });
  await composer.getByLabel('To', { exact: true }).fill('recipient@example.test');
  await composer.getByLabel('Subj', { exact: true }).fill('Keep this draft');
  await composer.getByRole('button', { name: /^Send/ }).click();
  await expect(composer.getByRole('alert')).toBeVisible();
  await expect(composer.getByLabel('Subj', { exact: true })).toHaveValue('Keep this draft');
});

test('shows a thread error without displaying another message', async ({ page, mailbox }) => {
  mailbox.failures.set('emails/thread/thread-1', 400);
  await page.goto('/email');
  await page.getByText(messages[0]!.subject, { exact: true }).click();
  await expect(page.getByText('Failed to load thread.')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Click to reply...' })).toHaveCount(0);
});
