import { describe, it, expect } from 'vitest';
import type { Email } from '@emailibrium/types';
import {
  normalizeGroupKey,
  extractDomain,
  resolveDisplayName,
  groupByDomain,
  flattenGroups,
  type SenderGroup,
  type DomainGroup,
} from './groupBySender';

/** Helper to create a minimal Email fixture with sensible defaults. */
function makeEmail(
  overrides: Partial<Email> & Pick<Email, 'id' | 'fromAddr' | 'receivedAt'>,
): Email {
  return {
    accountId: 'acc-1',
    provider: 'gmail',
    subject: 'Test subject',
    toAddrs: 'me@example.com',
    isRead: false,
    isStarred: false,
    hasAttachments: false,
    embeddingStatus: 'pending',
    category: 'primary',
    ...overrides,
  };
}

// ---------------------------------------------------------------------------
// normalizeGroupKey
// ---------------------------------------------------------------------------
describe('normalizeGroupKey', () => {
  it('lowercases the address', () => {
    expect(normalizeGroupKey('Alice@Example.COM')).toBe('alice@example.com');
  });

  it('trims leading and trailing whitespace', () => {
    expect(normalizeGroupKey('  bob@test.com  ')).toBe('bob@test.com');
  });

  it('handles already normalized input', () => {
    expect(normalizeGroupKey('carol@test.com')).toBe('carol@test.com');
  });

  it('extracts email from RFC 2822 angle-bracket format', () => {
    expect(normalizeGroupKey('"John Smith" <john@example.com>')).toBe('john@example.com');
  });

  it('extracts email from unquoted display name format', () => {
    expect(normalizeGroupKey('John Smith <john@example.com>')).toBe('john@example.com');
  });
});

// ---------------------------------------------------------------------------
// extractDomain
// ---------------------------------------------------------------------------
describe('extractDomain', () => {
  it('extracts the root domain from a plain email', () => {
    expect(extractDomain('john@gmail.com')).toBe('gmail.com');
  });

  it('lowercases the domain', () => {
    expect(extractDomain('alice@CORP.COM')).toBe('corp.com');
  });

  it('returns "unknown" for strings without @', () => {
    expect(extractDomain('notanemail')).toBe('unknown');
  });

  it('strips subdomains to root domain', () => {
    expect(extractDomain('user@newsletter.launchpadfast.com')).toBe('launchpadfast.com');
    expect(extractDomain('user@email.github.com')).toBe('github.com');
    expect(extractDomain('user@mail.replit.com')).toBe('replit.com');
    expect(extractDomain('user@hello.mindvalley.com')).toBe('mindvalley.com');
    expect(extractDomain('user@accountprotection.microsoft.com')).toBe('microsoft.com');
  });

  it('preserves 2-part domains unchanged', () => {
    expect(extractDomain('user@slack.com')).toBe('slack.com');
    expect(extractDomain('user@heroforge.ai')).toBe('heroforge.ai');
  });

  it('handles compound TLDs (co.uk) correctly', () => {
    expect(extractDomain('user@mail.example.co.uk')).toBe('example.co.uk');
    expect(extractDomain('user@acme.co.uk')).toBe('acme.co.uk');
  });
});

// ---------------------------------------------------------------------------
// resolveDisplayName
// ---------------------------------------------------------------------------
describe('resolveDisplayName', () => {
  it('picks the most common fromName', () => {
    const emails: Email[] = [
      makeEmail({
        id: '1',
        fromAddr: 'a@b.com',
        fromName: 'Alice',
        receivedAt: '2024-01-03T00:00:00Z',
      }),
      makeEmail({
        id: '2',
        fromAddr: 'a@b.com',
        fromName: 'Alice B',
        receivedAt: '2024-01-02T00:00:00Z',
      }),
      makeEmail({
        id: '3',
        fromAddr: 'a@b.com',
        fromName: 'Alice',
        receivedAt: '2024-01-01T00:00:00Z',
      }),
    ];
    expect(resolveDisplayName(emails)).toBe('Alice');
  });

  it('falls back to fromAddr when all fromName values are undefined', () => {
    const emails: Email[] = [
      makeEmail({ id: '1', fromAddr: 'nobody@test.com', receivedAt: '2024-01-02T00:00:00Z' }),
      makeEmail({ id: '2', fromAddr: 'nobody@test.com', receivedAt: '2024-01-01T00:00:00Z' }),
    ];
    expect(resolveDisplayName(emails)).toBe('nobody@test.com');
  });

  it('tie-breaks by picking the name on the most recent email', () => {
    const emails: Email[] = [
      makeEmail({
        id: '1',
        fromAddr: 'x@y.com',
        fromName: 'Beta',
        receivedAt: '2024-06-02T00:00:00Z',
      }),
      makeEmail({
        id: '2',
        fromAddr: 'x@y.com',
        fromName: 'Alpha',
        receivedAt: '2024-06-01T00:00:00Z',
      }),
    ];
    expect(resolveDisplayName(emails)).toBe('Beta');
  });

  it('ignores empty-string fromName values', () => {
    const emails: Email[] = [
      makeEmail({
        id: '1',
        fromAddr: 'x@y.com',
        fromName: '  ',
        receivedAt: '2024-01-02T00:00:00Z',
      }),
      makeEmail({
        id: '2',
        fromAddr: 'x@y.com',
        fromName: 'Real Name',
        receivedAt: '2024-01-01T00:00:00Z',
      }),
    ];
    expect(resolveDisplayName(emails)).toBe('Real Name');
  });

  it('returns empty string for an empty array', () => {
    expect(resolveDisplayName([])).toBe('');
  });
});

