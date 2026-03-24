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
