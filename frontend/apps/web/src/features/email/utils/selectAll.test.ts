import { describe, it, expect } from 'vitest';
import type { Email } from '@emailibrium/types';
import { selectAllEmailIds, selectVisibleGroupedEmailIds } from './selectAll';
import { groupByDomain } from './groupBySender';

function email(id: string, fromAddr = 'sender@example.com'): Email {
  return {
    id,
    accountId: 'acct-1',
    provider: 'gmail',
    subject: `Subject ${id}`,
    fromAddr,
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

describe('selectVisibleGroupedEmailIds', () => {
  // Two domains, one sender each -- built through the REAL groupByDomain, so "visible"
  // here is by construction what GroupedEmailList renders via flattenGroups.
  const emails = [
    email('a1', 'alice@alpha.com'),
    email('a2', 'alice@alpha.com'),
    email('b1', 'bob@beta.com'),
  ];
  const domains = groupByDomain(emails);

  it('selects nothing when every group is collapsed (no email rows are visible)', () => {
    const result = selectVisibleGroupedEmailIds(domains, new Set(), new Set());

    expect(result.size).toBe(0);
  });

  it('selects nothing when a domain is expanded but its sender groups are still collapsed', () => {
    const result = selectVisibleGroupedEmailIds(domains, new Set(['alpha.com']), new Set());

    expect(result.size).toBe(0);
  });

  it('selects only the emails under an expanded domain AND expanded sender', () => {
    const result = selectVisibleGroupedEmailIds(
      domains,
      new Set(['alpha.com']),
      new Set(['alice@alpha.com']),
    );

    expect(result).toEqual(new Set(['a1', 'a2']));
  });

  it('never includes loaded-but-collapsed emails from another domain', () => {
    const result = selectVisibleGroupedEmailIds(
      domains,
      new Set(['alpha.com']),
      new Set(['alice@alpha.com', 'bob@beta.com']),
    );

    expect(result.has('b1')).toBe(false);
  });
});
