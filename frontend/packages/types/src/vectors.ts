export interface Cluster {
  id: string;
  name: string;
  description: string;
  emailCount: number;
  stabilityScore: number;
  isPinned: boolean;
  createdAt: string;
}

export interface VectorStats {
  totalVectors: number;
  collections: Record<string, number>;
  dimensions: number;
  memoryBytes: number;
  indexType: string;
}

export interface HealthStatus {
  status: string;
  storeHealthy: boolean;
  embeddingAvailable: boolean;
  storeStats: VectorStats;
}
