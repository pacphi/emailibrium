import type { Cluster } from '@emailibrium/types';
import { api } from './client.js';

interface ClusterListResponse {
  clusters: Cluster[];
  total: number;
}

export interface ClusteringStatus {
  clusterCount: number;
  totalClusteredEmails: number;
  isClustering: boolean;
  isIngesting: boolean;
  phase: string | null;
}

export async function getClusters(): Promise<Cluster[]> {
  const res = await api.get('clustering/clusters').json<ClusterListResponse>();
  return res.clusters;
}

export async function getClusteringStatus(): Promise<ClusteringStatus> {
  return api.get('clustering/status').json<ClusteringStatus>();
}

export async function triggerRecluster(timeoutMs?: number): Promise<void> {
  await api.post('clustering/recluster', {
    timeout: timeoutMs ?? 300_000,
  });
}
