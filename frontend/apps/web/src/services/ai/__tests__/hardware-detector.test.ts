import { describe, it, expect, vi, beforeEach } from 'vitest';
import { detectHardware, getHardwareInfo, resetHardwareCache } from '../hardware-detector';

// ---------------------------------------------------------------------------
// Mock node-llama-cpp
// ---------------------------------------------------------------------------

const mockLlama = {
  getGpuDeviceNames: vi.fn(),
  getVramState: vi.fn(),
};

const mockGetLlama = vi.fn().mockResolvedValue(mockLlama);

vi.mock('node-llama-cpp', () => ({
  getLlama: mockGetLlama,
}));

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function setupGpu(names: string[], vram = { total: 16e9, used: 4e9, free: 12e9 }) {
  mockLlama.getGpuDeviceNames.mockResolvedValue(names);
  mockLlama.getVramState.mockResolvedValue(vram);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('HardwareDetector', () => {
  beforeEach(() => {
    resetHardwareCache();
    vi.clearAllMocks();
    // Re-setup getLlama mock after clearAllMocks wipes it
    mockGetLlama.mockResolvedValue(mockLlama);
    // Default: no GPU
    setupGpu([], { total: 0, used: 0, free: 0 });
  });

  // -------------------------------------------------------------------------
  // detectHardware
  // -------------------------------------------------------------------------

  describe('detectHardware', () => {
    it('returns Metal backend when macOS GPU detected', async () => {
      setupGpu(['Apple M2 Pro']);

      const info = await detectHardware();

      expect(info.selected).toBe('metal');
      expect(info.backends).toContain('metal');
    });

    it('returns CUDA backend when NVIDIA GPU detected', async () => {
      setupGpu(['NVIDIA GeForce RTX 4090']);

      const info = await detectHardware();

      expect(info.selected).toBe('cuda');
      expect(info.backends).toContain('cuda');
    });

    it('returns CPU-only when no GPU is available', async () => {
      setupGpu([]);

      const info = await detectHardware();

      expect(info.selected).toBe('cpu');
      expect(info.backends).toEqual(['cpu']);
    });

    it('caches result on subsequent calls (getLlama called only once)', async () => {
      setupGpu(['Apple M2 Pro']);

      const first = await detectHardware();
      const second = await detectHardware();

      expect(first).toBe(second);
      expect(mockGetLlama).toHaveBeenCalledTimes(1);
    });

    it('returns CPU fallback when node-llama-cpp throws', async () => {
      mockGetLlama.mockRejectedValueOnce(new Error('native binding missing'));

      const info = await detectHardware();

      expect(info.selected).toBe('cpu');
      expect(info.backends).toEqual(['cpu']);
    });
  });

  // -------------------------------------------------------------------------
  // getHardwareInfo
  // -------------------------------------------------------------------------

  describe('getHardwareInfo', () => {
    it('returns null before first detection', () => {
      expect(getHardwareInfo()).toBeNull();
    });

    it('returns cached info after detection', async () => {
      setupGpu(['Apple M2 Pro']);

      await detectHardware();

      const info = getHardwareInfo();
      expect(info).not.toBeNull();
      expect(info!.selected).toBe('metal');
    });
  });

  // -------------------------------------------------------------------------
  // resetHardwareCache
  // -------------------------------------------------------------------------

  describe('resetHardwareCache', () => {
    it('clears the cache', async () => {
      setupGpu(['Apple M2 Pro']);
      await detectHardware();

      expect(getHardwareInfo()).not.toBeNull();

      resetHardwareCache();

      expect(getHardwareInfo()).toBeNull();
    });
  });

  // -------------------------------------------------------------------------
  // GPU name and VRAM
  // -------------------------------------------------------------------------

  describe('GPU info population', () => {
    it('populates gpuName and vramMb when available', async () => {
      setupGpu(['Apple M2 Pro'], { total: 16 * 1024 * 1024 * 1024, used: 4e9, free: 12e9 });

      const info = await detectHardware();

      expect(info.gpuName).toBe('Apple M2 Pro');
      expect(info.vramMb).toBe(16384);
    });

    it('omits gpuName when no GPU names returned', async () => {
      setupGpu([]);

      const info = await detectHardware();

      expect(info.gpuName).toBeUndefined();
    });

    it('omits vramMb when total VRAM is zero', async () => {
      setupGpu(['Apple M2 Pro'], { total: 0, used: 0, free: 0 });

      const info = await detectHardware();

      expect(info.vramMb).toBeUndefined();
    });
  });
});
