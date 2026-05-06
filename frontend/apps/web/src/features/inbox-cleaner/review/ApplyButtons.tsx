import { useMemo } from 'react';
import type { PlanId, PlannedOperation, RiskMax } from '@emailibrium/types';
import { opMediumGroupKey } from './groupKey';

export interface ApplyButtonsProps {
  planId: PlanId;
  userId: string;
  rows: PlannedOperation[];
  ackedHighSeqs: number[];
  ackedMediumGroupKeys: string[];
  onApply(riskMax: RiskMax): void;
}

interface ButtonState {
  enabled: boolean;
  reason?: string;
}

export function ApplyButtons({
  planId,
  userId,
  rows,
  ackedHighSeqs,
  ackedMediumGroupKeys,
  onApply,
}: ApplyButtonsProps) {
  const ackedHighSet = useMemo(() => new Set(ackedHighSeqs), [ackedHighSeqs]);
  const ackedMediumSet = useMemo(() => new Set(ackedMediumGroupKeys), [ackedMediumGroupKeys]);

  const { lowState, mediumState, highState, lowCount, mediumCount, highCount, totalMediumGroups } =
    useMemo(() => {
      let low = 0;
      let medium = 0;
      let high = 0;
      const allMediumGroups = new Set<string>();
      const unackedHigh: number[] = [];
      const unackedMediumGroups: string[] = [];

      for (const op of rows) {
        if (op.risk === 'low') low += 1;
        else if (op.risk === 'medium') {
          medium += 1;
          const key = opMediumGroupKey(op);
          allMediumGroups.add(key);
          if (!ackedMediumSet.has(key)) unackedMediumGroups.push(key);
        } else if (op.risk === 'high') {
          high += 1;
          if (!ackedHighSet.has(op.seq)) unackedHigh.push(op.seq);
          // High rows also need their medium-group acked (the group key is
          // computed identically); track here too.
          const key = opMediumGroupKey(op);
          allMediumGroups.add(key);
          if (!ackedMediumSet.has(key)) unackedMediumGroups.push(key);
        }
      }

      const lowS: ButtonState = {
        enabled: low > 0,
        reason: low === 0 ? 'No low-risk operations in plan' : undefined,
      };

      const mediumS: ButtonState = {
        enabled: medium > 0 && unackedMediumGroups.length === 0,
        reason:
          medium === 0
            ? 'No medium-risk operations in plan'
            : unackedMediumGroups.length > 0
              ? `${unackedMediumGroups.length} medium group${
                  unackedMediumGroups.length === 1 ? '' : 's'
                } still need acknowledgment`
              : undefined,
      };

      const highS: ButtonState = {
        enabled: unackedHigh.length === 0 && unackedMediumGroups.length === 0,
        reason:
          unackedHigh.length > 0
            ? `${unackedHigh.length} high-risk row${
                unackedHigh.length === 1 ? '' : 's'
              } still need acknowledgment`
            : unackedMediumGroups.length > 0
              ? `${unackedMediumGroups.length} medium group${
                  unackedMediumGroups.length === 1 ? '' : 's'
                } still need acknowledgment`
              : undefined,
      };

      return {
        lowState: lowS,
        mediumState: mediumS,
        highState: highS,
        lowCount: low,
        mediumCount: medium,
        highCount: high,
        totalMediumGroups: allMediumGroups.size,
      };
    }, [rows, ackedHighSet, ackedMediumSet]);

  // Phase C: parent (CleanupReview) holds the useCleanupApply hook and
  // turns this callback into a real POST /apply. planId/userId are kept on
  // the props for future telemetry / logging hooks.
  void planId;
  void userId;
  const handleApply = (riskMax: RiskMax) => {
    onApply(riskMax);
  };

  const renderButton = (label: string, riskMax: RiskMax, state: ButtonState, classes: string) => (
    <button
      type="button"
      onClick={() => state.enabled && handleApply(riskMax)}
      disabled={!state.enabled}
      aria-disabled={!state.enabled}
      title={state.reason}
      className={`px-4 py-2 text-sm font-medium rounded-md transition-colors ${classes} ${
        state.enabled ? '' : 'opacity-50 cursor-not-allowed'
      }`}
    >
      {label}
    </button>
  );

  return (
    <section aria-labelledby="apply-heading" className="space-y-3">
      <h2 id="apply-heading" className="text-sm font-semibold text-gray-900 dark:text-gray-100">
        Apply plan
      </h2>
      <p className="text-xs text-gray-500 dark:text-gray-400">
        Acknowledged: {ackedMediumSet.size}/{totalMediumGroups} medium groups, {ackedHighSet.size}/
        {highCount} high-risk rows
      </p>
      <div className="flex flex-wrap gap-3">
        {renderButton(
          `Apply Low only (${lowCount.toLocaleString()})`,
          'low',
          lowState,
          'bg-green-600 text-white hover:bg-green-700',
        )}
        {renderButton(
          `Apply Low + Medium (${(lowCount + mediumCount).toLocaleString()})`,
          'medium',
          mediumState,
          'bg-amber-600 text-white hover:bg-amber-700',
        )}
        {renderButton(
          `Apply all incl. High (${(lowCount + mediumCount + highCount).toLocaleString()})`,
          'high',
          highState,
          'bg-red-600 text-white hover:bg-red-700',
        )}
      </div>
    </section>
  );
}
