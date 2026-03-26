// TODO: SHA-256 checksums need to be pinned from actual file downloads.
// Until verified, all checksums use placeholder values.

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

export const BUILTIN_LLM_MODELS: ModelManifest[] = [
  {
    modelId: 'qwen2.5-0.5b-q4km',
    displayName: 'Qwen 2.5 0.5B Instruct',
    repo: 'Qwen/Qwen2.5-0.5B-Instruct-GGUF',
    filename: 'qwen2.5-0.5b-instruct-q4_k_m.gguf',
    sizeBytes: 386_547_712,
    ramEstimateBytes: 524_288_000,
    quantization: 'Q4_K_M',
    contextLength: 32_768,
    description: 'Tiny but capable general-purpose model. Best balance of size and quality.',
    isDefault: true,
    sha256: 'sha256:pending-verification',
  },
  {
    modelId: 'smollm2-360m-q4km',
    displayName: 'SmolLM2 360M Instruct',
    repo: 'bartowski/SmolLM2-360M-Instruct-GGUF',
    filename: 'SmolLM2-360M-Instruct-Q4_K_M.gguf',
    sizeBytes: 271_581_184,
    ramEstimateBytes: 419_430_400,
    quantization: 'Q4_K_M',
    contextLength: 2_048,
    description: 'Ultra-light model for simple tasks. Fastest download and lowest memory.',
    isDefault: false,
    sha256: 'sha256:pending-verification',
  },
  {
    modelId: 'smollm2-1.7b-q4km',
    displayName: 'SmolLM2 1.7B Instruct',
    repo: 'HuggingFaceTB/SmolLM2-1.7B-Instruct-GGUF',
    filename: 'smollm2-1.7b-instruct-q4_k_m.gguf',
    sizeBytes: 1_106_247_680,
    ramEstimateBytes: 1_610_612_736,
    quantization: 'Q4_K_M',
    contextLength: 8_192,
    description: 'Mid-range model with good quality. Suitable for longer conversations.',
    isDefault: false,
    sha256: 'sha256:pending-verification',
  },
  {
    modelId: 'llama3.2-3b-q4km',
    displayName: 'Llama 3.2 3B Instruct',
    repo: 'bartowski/Llama-3.2-3B-Instruct-GGUF',
    filename: 'Llama-3.2-3B-Instruct-Q4_K_M.gguf',
    sizeBytes: 2_019_557_376,
    ramEstimateBytes: 2_684_354_560,
    quantization: 'Q4_K_M',
    contextLength: 131_072,
    description: 'Strong reasoning with massive context window. Needs 2.5 GB RAM.',
    isDefault: false,
    sha256: 'sha256:pending-verification',
  },
  {
    modelId: 'phi3.5-mini-q4km',
    displayName: 'Phi 3.5 Mini Instruct',
    repo: 'bartowski/Phi-3.5-mini-instruct-GGUF',
    filename: 'Phi-3.5-mini-instruct-Q4_K_M.gguf',
    sizeBytes: 2_394_046_464,
    ramEstimateBytes: 3_221_225_472,
    quantization: 'Q4_K_M',
    contextLength: 131_072,
    description: 'Highest quality for complex tasks. Largest download and RAM usage.',
    isDefault: false,
    sha256: 'sha256:pending-verification',
  },
];

export const DEFAULT_MODEL_ID = 'qwen2.5-0.5b-q4km';

/** Relative to the user home directory */
export const LLM_CACHE_DIR = '.emailibrium/models/llm';

export function getManifest(modelId: string): ModelManifest | undefined {
  return BUILTIN_LLM_MODELS.find((m) => m.modelId === modelId);
}

export function getDefaultManifest(): ModelManifest {
  const manifest = getManifest(DEFAULT_MODEL_ID);
  if (!manifest) throw new Error(`Default model ${DEFAULT_MODEL_ID} not found in manifest`);
  return manifest;
}

export function getAllManifests(): ModelManifest[] {
  return [...BUILTIN_LLM_MODELS];
}
