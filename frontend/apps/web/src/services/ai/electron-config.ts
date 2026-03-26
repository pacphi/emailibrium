/**
 * BL-4.01 — Electron packaging configuration for node-llama-cpp.
 *
 * Utilities and constants for bundling native LLM bindings in Electron.
 */

import { join } from 'path';

/** Environment variables needed for Electron packaging with Metal support on macOS. */
export const ELECTRON_BUILD_ENV = {
  NODE_LLAMA_CPP_CMAKE_OPTION_LLAMA_METAL_EMBED_LIBRARY: '1',
} as const;

/** Files/dirs that must be unpacked from ASAR for native module support. */
export const ASAR_UNPACK_PATTERNS = ['node_modules/node-llama-cpp/**'] as const;

/** Check if running inside a packaged Electron app. */
export function isPackagedElectron(): boolean {
  try {
    const app = require('electron')?.app; // eslint-disable-line @typescript-eslint/no-require-imports
    return app?.isPackaged === true;
  } catch {
    return false;
  }
}

/** Returns platform-appropriate model cache directory inside the app bundle or user data. */
export function getElectronModelDir(): string {
  try {
    const app = require('electron')?.app; // eslint-disable-line @typescript-eslint/no-require-imports
    if (app) {
      return join(app.getPath('userData'), 'models', 'llm');
    }
  } catch {
    // Not running in Electron — fall through.
  }
  return join(process.env.HOME ?? process.env.USERPROFILE ?? '.', '.emailibrium', 'models', 'llm');
}