// ---------------------------------------------------------------------------
// groupByDomain
// ---------------------------------------------------------------------------
describe('groupByDomain', () => {
  it('returns empty array for empty input', () => {
    expect(groupByDomain([])).toEqual([]);
  });

  it('creates a single domain group for emails from the same domain', () => {
    const emails: Email[] = [
      makeEmail({ id: '1', fromAddr: 'alice@gmail.com', receivedAt: '2024-01-02T00:00:00Z' }),
      makeEmail({ id: '2', fromAddr: 'bob@gmail.com', receivedAt: '2024-01-01T00:00:00Z' }),
    ];
    const domains = groupByDomain(emails);
    expect(domains).toHaveLength(1);
    expect(domains[0]!.domain).toBe('gmail.com');
    expect(domains[0]!.senderGroups).toHaveLength(2);
    expect(domains[0]!.totalEmails).toBe(2);
  });

  it('creates separate domain groups for emails from different domains', () => {
    const emails: Email[] = [
      makeEmail({ id: '1', fromAddr: 'alice@gmail.com', receivedAt: '2024-01-02T00:00:00Z' }),
      makeEmail({ id: '2', fromAddr: 'bob@outlook.com', receivedAt: '2024-01-01T00:00:00Z' }),
    ];
    const domains = groupByDomain(emails);
    expect(domains).toHaveLength(2);
    const domainNames = domains.map((d) => d.domain);
    expect(domainNames).toContain('gmail.com');
    expect(domainNames).toContain('outlook.com');
  });

  it('sorts domains A-Z', () => {
    const emails: Email[] = [
      makeEmail({ id: '1', fromAddr: 'x@zebra.com', receivedAt: '2024-01-03T00:00:00Z' }),
      makeEmail({ id: '2', fromAddr: 'x@alpha.com', receivedAt: '2024-01-02T00:00:00Z' }),
      makeEmail({ id: '3', fromAddr: 'x@mango.com', receivedAt: '2024-01-01T00:00:00Z' }),
    ];
    const domains = groupByDomain(emails);
    expect(domains.map((d) => d.domain)).toEqual(['alpha.com', 'mango.com', 'zebra.com']);
  });

  it('sorts sender groups within a domain A-Z by displayName', () => {
    const emails: Email[] = [
      makeEmail({
        id: '1',
        fromAddr: 'zoe@corp.com',
        fromName: 'Zoe',
        receivedAt: '2024-01-03T00:00:00Z',
      }),
      makeEmail({
        id: '2',
        fromAddr: 'alice@corp.com',
        fromName: 'Alice',
        receivedAt: '2024-01-02T00:00:00Z',
      }),
      makeEmail({
        id: '3',
        fromAddr: 'mike@corp.com',
        fromName: 'Mike',
        receivedAt: '2024-01-01T00:00:00Z',
      }),
    ];
    const domains = groupByDomain(emails);
    const senderNames = domains[0]!.senderGroups.map((g) => g.displayName);
    expect(senderNames).toEqual(['Alice', 'Mike', 'Zoe']);
  });

  it('sorts emails within each sender group newest-first', () => {
    const emails: Email[] = [
      makeEmail({ id: '1', fromAddr: 'sam@test.com', receivedAt: '2024-01-01T00:00:00Z' }),
      makeEmail({ id: '2', fromAddr: 'sam@test.com', receivedAt: '2024-06-01T00:00:00Z' }),
      makeEmail({ id: '3', fromAddr: 'sam@test.com', receivedAt: '2024-03-01T00:00:00Z' }),
    ];
    const domains = groupByDomain(emails);
    const group = domains[0]!.senderGroups[0]!;
    expect(group.emails.map((e) => e.id)).toEqual(['2', '3', '1']);
  });

  it('merges emails with different casing of the same fromAddr into one sender group', () => {
    const emails: Email[] = [
      makeEmail({ id: '1', fromAddr: 'Alice@Corp.com', receivedAt: '2024-03-02T00:00:00Z' }),
      makeEmail({ id: '2', fromAddr: 'alice@corp.com', receivedAt: '2024-03-01T00:00:00Z' }),
    ];
    const domains = groupByDomain(emails);
    expect(domains[0]!.senderGroups).toHaveLength(1);
    expect(domains[0]!.senderGroups[0]!.emails).toHaveLength(2);
  });

  it('totals emails correctly across multiple sender groups', () => {
    const emails: Email[] = [
      makeEmail({ id: '1', fromAddr: 'a@corp.com', receivedAt: '2024-01-03T00:00:00Z' }),
      makeEmail({ id: '2', fromAddr: 'a@corp.com', receivedAt: '2024-01-02T00:00:00Z' }),
      makeEmail({ id: '3', fromAddr: 'b@corp.com', receivedAt: '2024-01-01T00:00:00Z' }),
    ];
    const domains = groupByDomain(emails);
    expect(domains[0]!.totalEmails).toBe(3);
  });
});

