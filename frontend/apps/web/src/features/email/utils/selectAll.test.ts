import { describe, it, expect } from 'vitest';
import type { Email } from '@emailibrium/types';
import { selectAllEmailIds } from './selectAll';

function email(id: string): Email {
  return {
    id,
    accountId: 'acct-1',
    provider: 'gmail',
    subject: `Subject ${id}`,
    fromAddr: 'sender@example.com',
    toAddrs: 'me@example.com',
    receivedAt: new Date().toISOString(),
    isRead: true,
    isStarred: false,
    hasAttachments: false,
    embeddingStatus: 'embedded',
    category: 'primary',
  };
}

describe('selectAllEmailIds', () => {
  it('selects every id in the given list', () => {
    const emails = [email('1'), email('2'), email('3')];

    expect(selectAllEmailIds(emails)).toEqual(new Set(['1', '2', '3']));
  });

  it('returns an empty set for an empty (fully filtered-out) view', () => {
    expect(selectAllEmailIds([])).toEqual(new Set());
  });

  it('only selects ids from the passed-in (already filtered) list, never anything else', () => {
    const visible = [email('a'), email('b')];

    const result = selectAllEmailIds(visible);

    expect(result.has('c')).toBe(false);
    expect(result.size).toBe(2);
  });
});
