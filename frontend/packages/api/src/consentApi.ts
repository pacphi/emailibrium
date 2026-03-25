import type {
  GdprConsent,
  ConsentRecord,
  DataExportRequest,
  DataExportResponse,
  DataEraseResponse,
} from '@emailibrium/types';
import { api } from './client.js';

export async function recordConsent(consent: ConsentRecord): Promise<GdprConsent> {
  return api.post('consent/gdpr', { json: consent }).json<GdprConsent>();
}

export async function getConsents(): Promise<GdprConsent[]> {
  const resp = await api.get('consent/gdpr').json<{ decisions: GdprConsent[] }>();
  return resp.decisions ?? [];
}

export async function requestDataExport(request: DataExportRequest): Promise<DataExportResponse> {
  return api.post('consent/export', { json: request }).json<DataExportResponse>();
}

export async function requestDataErase(confirmation: {
  confirm: boolean;
}): Promise<DataEraseResponse> {
  return api.post('consent/erase', { json: confirmation }).json<DataEraseResponse>();
}
