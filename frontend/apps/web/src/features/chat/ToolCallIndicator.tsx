import React from 'react';

interface ToolCallIndicatorProps {
  toolName: string;
  status: 'calling' | 'complete' | 'error';
}

/** Map tool names to user-friendly descriptions */
const TOOL_LABELS: Record<string, string> = {
  search_emails: 'Searching emails',
  get_email: 'Retrieving email',
  list_recent_emails: 'Loading recent emails',
  send_email: 'Sending email',
  reply_to_email: 'Sending reply',
  forward_email: 'Forwarding email',
  create_rule: 'Creating rule',
  classify_email: 'Classifying email',
  get_insights: 'Loading analytics',
  get_clusters: 'Loading clusters',
  list_accounts: 'Loading accounts',
  sync_account: 'Syncing account',
};

export function ToolCallIndicator({ toolName, status }: ToolCallIndicatorProps) {
  const label = TOOL_LABELS[toolName] || `Running ${toolName}`;

  return (
    <div className="flex items-center gap-2 text-sm text-gray-500 py-1">
      {status === 'calling' && <span className="animate-pulse">{'>'}</span>}
      {status === 'complete' && <span className="text-green-600">[done]</span>}
      {status === 'error' && <span className="text-red-600">[error]</span>}
      <span>{label}...</span>
    </div>
  );
}
