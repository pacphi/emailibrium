export type ConsentPurpose =
  | 'analytics'
  | 'ai_processing'
  | 'email_sync'
  | 'data_sharing'
  | 'marketing';

export interface GdprConsent {
  id: string;
  purpose: ConsentPurpose;
  granted: boolean;
  grantedAt: string;
  expiresAt?: string;
  ipAddress?: string;
}

export interface ConsentRecord {
  purpose: ConsentPurpose;
  granted: boolean;
}

export interface DataExportRequest {
  format: 'json' | 'csv';
  includeEmails: boolean;
  includeVectors: boolean;
  includeRules: boolean;
}

export interface DataExportResponse {
  exportId: string;
  status: 'pending' | 'processing' | 'ready' | 'expired';
  downloadUrl?: string;
  expiresAt?: string;
}

export interface DataEraseResponse {
  erasedAt: string;
  itemsErased: number;
}
