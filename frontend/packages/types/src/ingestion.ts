export type IngestionPhase =
  | 'syncing'
  | 'embedding'
  | 'categorizing'
  | 'clustering'
  | 'analyzing'
  | 'complete';

export interface IngestionProgress {
  jobId: string;
  total: number;
  processed: number;
  embedded: number;
  categorized: number;
  failed: number;
  phase: IngestionPhase;
  etaSeconds: number | null;
  emailsPerSecond: number;
}

/** Describes an active pipeline operation blocking a given account. */
export type PipelineSource = 'onboarding' | 'manual_sync' | 'inbox_clean' | 'poll' | 'unknown';

export interface PipelineActivity {
  jobId: string;
  accountId: string;
  phase: string;
  startedAt: string;
  source: PipelineSource;
}

/** 409 response body when a pipeline is already running for an account. */
export interface PipelineBusyResponse {
  error: 'pipeline_busy';
  message: string;
  existingJobId: string;
  existingSource: PipelineSource;
  existingPhase: string;
  startedAt: string;
}
