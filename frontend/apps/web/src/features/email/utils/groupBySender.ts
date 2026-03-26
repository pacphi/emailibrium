import type { Email } from '@emailibrium/types';

/**
 * Parse a RFC 2822 "From" field which may be in any of these formats:
 *   - plain email: `john@example.com`
 *   - with display name: `John Smith <john@example.com>`
 *   - with quoted display name: `"John Smith" <john@example.com>`
 */
function parseFromAddr(fromAddr: string): { email: string; name: string | null } {
  const angleMatch = fromAddr.match(/^(.*?)\s*<([^>]+)>\s*$/);
  if (angleMatch) {
    const rawName = angleMatch[1]!.trim().replace(/^["']|["']$/g, '');
    return { email: angleMatch[2]!.trim(), name: rawName || null };
  }
  return { email: fromAddr.trim(), name: null };
}

/**
 * Normalize an email address for use as a grouping key.
 * Extracts just the email address (stripping display name) and lowercases it.
 */
export function normalizeGroupKey(fromAddr: string): string {
  return parseFromAddr(fromAddr).email.toLowerCase();
}

// Known compound TLDs that require 3 parts to identify the registrable domain
// e.g. "co.uk" means "acme.co.uk" not "co.uk" is the root
const COMPOUND_TLDS = new Set([
  'co.uk',
  'co.nz',
  'co.za',
  'co.in',
  'co.jp',
  'co.kr',
  'com.au',
  'com.br',
  'com.mx',
  'com.ar',
  'com.cn',
  'org.uk',
  'net.uk',
  'gov.uk',
  'ac.uk',
  'org.au',
  'net.au',
]);

/**
 * Normalize a full subdomain to its registrable root domain.
 * e.g. "newsletter.launchpadfast.com" → "launchpadfast.com"
 *      "email.github.com" → "github.com"
 *      "acme.co.uk" → "acme.co.uk" (compound TLD handled)
 */
function normalizeToRootDomain(domain: string): string {
  const parts = domain.toLowerCase().split('.');
  if (parts.length <= 2) return domain;

  // Check for compound TLD (e.g. "co.uk")
  const lastTwo = parts.slice(-2).join('.');
  if (COMPOUND_TLDS.has(lastTwo)) {
    return parts.slice(-3).join('.');
  }

  return parts.slice(-2).join('.');
}

/**
 * Extract the registrable root domain from a normalized email address.
 * e.g. "user@newsletter.launchpadfast.com" → "launchpadfast.com"
 */
export function extractDomain(email: string): string {
  const atIndex = email.lastIndexOf('@');
  if (atIndex === -1) return 'unknown';
  const rawDomain = email.slice(atIndex + 1).toLowerCase();
  return normalizeToRootDomain(rawDomain);
}

export interface SenderGroup {
  /** Normalized fromAddr used as grouping key */
  key: string;
  /** Most common fromName in the group, fallback to fromAddr */
  displayName: string;
  /** Clean email address from the most recent email */
  fromAddr: string;
  /** Emails in this group, sorted newest-first */
  emails: Email[];
  /** ISO date string of the most recent email */
  latestTimestamp: string;
  /** Provider of the most recent email */
  provider: string;
}

export interface DomainGroup {
  /** Domain name, e.g. "gmail.com" */
  domain: string;
  /** Sender groups within this domain, sorted A-Z by displayName */
  senderGroups: SenderGroup[];
  /** Total number of emails across all senders in this domain */
  totalEmails: number;
}

export type VirtualItem =
  | { type: 'domain-header'; domain: DomainGroup }
  | { type: 'sender-header'; group: SenderGroup; domain: string }
  | { type: 'email'; email: Email; groupKey: string };

/**
 * Resolve the display name for a group of emails from the same sender.
 *
 * Strategy:
 * 1. Count occurrences of each non-empty fromName.
 * 2. Pick the most frequent name.
 * 3. On tie, pick the name that appears on the most recent email.
 * 4. If no fromName exists at all, try to extract name from fromAddr, then fall back to address.
 *
 * Assumes `emails` is already sorted by receivedAt descending.
 */
export function resolveDisplayName(emails: Email[]): string {
  if (emails.length === 0) return '';

  const counts = new Map<string, number>();
  const earliestIndex = new Map<string, number>();

  for (let i = 0; i < emails.length; i++) {
    const email = emails[i];
    if (!email) continue;
    const name = email.fromName?.trim();
    if (!name) continue;

    counts.set(name, (counts.get(name) ?? 0) + 1);
    if (!earliestIndex.has(name)) {
      earliestIndex.set(name, i);
    }
  }

  if (counts.size === 0) {
    const parsed = parseFromAddr(emails[0]!.fromAddr);
    return parsed.name ?? parsed.email;
  }

  let bestName = '';
  let bestCount = 0;
  let bestIndex = Infinity;

  for (const [name, count] of counts) {
    const idx = earliestIndex.get(name)!;
    if (count > bestCount || (count === bestCount && idx < bestIndex)) {
      bestName = name;
      bestCount = count;
      bestIndex = idx;
    }
  }

  return bestName;
}

/**
 * Group emails by normalized sender address, then by domain.
 *
 * Returns DomainGroup[] sorted A-Z by domain.
 * Within each domain, SenderGroup[] sorted A-Z by displayName.
 * Within each SenderGroup, emails sorted newest-first.
 */
export function groupByDomain(emails: Email[]): DomainGroup[] {
  if (emails.length === 0) return [];

  // 1. Build map: normalized sender address -> Email[]
  const senderMap = new Map<string, Email[]>();

  for (const email of emails) {
    const key = normalizeGroupKey(email.fromAddr);
    const list = senderMap.get(key);
    if (list) {
      list.push(email);
    } else {
      senderMap.set(key, [email]);
    }
  }

  // 2. Build SenderGroup[] and group by domain
  const domainMap = new Map<string, SenderGroup[]>();

  for (const [key, groupEmails] of senderMap) {
    groupEmails.sort((a, b) => new Date(b.receivedAt).getTime() - new Date(a.receivedAt).getTime());

    const mostRecent = groupEmails[0]!;
    const cleanEmail = parseFromAddr(mostRecent.fromAddr).email;
    const domain = extractDomain(key);

    const group: SenderGroup = {
      key,
      displayName: resolveDisplayName(groupEmails),
      fromAddr: cleanEmail,
      emails: groupEmails,
      latestTimestamp: mostRecent.receivedAt,
      provider: mostRecent.provider,
    };

    const domainList = domainMap.get(domain);
    if (domainList) {
      domainList.push(group);
    } else {
      domainMap.set(domain, [group]);
    }
  }

  // 3. Build DomainGroup[] sorted A-Z by domain
  const domains: DomainGroup[] = [];

  for (const [domain, senderGroups] of domainMap) {
    // Sort sender groups A-Z by displayName within each domain
    senderGroups.sort((a, b) =>
      a.displayName.localeCompare(b.displayName, undefined, { sensitivity: 'base' }),
    );

    domains.push({
      domain,
      senderGroups,
      totalEmails: senderGroups.reduce((sum, g) => sum + g.emails.length, 0),
    });
  }

  domains.sort((a, b) => a.domain.localeCompare(b.domain, undefined, { sensitivity: 'base' }));

  return domains;
}

/**
 * Flatten the 2-level domain→sender hierarchy into a flat VirtualItem[] for react-virtual.
 *
 * - `expandedDomains`: domains whose sender list is visible. Empty = all collapsed.
 * - `expandedSenders`: sender groups whose email list is visible. Empty = all collapsed.
 */
export function flattenGroups(
  domains: DomainGroup[],
  expandedDomains: Set<string>,
  expandedSenders: Set<string>,
): VirtualItem[] {
  const items: VirtualItem[] = [];

  for (const domainGroup of domains) {
    items.push({ type: 'domain-header', domain: domainGroup });

    if (expandedDomains.has(domainGroup.domain)) {
      for (const senderGroup of domainGroup.senderGroups) {
        items.push({ type: 'sender-header', group: senderGroup, domain: domainGroup.domain });

        if (expandedSenders.has(senderGroup.key)) {
          for (const email of senderGroup.emails) {
            items.push({ type: 'email', email, groupKey: senderGroup.key });
          }
        }
      }
    }
  }

  return items;
}
