export interface ClusterTerm {
  word: string;
  score: number;
  count: number;
}

export interface RepresentativeEmail {
  id: string;
  subject: string;
  fromAddr: string;
  fromName?: string;
}

export interface Cluster {
  id: string;
  name: string;
  description: string;
  emailCount: number;
  stabilityScore: number;
  isPinned: boolean;
  createdAt: string;
  topTerms: ClusterTerm[];
  representativeEmails: RepresentativeEmail[];
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
