// Model manifest types — model data now comes from the backend API
// (config/models-llm.yaml), not from hardcoded arrays.

export interface ModelManifest {
  modelId: string;
  displayName: string;
  repo: string;
  filename: string;
  sizeBytes: number;
  ramEstimateBytes: number;
  quantization: string;
  contextLength: number;
  description: string;
  isDefault: boolean;
  sha256: string;
}

/**
 * Default model ID — kept as a fallback identifier. The actual model
 * catalog is served by the backend API (`GET /api/v1/ai/model-catalog`).
 */
export const DEFAULT_MODEL_ID = 'qwen3-1.7b-q4km';

/** Relative to the user home directory */
export const LLM_CACHE_DIR = '.emailibrium/models/llm';

/**
 * Returns `undefined` — model data is now served by the backend API
 * (`GET /api/v1/ai/model-catalog`). This function is kept for backward
 * compatibility with components that call it; callers should handle the
 * `undefined` return gracefully.
 */
export function getManifest(_modelId: string): ModelManifest | undefined {
  return undefined;
}

/**
 * @deprecated Model data is now served by the backend API.
 * Returns `undefined` — callers should handle this gracefully.
 */
export function getDefaultManifest(): ModelManifest | undefined {
  return undefined;
}

/**
 * Returns an empty array — model data is now served by the backend API.
 */
export function getAllManifests(): ModelManifest[] {
  return [];
}
