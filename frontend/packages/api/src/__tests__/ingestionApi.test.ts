import { describe, it, expect, vi, beforeEach } from 'vitest';

// ---------------------------------------------------------------------------
// Mocks (vitest 4: hoist mock variables)
// ---------------------------------------------------------------------------

const { mockGet, mockPost, jsonFn, mockCreateSSEStream } = vi.hoisted(() => {
  const jsonFn = vi.fn().mockResolvedValue({});
  const responseLike = () => ({ json: jsonFn });

  return {
    mockGet: vi.fn().mockImplementation(() => responseLike()),
    mockPost: vi.fn().mockImplementation(() => responseLike()),
    jsonFn,
    mockCreateSSEStream: vi.fn().mockReturnValue({
      subscribe: vi.fn(),
      close: vi.fn(),
    }),
  };
});

vi.mock('../client.js', () => ({
  api: {
    get: mockGet,
    post: mockPost,
  },
}));

vi.mock('../sse.js', () => ({
  createSSEStream: mockCreateSSEStream,
}));

import {
  startIngestion,
  pauseIngestion,
  resumeIngestion,
  createIngestionStream,
  getEmbeddingStatus,
  PipelineBusyError,
} from '../ingestionApi.js';

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('ingestionApi', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    jsonFn.mockResolvedValue({});
  });

  describe('startIngestion', () => {
    it('calls POST ingestion/start with account_id, source, and 5-minute timeout', async () => {
      // startIngestion now uses throwHttpErrors: false and manually checks resp.ok
      mockPost.mockResolvedValueOnce({
        status: 200,
        ok: true,
        json: vi.fn().mockResolvedValue({ jobId: 'job1' }),
      });

      const result = await startIngestion('acc1');

      expect(mockPost).toHaveBeenCalledWith('ingestion/start', {
        json: { account_id: 'acc1', source: 'manual_sync' },
        timeout: 300_000,
        throwHttpErrors: false,
      });
      expect(result).toEqual({ jobId: 'job1' });
    });

    it('throws PipelineBusyError on 409 conflict', async () => {
      const busyBody = {
        error: 'pipeline_busy',
        message: 'A poll operation is already in progress',
        existingJobId: 'job-999',
        existingSource: 'poll',
        existingPhase: 'embedding',
        startedAt: '2026-04-03T10:00:00Z',
      };
      mockPost.mockResolvedValueOnce({
        status: 409,
        ok: false,
        json: vi.fn().mockResolvedValue(busyBody),
      });

      await expect(startIngestion('acc1', 'inbox_clean')).rejects.toThrow(PipelineBusyError);
    });

    it('throws generic error on non-409 failure', async () => {
      mockPost.mockResolvedValueOnce({
        status: 500,
        ok: false,
        statusText: 'Internal Server Error',
        text: vi.fn().mockResolvedValue('Something broke'),
      });

      await expect(startIngestion('acc1')).rejects.toThrow('Ingestion start failed (500)');
    });
  });

  describe('pauseIngestion', () => {
    it('calls POST ingestion/pause with job_id', async () => {
      await pauseIngestion('job1');

      expect(mockPost).toHaveBeenCalledWith('ingestion/pause', {
        json: { job_id: 'job1' },
      });
    });
  });

  describe('resumeIngestion', () => {
    it('calls POST ingestion/resume with job_id', async () => {
      await resumeIngestion('job1');

      expect(mockPost).toHaveBeenCalledWith('ingestion/resume', {
        json: { job_id: 'job1' },
      });
    });
  });

  describe('createIngestionStream', () => {
    it('calls createSSEStream with correct URL', () => {
      const stream = createIngestionStream('job1');

      expect(mockCreateSSEStream).toHaveBeenCalledWith('/api/v1/ingestion/job1/stream');
      expect(stream).toBeDefined();
      expect(stream.subscribe).toBeDefined();
      expect(stream.close).toBeDefined();
    });
  });

  describe('getEmbeddingStatus', () => {
    it('calls GET ingestion/embedding-status', async () => {
      const status = {
        totalEmails: 100,
        embeddingStatusSummary: {
          embeddedCount: 80,
          pendingCount: 15,
          failedCount: 5,
        },
      };
      jsonFn.mockResolvedValueOnce(status);

      const result = await getEmbeddingStatus();

      expect(mockGet).toHaveBeenCalledWith('ingestion/embedding-status');
      expect(result).toEqual(status);
    });
  });
});
