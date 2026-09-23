#!/usr/bin/env node
// Load local configuration with Node itself:
// node --env-file-if-exists=.env.integration scripts/test-integration.mjs MODE
import { spawn } from 'node:child_process';
import { realpathSync } from 'node:fs';
import { dirname, isAbsolute, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const MODES = ['local', 'postgres', 'gmail', 'outlook', 'providers'];
const USAGE = 'Usage: node --env-file-if-exists=.env.integration scripts/test-integration.mjs MODE [--check-config]\nModes: local, postgres, gmail, outlook, providers';
const PG_KEY = 'EMAILIBRIUM_TEST_PG_URL';
const TENANT_KEY = 'EMAILIBRIUM_TEST_MICROSOFT_TENANT_ID';
export const PROVIDER_KEYS = Object.freeze({
  gmail: Object.freeze(['CLIENT_ID', 'CLIENT_SECRET', 'REFRESH_TOKEN', 'EXPECTED_EMAIL'].map(key => `EMAILIBRIUM_TEST_GOOGLE_${key}`)),
  outlook: Object.freeze(['CLIENT_ID', 'CLIENT_SECRET', 'REFRESH_TOKEN', 'EXPECTED_EMAIL'].map(key => `EMAILIBRIUM_TEST_MICROSOFT_${key}`)),
});
const TOOL_KEYS = [
  'PATH', 'Path', 'HOME', 'USER', 'LOGNAME', 'TMPDIR', 'TMP', 'TEMP',
  'SystemRoot', 'SYSTEMROOT', 'WINDIR', 'COMSPEC', 'PATHEXT', 'USERPROFILE', 'APPDATA', 'LOCALAPPDATA',
  'CARGO_HOME', 'RUSTUP_HOME', 'CARGO_TARGET_DIR', 'CARGO_BUILD_JOBS',
  'CARGO_NET_OFFLINE', 'CARGO_NET_GIT_FETCH_WITH_CLI',
  'SDKROOT', 'MACOSX_DEPLOYMENT_TARGET', 'CC', 'CXX', 'AR', 'PKG_CONFIG_PATH',
  'LIBRARY_PATH', 'LD_LIBRARY_PATH', 'CPATH', 'C_INCLUDE_PATH', 'CPLUS_INCLUDE_PATH',
  'OPENSSL_DIR', 'OPENSSL_LIB_DIR', 'OPENSSL_INCLUDE_DIR', 'OPENSSL_STATIC', 'LIBCLANG_PATH',
  'CMAKE_PREFIX_PATH', 'SSL_CERT_FILE', 'SSL_CERT_DIR', 'LANG', 'LC_ALL', 'TZ',
];

class IntegrationError extends Error {
  constructor(message, exitCode = 1) {
    super(message);
    this.exitCode = Number.isInteger(exitCode) && exitCode > 0 && exitCode < 256 ? exitCode : 1;
  }
}

function requireMode(mode) {
  if (!MODES.includes(mode)) throw new IntegrationError(USAGE);
}

export function parseArguments(argv) {
  const modes = argv.filter(value => value !== '--check-config');
  const checks = argv.filter(value => value === '--check-config').length;
  if (modes.length !== 1 || checks > 1) throw new IntegrationError(USAGE);
  requireMode(modes[0]);
  return { mode: modes[0], checkConfig: checks === 1 };
}

function requiredKeys(mode) {
  requireMode(mode);
  if (mode === 'providers') return [...PROVIDER_KEYS.gmail, ...PROVIDER_KEYS.outlook];
  if (mode === 'postgres') return [PG_KEY];
  return [...(PROVIDER_KEYS[mode] ?? [])];
}

export function validateConfiguration(mode, env) {
  const keys = requiredKeys(mode);
  const missing = keys.filter(key => typeof env[key] !== 'string' || env[key].trim() === '');
  if ((mode === 'outlook' || mode === 'providers') && env[TENANT_KEY] !== undefined &&
      (typeof env[TENANT_KEY] !== 'string' || env[TENANT_KEY].trim() === '')) missing.push(TENANT_KEY);
  if (missing.length) throw new IntegrationError(`Missing or empty integration configuration: ${missing.join(', ')}`);
  return keys;
}

export function childEnvironment(mode, env) {
  const allowed = [...TOOL_KEYS, ...requiredKeys(mode)];
  if (mode === 'outlook' || mode === 'providers') allowed.push(TENANT_KEY);
  const clean = Object.fromEntries(allowed.filter(key => typeof env[key] === 'string').map(key => [key, env[key]]));
  clean.CARGO_TERM_COLOR = 'never';
  clean.NO_COLOR = '1';
  return clean;
}

export function createPlan(mode, env, root = REPO_ROOT) {
  validateConfiguration(mode, env); // Validate all provider keys before the first provider can run.
  const cwd = join(root, 'backend');
  const step = (label, args, environment, kind = 'tests', exact = false) => ({
    command: 'cargo', args, cwd, env: childEnvironment(environment, env), label, kind, exact,
  });
  let steps;
  if (mode === 'local') {
    steps = [
      step('Local Rust tests', ['test', '--locked'], 'local'),
      step('Native HTTP backend build', ['build', '--locked', '--features', 'test-vectors', '--bin', 'emailibrium', '--message-format=json'], 'local', 'build'),
    ];
  } else if (mode === 'postgres') {
    const serial = ['--', '--nocapture', '--test-threads=1'];
    steps = [
      step('PostgreSQL library tests', ['test', '--locked', '--lib', 'postgres_', ...serial], mode),
      step('PostgreSQL binary tests', ['test', '--locked', '--bin', 'emailibrium', 'postgres_', ...serial], mode),
      step('PostgreSQL timestamp safety test', ['test', '--locked', '--bin', 'emailibrium', 'cleanup::orchestrator::expander::tests::safety_review_cutoff_handles_postgres_timestamp_formats', '--', '--ignored', '--exact', '--nocapture', '--test-threads=1'], mode, 'tests', true),
    ];
  } else {
    const providers = mode === 'providers' ? ['gmail', 'outlook'] : [mode];
    steps = providers.map(provider => step(`${provider} read-only credential test`, [
      'test', '--locked', '--test', 'live_provider_credentials', `${provider}_credentials_read_only`,
      '--', '--ignored', '--exact', '--nocapture', '--test-threads=1',
    ], provider, 'tests', true));
  }
  return { mode, root, steps };
}

export function testCounts(output) {
  const plain = output.replace(/\x1b\[[0-9;]*m/g, '').replace(/\r/g, '');
  const summaries = [...plain.matchAll(/^test result: (ok|FAILED)\.\s+(\d+) passed;\s+(\d+) failed;\s+(\d+) ignored;/gm)];
  if (!summaries.length) throw new IntegrationError('The test process did not report a test summary.');
  const counts = { passed: 0, failed: 0, ignored: 0 };
  for (const summary of summaries) {
    if (summary[1] !== 'ok') throw new IntegrationError('The test process reported failed tests.');
    counts.passed += Number(summary[2]);
    counts.failed += Number(summary[3]);
    counts.ignored += Number(summary[4]);
  }
  if (!Object.values(counts).every(Number.isSafeInteger) || counts.failed || !counts.passed) {
    throw new IntegrationError('The selected tests failed or zero tests actually passed.');
  }
  return counts;
}

export function nativeExecutable(output) {
  const executables = new Set();
  for (const line of output.split(/\r?\n/)) {
    let artifact;
    try { artifact = JSON.parse(line); } catch { continue; }
    if (artifact?.reason === 'compiler-artifact' && artifact.target?.name === 'emailibrium' &&
        artifact.target.kind?.includes('bin') && typeof artifact.executable === 'string') {
      if (!isAbsolute(artifact.executable)) throw new IntegrationError('Cargo reported a non-absolute native executable.');
      executables.add(artifact.executable);
    }
  }
  if (executables.size !== 1) throw new IntegrationError('Cargo did not identify exactly one native executable.');
  return [...executables][0];
}

// Raw output stays in memory. Provider tests can acquire new access tokens that
// are not in the input environment, so string redaction alone is not sufficient.
export function runCommand(command, args, {
  cwd, env, timeoutMs = 30 * 60_000, maxOutputBytes = 8 * 1024 * 1024, killGraceMs = 1000,
}) {
  return new Promise(resolveResult => {
    const stdout = [], stderr = [];
    const grouped = process.platform !== 'win32';
    let child, timer, escalation, finalizer;
    let size = 0, failure, forcedCode, settled = false;

    function terminate(signal) {
      if (!child?.pid) return;
      try {
        if (grouped) process.kill(-child.pid, signal);
        else child.kill(signal); // Windows: best effort for the direct process.
      } catch (error) {
        if (error.code !== 'ESRCH') { try { child.kill(signal); } catch { /* Bounded fallback below. */ } }
      }
    }
    function finish(code, signal) {
      if (settled) return;
      settled = true;
      // A parent can close its pipes before a TERM-resistant descendant exits.
      if (failure) terminate('SIGKILL');
      clearTimeout(timer); clearTimeout(escalation); clearTimeout(finalizer);
      process.off('SIGINT', interrupt); process.off('SIGTERM', terminateSignal);
      resolveResult({
        code: forcedCode ?? code ?? (signal === 'SIGINT' ? 130 : 1), failure,
        stdout: Buffer.concat(stdout).toString('utf8'), stderr: Buffer.concat(stderr).toString('utf8'),
      });
    }
    function stop(reason, code = 1, signal = 'SIGTERM') {
      if (settled || failure) return;
      failure = reason; forcedCode = code;
      terminate(signal);
      escalation = setTimeout(() => {
        terminate('SIGKILL');
        // Even if the OS cannot terminate a process, inherited pipes cannot
        // keep this runner hung forever. POSIX kills the complete process group;
        // Windows lacks this primitive and is explicitly best effort.
        finalizer = setTimeout(() => {
          child.stdout?.destroy(); child.stderr?.destroy(); child.unref(); finish(code);
        }, 1000);
      }, killGraceMs);
    }
    function interrupt() { stop('Process interrupted.', 130, 'SIGINT'); }
    function terminateSignal() { stop('Process interrupted.', 143, 'SIGTERM'); }

    try {
      child = spawn(command, args, {
        cwd, env, shell: false, detached: grouped, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'],
      });
    } catch {
      failure = 'Process could not start.'; finish(1); return;
    }
    process.on('SIGINT', interrupt); process.on('SIGTERM', terminateSignal);
    timer = setTimeout(() => stop('Process time limit exceeded.'), timeoutMs);
    const collect = chunks => chunk => {
      if (failure) return;
      size += chunk.length;
      if (size > maxOutputBytes) { stop('Process output limit exceeded.'); return; }
      chunks.push(chunk);
    };
    child.stdout.on('data', collect(stdout));
    child.stderr.on('data', collect(stderr));
    child.on('error', () => {
      failure = 'Process could not start.'; forcedCode = 1;
      child.stdout?.destroy(); child.stderr?.destroy(); child.unref(); finish(1);
    });
    child.on('close', finish);
  });
}

export async function runIntegration(mode, { env = process.env, root = REPO_ROOT, checkConfig = false, execute = runCommand, log = console.log } = {}) {
  const keys = validateConfiguration(mode, env);
  if (checkConfig) {
    log(`Configuration ready: ${mode}`);
    for (const key of keys) log(`${key}: present`);
    if ((mode === 'outlook' || mode === 'providers') && env[TENANT_KEY] !== undefined) log(`${TENANT_KEY}: present`);
    return [];
  }
  const plan = createPlan(mode, env, root);
  const results = [];
  let binary;
  async function run(step) {
    log(`${step.label}: running`);
    let result;
    try { result = await execute(step.command, step.args, { cwd: step.cwd, env: step.env }); }
    catch { throw new IntegrationError(`${step.label}: process could not start.`); }
    if (result.failure || result.code !== 0) {
      throw new IntegrationError(`${step.label}: failed (exit ${Number.isInteger(result.code) ? result.code : 1}). Raw child output is withheld to protect credentials.`, result.code);
    }
    return result;
  }
  for (const step of plan.steps) {
    const result = await run(step);
    if (step.kind === 'tests') {
      const counts = testCounts(`${result.stdout}\n${result.stderr}`);
      if (step.exact && counts.passed !== 1) throw new IntegrationError(`${step.label}: expected exactly one passed test.`);
      log(`${step.label}: ${counts.passed} passed; ${counts.ignored} ignored`);
      results.push({ label: step.label, ...counts });
    } else {
      binary = nativeExecutable(result.stdout);
      log(`${step.label}: built`);
    }
  }
  if (mode === 'local') {
    const step = {
      command: 'python3', args: [join(root, 'scripts/test-local-http.py'), '--binary', binary, '--output-dir', join(root, 'test-results/integration/local')],
      cwd: root, env: childEnvironment('local', env), label: 'Native HTTP smoke',
    };
    const result = await run(step);
    let smoke;
    try { smoke = JSON.parse(result.stdout.trim()); } catch { throw new IntegrationError('Native HTTP smoke did not report a valid result.'); }
    const checks = smoke?.checks && typeof smoke.checks === 'object' && !Array.isArray(smoke.checks) ? Object.values(smoke.checks) : [];
    if (smoke?.status !== 'passed' || !checks.length || checks.some(value => value !== true)) {
      throw new IntegrationError('Native HTTP smoke did not pass every reported check.');
    }
    log(`Native HTTP smoke: ${checks.length} checks passed`);
    results.push({ label: step.label, passed: checks.length });
  }
  log(`Integration checks passed: ${mode}`);
  return results;
}

let entrypoint = false;
try { entrypoint = !!process.argv[1] && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url); } catch { /* Imported by a non-file launcher. */ }
if (entrypoint) {
  try {
    const { mode, checkConfig } = parseArguments(process.argv.slice(2));
    await runIntegration(mode, { checkConfig });
  } catch (error) {
    console.error(error instanceof IntegrationError ? error.message : 'Integration runner failed. Raw diagnostics are withheld to protect credentials.');
    process.exitCode = error instanceof IntegrationError ? error.exitCode : 1;
  }
}
