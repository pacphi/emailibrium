import type { Email } from '@emailibrium/types';
import { flattenGroups, type DomainGroup } from './groupBySender';

/** Every email id in the current flat (ungrouped) view, for the select-all shortcut. */
export function selectAllEmailIds(emails: Email[]): Set<string> {
  return new Set(emails.map((email) => email.id));
}

/** Only the email rows actually VISIBLE in the grouped view: emails inside collapsed
 * domains or sender groups are excluded, so select-all can never silently check rows
 * the user cannot see on screen. Reuses the same flattening the grouped list renders
 * from, so "visible" here is by construction what GroupedEmailList shows. */
export function selectVisibleGroupedEmailIds(
  domains: DomainGroup[],
  expandedDomains: Set<string>,
  expandedSenders: Set<string>,
): Set<string> {
  const ids = new Set<string>();
  for (const item of flattenGroups(domains, expandedDomains, expandedSenders)) {
    if (item.type === 'email') {
      ids.add(item.email.id);
    }
  }
  return ids;
}
