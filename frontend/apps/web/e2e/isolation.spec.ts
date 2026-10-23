import { test, expect } from './fixtures/mailbox';

test('fixture server rejects API traffic that bypasses browser interception', async ({
  request,
}) => {
  const response = await request.post('/api/v1/emails/send', {
    data: { to: 'nobody@example.test' },
  });
  expect(response.status()).toBe(503);
  expect(await response.text()).toBe('UI tests require an explicit synthetic API fixture');
});
