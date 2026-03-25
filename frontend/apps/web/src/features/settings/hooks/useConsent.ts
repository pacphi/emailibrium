import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import {
  getConsents,
  recordConsent,
  requestDataExport,
  requestDataErase,
} from '@emailibrium/api';
import type {
  GdprConsent,
  ConsentRecord,
  DataExportRequest,
  DataExportResponse,
  DataEraseResponse,
} from '@emailibrium/types';

export function useConsents() {
  return useQuery<GdprConsent[]>({
    queryKey: ['consents'],
    queryFn: getConsents,
    staleTime: 60_000,
  });
}

export function useRecordConsent() {
  const queryClient = useQueryClient();
  return useMutation<GdprConsent, Error, ConsentRecord>({
    mutationFn: recordConsent,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['consents'] });
    },
  });
}

export function useDataExport() {
  return useMutation<DataExportResponse, Error, DataExportRequest>({
    mutationFn: requestDataExport,
  });
}

export function useDataErase() {
  return useMutation<DataEraseResponse, Error, { confirm: boolean }>({
    mutationFn: requestDataErase,
  });
}
