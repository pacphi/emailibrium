// Deterministic group-key helpers for risk acknowledgments.
//
// Medium-risk acknowledgments are tracked at the (account, source-type+id)
// level, so the user only has to acknowledge once per source group rather
// than per row. The key format is `${accountId}|${sourceTypeWithId}` and
// must be stable across renders.

import type { PlanSource, PlannedOperation } from '@emailibrium/types';

export function sourceKey(source: PlanSource): string {
  switch (source.type) {
    case 'subscription':
      return `subscription:${source.sender}`;
    case 'cluster':
      return `cluster:${source.clusterId}:${source.clusterAction}`;
    case 'rule':
      return `rule:${source.ruleId}`;
    case 'archiveStrategy':
      return `archiveStrategy:${source.strategy}`;
    case 'manual':
      return 'manual';
  }
}

export function sourceLabel(source: PlanSource): string {
  switch (source.type) {
    case 'subscription':
      return `Subscription · ${source.sender}`;
    case 'cluster':
      return `Cluster · ${source.clusterId}`;
    case 'rule':
      return `Rule · ${source.ruleId} (${source.matchBasis})`;
    case 'archiveStrategy':
      return `Archive strategy · ${source.strategy}`;
    case 'manual':
      return 'Manual selection';
  }
}

/** `${accountId}|${sourceKey}` — used as the Medium-ack identifier. */
export function mediumGroupKey(accountId: string, source: PlanSource): string {
  return `${accountId}|${sourceKey(source)}`;
}

export function opMediumGroupKey(op: PlannedOperation): string {
  return mediumGroupKey(op.accountId, op.source);
}
