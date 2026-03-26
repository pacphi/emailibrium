#!/usr/bin/env npx tsx
/**
 * BL-2.05 — CLI script for managing GGUF models.
 *
 * Usage: npx tsx scripts/models.ts <command> [args]
 *
 * Commands:
 *   list                  Show available models with download status
 *   download <model-id>   Download a specific model
 *   download --default    Download the default model
 *   delete <model-id>     Delete a cached model
 *   info <model-id>       Show detailed model info
 */

import {
  getAllManifests,
  getManifest,
  getDefaultManifest,
  type ModelManifest,
} from '../frontend/apps/web/src/services/ai/model-manifest';
import {
  getCacheDir,
  isModelCached,
  deleteModel,
  getCacheSize,
} from '../frontend/apps/web/src/services/ai/model-cache';
import { downloadModel } from '../frontend/apps/web/src/services/ai/model-downloader';
import { detectHardware } from '../frontend/apps/web/src/services/ai/hardware-detector';

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const mb = bytes / (1024 * 1024);
  return mb < 1024 ? `${mb.toFixed(0)} MB` : `${(mb / 1024).toFixed(1)} GB`;
}

function printUsage(): void {
  console.log(
    'Usage: npx tsx scripts/models.ts <command> [args]\n\n' +
    'Commands:\n' +
    '  list                  Show available models with download status\n' +
    '  download <model-id>   Download a specific model\n' +
    '  download --default    Download the default model\n' +
    '  delete <model-id>     Delete a cached model\n' +
    '  info <model-id>       Show detailed model info',
  );
}

async function cmdList(): Promise<void> {
  const manifests = getAllManifests();

  const rows = await Promise.all(
    manifests.map(async (m) => {
      const cached = await isModelCached(m);
      return {
        'Model ID': m.modelId,
        Name: m.displayName,
        Size: formatBytes(m.sizeBytes),
        Status: cached ? 'cached' : 'not cached',
        Default: m.isDefault ? 'yes' : '',
      };
    }),
  );

  console.table(rows);

  const { totalBytes, models } = await getCacheSize();
  const cacheDir = await getCacheDir();
  console.log(
    `\nCache: ${cacheDir} (${models.length} model(s), ${formatBytes(totalBytes)} total)`,
  );
}

async function cmdDownload(modelIdOrFlag: string): Promise<void> {
  let manifest: ModelManifest | undefined;

  if (modelIdOrFlag === '--default') {
    manifest = getDefaultManifest();
  } else {
    manifest = getManifest(modelIdOrFlag);
  }

  if (!manifest) {
    console.error(`Error: unknown model "${modelIdOrFlag}"`);
    process.exit(1);
  }

  const cached = await isModelCached(manifest);
  if (cached) {
    console.log(`Model ${manifest.modelId} is already cached.`);
    return;
  }

  const total = manifest.sizeBytes;
  const totalMb = Math.round(total / (1024 * 1024));
  const barWidth = 20;

  const destDir = await getCacheDir();
  await downloadModel(manifest, destDir, {
    onProgress: ({ bytesDownloaded, percent }) => {
      const dlMb = Math.round(bytesDownloaded / (1024 * 1024));
      const filled = Math.round((percent / 100) * barWidth);
      const empty = barWidth - filled;
      const bar = '\u2588'.repeat(filled) + '\u2591'.repeat(empty);
      process.stdout.write(
        `\rDownloading ${manifest!.modelId} [${bar}] ${Math.round(percent)}% (${dlMb}/${totalMb} MB)`,
      );
    },
  });

  process.stdout.write('\n');
  console.log(`Download complete: ${manifest.modelId}`);
}

async function cmdDelete(modelId: string): Promise<void> {
  const manifest = getManifest(modelId);
  if (!manifest) {
    console.error(`Error: unknown model "${modelId}"`);
    process.exit(1);
  }

  const cached = await isModelCached(manifest);
  if (!cached) {
    console.log(`Model ${modelId} is not cached.`);
    return;
  }

  await deleteModel(modelId);
  console.log(`Deleted model ${modelId}.`);
}

async function cmdInfo(modelId: string): Promise<void> {
  const manifest = getManifest(modelId);
  if (!manifest) {
    console.error(`Error: unknown model "${modelId}"`);
    process.exit(1);
  }

  const cached = await isModelCached(manifest);

  let hwLine = 'run "npx tsx scripts/models.ts list" to detect';
  try {
    const hw = await detectHardware();
    hwLine = `${hw.selected} (available: ${hw.backends.join(', ')})`;
    if (hw.gpuName) hwLine += ` — ${hw.gpuName}`;
  } catch {
    // hardware detection may fail without node-llama-cpp
  }

  console.log(`
Model:          ${manifest.displayName}
ID:             ${manifest.modelId}
Repository:     ${manifest.repo}
Filename:       ${manifest.filename}
Size:           ${formatBytes(manifest.sizeBytes)}
RAM estimate:   ${formatBytes(manifest.ramEstimateBytes)}
Quantization:   ${manifest.quantization}
Context length: ${manifest.contextLength.toLocaleString()} tokens
Default:        ${manifest.isDefault ? 'yes' : 'no'}
Cached:         ${cached ? 'yes' : 'no'}
Hardware:       ${hwLine}
Description:    ${manifest.description}
`.trim());
}

async function main(): Promise<void> {
  const [command, arg] = process.argv.slice(2);
  if (!command) { printUsage(); process.exit(1); }

  switch (command) {
    case 'list': return cmdList();
    case 'download':
      if (!arg) { console.error('Error: specify a model ID or --default'); process.exit(1); }
      return cmdDownload(arg);
    case 'delete':
      if (!arg) { console.error('Error: specify a model ID'); process.exit(1); }
      return cmdDelete(arg);
    case 'info':
      if (!arg) { console.error('Error: specify a model ID'); process.exit(1); }
      return cmdInfo(arg);
    default:
      console.error(`Error: unknown command "${command}"`);
      printUsage();
      process.exit(1);
  }
}

main().catch((err: unknown) => {
  console.error(err instanceof Error ? err.message : String(err));
  process.exit(1);
});
