import type {
  PlanAction,
  PlanId,
  PlannedOperation,
  CleanupFolderOrLabel,
  RiskLevel,
} from '@emailibrium/types';
import { SampleEmailPeek } from './SampleEmailPeek';

export interface PlanDiffRowProps {
  op: PlannedOperation;
  planId: PlanId;
  userId: string;
  ackState: { isAcked: boolean; onToggle(): void } | null;
}

const riskPillClasses: Record<RiskLevel, string> = {
  low: 'bg-green-100 text-green-800 dark:bg-green-900/40 dark:text-green-200',
  medium: 'bg-amber-100 text-amber-800 dark:bg-amber-900/40 dark:text-amber-200',
  high: 'bg-red-100 text-red-800 dark:bg-red-900/40 dark:text-red-200',
};

const riskPillLabels: Record<RiskLevel, string> = {
  low: 'Low',
  medium: 'Medium',
  high: 'High',
};

function actionChipText(action: PlanAction): string {
  switch (action.type) {
    case 'archive':
      return 'Archive';
    case 'addLabel':
      return action.kind === 'label' ? 'Add label' : 'Add folder';
    case 'move':
      return action.kind === 'label' ? 'Move (label)' : 'Move (folder)';
    case 'delete':
      return action.permanent ? 'Delete permanently' : 'Delete (soft)';
    case 'unsubscribe':
      return `Unsubscribe (${action.method})`;
    case 'markRead':
      return 'Mark read';
    case 'star':
      return action.on ? 'Star' : 'Unstar';
  }
}

function explainOperation(
  action: PlanAction,
  target: CleanupFolderOrLabel | null,
  count: number | null,
): string {
  const noun =
    count !== null ? `${count.toLocaleString()} email${count === 1 ? '' : 's'}` : '1 email';
  switch (action.type) {
    case 'archive':
      return `Archive ${noun} (recoverable from All Mail).`;
    case 'addLabel':
      return target ? `Add the "${target.name}" label to ${noun}.` : `Add a label to ${noun}.`;
    case 'move':
      return target ? `Move ${noun} to "${target.name}".` : `Move ${noun}.`;
    case 'delete':
      return action.permanent
        ? `Permanently delete ${noun} — NOT recoverable.`
        : `Move ${noun} to Trash — recoverable for 30 days.`;
    case 'unsubscribe':
      return `Send unsubscribe via ${action.method} for ${noun}.`;
    case 'markRead':
      return `Mark ${noun} as read.`;
    case 'star':
      return action.on ? `Star ${noun}.` : `Unstar ${noun}.`;
  }
}

export function PlanDiffRow({ op, planId, userId, ackState }: PlanDiffRowProps) {
  const isPredicate = op.opKind === 'predicate';
  const count = isPredicate ? op.projectedCount : 1;
  const target = op.target;
  const actionText = actionChipText(op.action);
  const explainer = explainOperation(op.action, target, count);

  return (
    <li
      role="listitem"
      className="flex items-start gap-3 px-3 py-2 border-b border-gray-100 dark:border-gray-700 last:border-b-0"
    >
      {ackState && (
        <input
          type="checkbox"
          checked={ackState.isAcked}
          onChange={ackState.onToggle}
          className="mt-1 h-4 w-4 rounded border-gray-300 text-red-600 focus:ring-red-500"
          aria-label={`Acknowledge high-risk row #${op.seq}`}
        />
      )}

      <span className="font-mono text-[10px] text-gray-400 mt-1 shrink-0 w-12">#{op.seq}</span>

      <span className="inline-flex items-center rounded-md bg-gray-100 dark:bg-gray-700 px-2 py-0.5 text-xs font-medium text-gray-700 dark:text-gray-200 shrink-0">
        {actionText}
      </span>

      <div className="flex-1 min-w-0">
        <p className="text-sm text-gray-800 dark:text-gray-200">{explainer}</p>
        {!isPredicate && op.emailId && (
          <p className="mt-0.5 font-mono text-[11px] text-gray-500 dark:text-gray-400 truncate">
            {op.emailId}
          </p>
        )}
        {isPredicate && (
          <div className="mt-1">
            <SampleEmailPeek planId={planId} userId={userId} source={op.source} />
          </div>
        )}
      </div>

      <span
        className={`inline-flex items-center rounded-full px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wide shrink-0 ${riskPillClasses[op.risk]}`}
        aria-label={`Risk: ${riskPillLabels[op.risk]}`}
      >
        {riskPillLabels[op.risk]}
      </span>
    </li>
  );
}
