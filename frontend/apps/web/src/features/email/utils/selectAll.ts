import type { Email } from '@emailibrium/types';

/** Every email id in the current filtered/grouped view, for the select-all shortcut. */
export function selectAllEmailIds(emails: Email[]): Set<string> {
  return new Set(emails.map((email) => email.id));
}