// ---------------------------------------------------------------------------
// flattenGroups (2-level: domains → senders → emails)
// ---------------------------------------------------------------------------
describe('flattenGroups', () => {
  const senderAlice: SenderGroup = {
    key: 'alice@test.com',
    displayName: 'Alice',
    fromAddr: 'alice@test.com',
    emails: [
      makeEmail({ id: 'a1', fromAddr: 'alice@test.com', receivedAt: '2024-06-02T00:00:00Z' }),
      makeEmail({ id: 'a2', fromAddr: 'alice@test.com', receivedAt: '2024-06-01T00:00:00Z' }),
    ],
    latestTimestamp: '2024-06-02T00:00:00Z',
    provider: 'gmail',
  };

  const senderBob: SenderGroup = {
    key: 'bob@other.com',
    displayName: 'Bob',
    fromAddr: 'bob@other.com',
    emails: [
      makeEmail({ id: 'b1', fromAddr: 'bob@other.com', receivedAt: '2024-05-01T00:00:00Z' }),
    ],
    latestTimestamp: '2024-05-01T00:00:00Z',
    provider: 'outlook',
  };

  const domainTest: DomainGroup = {
    domain: 'test.com',
    senderGroups: [senderAlice],
    totalEmails: 2,
  };

  const domainOther: DomainGroup = {
    domain: 'other.com',
    senderGroups: [senderBob],
    totalEmails: 1,
  };

  it('returns only domain headers when both expand sets are empty (all collapsed)', () => {
    const items = flattenGroups([domainTest, domainOther], new Set(), new Set());
    expect(items).toHaveLength(2);
    expect(items[0]).toEqual({ type: 'domain-header', domain: domainTest });
    expect(items[1]).toEqual({ type: 'domain-header', domain: domainOther });
  });

  it('shows sender headers when domain is expanded', () => {
    const items = flattenGroups([domainTest], new Set(['test.com']), new Set());
    expect(items).toHaveLength(2); // domain-header + sender-header (alice still collapsed)
    expect(items[0]).toEqual({ type: 'domain-header', domain: domainTest });
    expect(items[1]).toEqual({ type: 'sender-header', group: senderAlice, domain: 'test.com' });
  });

  it('shows emails when domain and sender are both expanded', () => {
    const items = flattenGroups([domainTest], new Set(['test.com']), new Set(['alice@test.com']));
    expect(items).toHaveLength(4); // domain + sender + 2 emails
    expect(items[0]).toEqual({ type: 'domain-header', domain: domainTest });
    expect(items[1]).toEqual({ type: 'sender-header', group: senderAlice, domain: 'test.com' });
    expect(items[2]).toEqual({
      type: 'email',
      email: senderAlice.emails[0],
      groupKey: 'alice@test.com',
    });
    expect(items[3]).toEqual({
      type: 'email',
      email: senderAlice.emails[1],
      groupKey: 'alice@test.com',
    });
  });

  it('handles mixed collapse state across domains', () => {
    const items = flattenGroups(
      [domainTest, domainOther],
      new Set(['other.com']),
      new Set(['bob@other.com']),
    );
    // test.com collapsed: 1 item; other.com expanded + bob expanded: 1 + 1 + 1 = 3 items
    expect(items).toHaveLength(4);
    expect(items[0]).toEqual({ type: 'domain-header', domain: domainTest });
    expect(items[1]).toEqual({ type: 'domain-header', domain: domainOther });
    expect(items[2]).toEqual({ type: 'sender-header', group: senderBob, domain: 'other.com' });
    expect(items[3]).toEqual({
      type: 'email',
      email: senderBob.emails[0],
      groupKey: 'bob@other.com',
    });
  });

  it('returns empty array for empty input', () => {
    expect(flattenGroups([], new Set(), new Set())).toEqual([]);
  });
});
