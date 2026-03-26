export type HardwareBackend = 'metal' | 'cuda' | 'vulkan' | 'cpu';

export interface HardwareInfo {
  backends: HardwareBackend[];
  selected: HardwareBackend;
  gpuName?: string;
  vramMb?: number;
}

const BACKEND_PRIORITY: HardwareBackend[] = ['metal', 'cuda', 'vulkan', 'cpu'];

let cachedInfo: HardwareInfo | null = null;

function selectBestBackend(backends: HardwareBackend[]): HardwareBackend {
  for (const backend of BACKEND_PRIORITY) {
    if (backends.includes(backend)) return backend;
  }
  return 'cpu';
}

function inferBackends(gpuNames: string[]): HardwareBackend[] {
  const backends: HardwareBackend[] = [];

  for (const name of gpuNames) {
    const lower = name.toLowerCase();
    if (
      lower.includes('apple') ||
      lower.includes('m1') ||
      lower.includes('m2') ||
      lower.includes('m3') ||
      lower.includes('m4')
    ) {
      if (!backends.includes('metal')) backends.push('metal');
    } else if (
      lower.includes('nvidia') ||
      lower.includes('geforce') ||
      lower.includes('rtx') ||
      lower.includes('gtx')
    ) {
      if (!backends.includes('cuda')) backends.push('cuda');
    } else if (lower.includes('amd') || lower.includes('radeon') || lower.includes('intel')) {
      if (!backends.includes('vulkan')) backends.push('vulkan');
    }
  }

  if (!backends.includes('cpu')) backends.push('cpu');
  return backends;
}

/**
 * Probes available compute backends via node-llama-cpp.
 * Results are cached for the lifetime of the process.
 * Falls back to CPU-only if node-llama-cpp is unavailable.
 */
export async function detectHardware(): Promise<HardwareInfo> {
  if (cachedInfo) return cachedInfo;

  try {
    const { getLlama } = await import('node-llama-cpp');
    const llama = await getLlama();

    const gpuNames = await llama.getGpuDeviceNames();
    const vramState = await llama.getVramState();

    const backends = inferBackends(gpuNames);
    const selected = selectBestBackend(backends);
    const gpuName = gpuNames.length > 0 ? gpuNames[0] : undefined;
    const vramMb = vramState.total > 0 ? Math.round(vramState.total / (1024 * 1024)) : undefined;

    cachedInfo = { backends, selected, gpuName, vramMb };
  } catch {
    cachedInfo = { backends: ['cpu'], selected: 'cpu' };
  }

  return cachedInfo;
}

/** Returns the cached hardware info, or null if detection has not yet run. */
export function getHardwareInfo(): HardwareInfo | null {
  return cachedInfo;
}

/** Clears the cached hardware info. Intended for testing. */
export function resetHardwareCache(): void {
  cachedInfo = null;
}
