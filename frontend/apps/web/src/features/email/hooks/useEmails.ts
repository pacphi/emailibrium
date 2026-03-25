import { useQuery, useInfiniteQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import {
  getEmails,
  getEmail,
  getThread,
  archiveEmail,
  starEmail,
  deleteEmail,
  sendEmail,
  replyToEmail,
  forwardEmail,
  bulkArchive,
  bulkDelete,
  getLabels,
  moveEmail,
  markEmailRead,
} from '@emailibrium/api';
import type { GetEmailsParams, SendEmailDraft, FolderOrLabel } from '@emailibrium/api';
import type { Email, EmailThread } from '@emailibrium/types';

const PAGE_SIZE = 50;

export function useEmailsQuery(params?: GetEmailsParams) {
  return useInfiniteQuery({
    queryKey: ['emails', params],
    queryFn: ({ pageParam = 0 }) =>
      getEmails({ ...params, limit: PAGE_SIZE, offset: pageParam as number }),
    initialPageParam: 0,
    getNextPageParam: (lastPage, allPages) => {
      const loaded = allPages.reduce((sum, p) => sum + p.emails.length, 0);
      return loaded < lastPage.total ? loaded : undefined;
    },
    staleTime: 30_000,
  });
}

export function useEmailQuery(id: string | null) {
  return useQuery<Email>({
    queryKey: ['email', id],
    queryFn: () => getEmail(id!),
    enabled: !!id,
    staleTime: 60_000,
  });
}

export function useThreadQuery(threadId: string | null) {
  return useQuery<EmailThread>({
    queryKey: ['thread', threadId],
    queryFn: () => getThread(threadId!),
    enabled: !!threadId,
    staleTime: 60_000,
  });
}

export function useArchiveEmail() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: archiveEmail,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['emails'] });
    },
  });
}

export function useStarEmail() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: starEmail,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['emails'] });
    },
  });
}

export function useMarkRead() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ id, read }: { id: string; read: boolean }) => markEmailRead(id, read),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['emails'] });
      queryClient.invalidateQueries({ queryKey: ['thread'] });
    },
  });
}

export function useDeleteEmail() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: deleteEmail,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['emails'] });
    },
  });
}

export function useSendEmail() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (draft: SendEmailDraft) => sendEmail(draft),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['emails'] });
    },
  });
}

export function useReplyToEmail() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ id, body }: { id: string; body: { bodyText?: string; bodyHtml?: string } }) =>
      replyToEmail(id, body),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['emails'] });
      queryClient.invalidateQueries({ queryKey: ['thread'] });
    },
  });
}

export function useForwardEmail() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ id, to }: { id: string; to: string }) => forwardEmail(id, to),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['emails'] });
    },
  });
}

export function useBulkArchive() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: bulkArchive,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['emails'] });
    },
  });
}

export function useBulkDelete() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: bulkDelete,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['emails'] });
    },
  });
}

export function useLabelsQuery(accountId: string | undefined) {
  return useQuery<FolderOrLabel[]>({
    queryKey: ['labels', accountId],
    queryFn: () => getLabels(accountId!),
    enabled: !!accountId,
    staleTime: 300_000,
  });
}

export function useMoveEmail() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({
      id,
      ...body
    }: {
      id: string;
      accountId: string;
      targetId: string;
      kind: 'folder' | 'label';
    }) => moveEmail(id, body),
    onMutate: async ({ id }) => {
      await queryClient.cancelQueries({ queryKey: ['emails'] });
      const prev = queryClient.getQueryData<{ emails: Email[]; total: number }>(['emails']);
      if (prev) {
        queryClient.setQueryData(['emails'], {
          emails: prev.emails.filter((e) => e.id !== id),
          total: prev.total - 1,
        });
      }
      return { prev };
    },
    onError: (_err, _vars, context) => {
      if (context?.prev) {
        queryClient.setQueryData(['emails'], context.prev);
      }
    },
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: ['emails'] });
    },
  });
}
