export interface UnsubscribeRequest {
  subscriptionIds: string[];
}

export interface UnsubscribeResult {
  batchId: string;
  succeeded: string[];
  failed: Array<{ subscriptionId: string; reason: string }>;
  undoDeadline: string;
}

export interface UnsubscribePreview {
  subscriptionId: string;
  senderAddress: string;
  senderDomain: string;
  emailCount: number;
  method: 'link' | 'email' | 'header';
  estimatedEffect: string;
}
