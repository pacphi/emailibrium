import { afterEach, describe, expect, it, vi } from 'vitest';
import { connectEngine, getEngineSession, lockEngine } from '../sessionApi.js';

afterEach(() => vi.unstubAllGlobals());

describe('local engine session transport', () => {
  it('recognizes a missing or expired session without treating it as success', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('{}', { status: 401 })));
    expect(await getEngineSession()).toBe(false);
  });

  it('sends an unlock credential only in the authorization header', async () => {
    const fetch = vi.fn().mockResolvedValue(new Response('{}'));
    vi.stubGlobal('fetch', fetch);
    await connectEngine('synthetic-local-access-token-32-bytes');
    expect(fetch).toHaveBeenCalledWith(
      '/api/v1/session',
      expect.objectContaining({
        method: 'POST',
        credentials: 'same-origin',
        headers: { Authorization: 'Bearer synthetic-local-access-token-32-bytes' },
      }),
    );
    expect(fetch.mock.calls[0]![1]).not.toHaveProperty('body');
  });

  it('reports an incorrect unlock credential', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('{}', { status: 401 })));
    await expect(connectEngine('wrong')).rejects.toThrow('Access token was not accepted');
  });

  it('revokes the browser session and does not claim success on failure', async () => {
    const fetch = vi.fn().mockResolvedValue(new Response('{}', { status: 503 }));
    vi.stubGlobal('fetch', fetch);
    await expect(lockEngine()).rejects.toThrow('Unable to revoke');
    expect(fetch).toHaveBeenCalledWith(
      '/api/v1/session',
      expect.objectContaining({ method: 'DELETE', credentials: 'same-origin' }),
    );
  });
});
