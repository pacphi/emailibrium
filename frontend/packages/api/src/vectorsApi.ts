import type { HealthStatus, VectorStats } from '@emailibrium/types';
import { api } from './client.js';

export async function getHealth(): Promise<HealthStatus> {
  return api.get('vectors/health').json<HealthStatus>();
}

export async function getStats(): Promise<VectorStats> {
  return api.get('vectors/stats').json<VectorStats>();
}
