import { test, expect } from './fixtures/mailbox';

test('chat streams an answer and clears the conversation', async ({ page, mailbox }) => {
  await page.goto('/chat');
  await page.getByRole('textbox', { name: 'Chat message input' }).fill('What needs review?');
  await page.getByRole('button', { name: 'Send message', exact: true }).click();
  const messages = page.getByRole('list', { name: 'Chat messages' });
  await expect(messages).toContainText('What needs review?');
  await expect(messages).toContainText('Atlas needs a review by Friday.');
  await expect(page.getByRole('button', { name: 'Stop generating' })).toHaveCount(0);
  expect(mailbox.matching('POST', 'ai/chat/stream')[0]!.postDataJSON().message).toBe(
    'What needs review?',
  );
  await page.getByRole('button', { name: 'Clear chat history' }).click();
  await expect(messages).not.toContainText('Atlas needs a review by Friday.');
});

test('chat exposes a service error and becomes ready for another message', async ({
  page,
  mailbox,
}) => {
  mailbox.failures.set('ai/chat/stream', 503);
  await page.goto('/chat');
  await page.getByRole('textbox', { name: 'Chat message input' }).fill('Find Atlas');
  await page.getByRole('button', { name: 'Send message', exact: true }).click();
  await expect(page.getByRole('list', { name: 'Chat messages' })).toContainText(
    'Chat request failed: 503',
  );
  await page.getByRole('textbox', { name: 'Chat message input' }).fill('Retry Atlas');
  await expect(page.getByRole('button', { name: 'Send message', exact: true })).toBeEnabled();
});
