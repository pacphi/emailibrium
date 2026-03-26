import type { Cluster } from '@emailibrium/types';
import { api } from './client.js';

interface ClusterListResponse {
  clusters: Cluster[];
  total: number;
}

export async function getClusters(): Promise<Cluster[]> {
  const res = await api.get('clustering/clusters').json<ClusterListResponse>();
  return res.clusters;
}
