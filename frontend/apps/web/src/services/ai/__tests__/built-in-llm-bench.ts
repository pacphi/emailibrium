/* eslint-disable no-console */
/**
 * BL-4.03 — Built-in LLM performance benchmark suite.
 *
 * Standalone script — run with: npx tsx src/services/ai/__tests__/built-in-llm-bench.ts
 * Gracefully skips when no model is cached locally.
 */
import { getDefaultManifest, type ModelManifest } from '../model-manifest';
import { isModelCached, getModelPath } from '../model-cache';
import { BuiltInLlmAdapter } from '../built-in-llm-adapter';
import { detectHardware, type HardwareInfo } from '../hardware-detector';

interface BenchmarkResult {
  name: string;
  durationMs: number;
  tokensPerSecond?: number;
  memoryMb?: number;
  notes?: string;
}

const TARGETS: Record<string, string> = {
  'Cold start (model load)': '< 5s',
  Classification: '< 3s',
  'Chat (first token)': '< 0.5s',
  'Token counting (1000)': '-',
};

const rssMb = () => Math.round(process.memoryUsage().rss / 1024 / 1024);

const fmtDuration = (ms: number) =>
  ms >= 1000 ? `${(ms / 1000).toFixed(1)}s` : `${ms.toFixed(1)}ms`;

function printTable(results: BenchmarkResult[]): void {
  const [nW, dW, tW, gW] = [25, 10, 9, 8];
  const pad = (s: string, w: number) => s.padEnd(w);
  const sep = (w: number) => '-'.repeat(w);

  console.log(
    `| ${pad('Benchmark', nW)} | ${pad('Duration', dW)} | ${pad('tok/sec', tW)} | ${pad('Target', gW)} |`,
  );
  console.log(`|-${sep(nW)}-|-${sep(dW)}-|-${sep(tW)}-|-${sep(gW)}-|`);

  for (const r of results) {
    const tok = r.tokensPerSecond != null ? String(Math.round(r.tokensPerSecond)) : '-';
    console.log(
      `| ${pad(r.name, nW)} | ${pad(fmtDuration(r.durationMs), dW)} | ${pad(tok, tW)} | ${pad(TARGETS[r.name] ?? '-', gW)} |`,
    );
  }
}

async function benchColdStart(modelPath: string, manifest: ModelManifest) {
  const start = performance.now();
  const adapter = new BuiltInLlmAdapter({
    modelPath,
    contextSize: Math.min(manifest.contextLength, 2048),
  });
  await adapter.load();
  const durationMs = performance.now() - start;
  return { adapter, result: { name: 'Cold start (model load)', durationMs } as BenchmarkResult };
}

async function benchClassification(adapter: BuiltInLlmAdapter): Promise<BenchmarkResult> {
  const start = performance.now();
  const result = await adapter.classify({
    subject: 'Q3 Revenue Report',
    sender: 'cfo@company.com',
    bodyPreview:
      'Attached is the quarterly revenue report for Q3. Please review before the board meeting.',
    categories: ['finance', 'marketing', 'engineering', 'hr', 'legal'],
  });
  const durationMs = performance.now() - start;
  return {
    name: 'Classification',
    durationMs,
    notes: `category=${result.category} confidence=${result.confidence}`,
  };
}

async function benchChat(adapter: BuiltInLlmAdapter): Promise<BenchmarkResult> {
  let firstTokenMs: number | undefined;
  const start = performance.now();
  const gen = adapter.stream(
    [{ role: 'user', content: 'Summarize this email in one sentence: Meeting moved to 3pm.' }],
    {
      maxTokens: 64,
      onToken: () => {
        if (firstTokenMs == null) firstTokenMs = performance.now() - start;
      },
    },
  );
  let tokenCount = 0;
  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  for await (const _ of gen) {
    tokenCount++;
  }
  const totalMs = performance.now() - start;
  const tokPerSec = tokenCount > 0 ? (tokenCount / totalMs) * 1000 : undefined;
  return {
    name: 'Chat (first token)',
    durationMs: firstTokenMs ?? totalMs,
    tokensPerSecond: tokPerSec,
    notes: `total=${fmtDuration(totalMs)} tokens=${tokenCount}`,
  };
}

async function benchTokenCounting(adapter: BuiltInLlmAdapter): Promise<BenchmarkResult> {
  const text = 'The quick brown fox jumps over the lazy dog. '.repeat(25);
  const start = performance.now();
  const count = await adapter.countTokens(text);
  const durationMs = performance.now() - start;
  return { name: 'Token counting (1000)', durationMs, notes: `tokens=${count}` };
}

async function runBenchmarks(): Promise<BenchmarkResult[]> {
  const manifest = getDefaultManifest();
  if (!(await isModelCached(manifest))) {
    console.log('Skipping benchmarks -- no model cached');
    process.exit(0);
  }

  const modelPath = (await getModelPath(manifest))!;
  let hw: HardwareInfo;
  try {
    hw = await detectHardware();
  } catch {
    hw = { backends: ['cpu'], selected: 'cpu' };
  }

  console.log('\nBuilt-in LLM Performance Benchmarks');
  console.log('=====================================');
  console.log(`Model: ${manifest.modelId} (${manifest.quantization})`);
  console.log(`Hardware: ${hw.selected.toUpperCase()}${hw.gpuName ? ` (${hw.gpuName})` : ''}\n`);

  const memBefore = rssMb();
  const { adapter, result: coldStart } = await benchColdStart(modelPath, manifest);
  const memAfterLoad = rssMb();
  const classification = await benchClassification(adapter);
  const chat = await benchChat(adapter);
  const memDuringInference = rssMb();
  const tokenCounting = await benchTokenCounting(adapter);
  await adapter.unload();
  const memAfterUnload = rssMb();

  const results = [coldStart, classification, chat, tokenCounting];
  printTable(results);
  console.log(
    `\nMemory: ${memAfterLoad} MB loaded (${memBefore} MB before), ${memDuringInference} MB during inference, ${memAfterUnload} MB after unload\n`,
  );

  return results;
}

runBenchmarks().catch((err) => {
  console.error('Benchmark failed:', err);
  process.exit(1);
});
