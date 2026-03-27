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
    it('calls POST ingestion/start with account_id and 5-minute timeout', async () => {
      jsonFn.mockResolvedValueOnce({ jobId: 'job1' });

      const result = await startIngestion('acc1');

      expect(mockPost).toHaveBeenCalledWith('ingestion/start', {
        json: { account_id: 'acc1' },
        timeout: 300_000,
      });
      expect(result).toEqual({ jobId: 'job1' });
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
