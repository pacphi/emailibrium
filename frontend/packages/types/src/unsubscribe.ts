export interface UnsubscribeTarget {
  sender: string;
  listUnsubscribeHeader?: string;
  listUnsubscribePost?: string;
}

export interface UnsubscribeRequest {
  subscriptions: UnsubscribeTarget[];
}

export interface UnsubscribeResult {
  batchId: string;
  total: number;
  succeeded: number;
  failed: number;
  results: Array<{
    sender: string;
    methodUsed?: { type: string; url?: string; email?: string };
    success: boolean;
    error?: string;
  }>;
  undoAvailableUntil: string;
}

export interface UnsubscribePreview {
  sender: string;
  methods: Array<{ type: string; url?: string; email?: string }>;
  bestMethod?: { type: string; url?: string; email?: string };
  warning?: string;
}
