import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, writeFile, readFile, rm, access } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import {
  REPO_ROOT, PROVIDER_KEYS, parseArguments, validateConfiguration, childEnvironment,
  createPlan, testCounts, nativeExecutable, runCommand, runIntegration,
} from '../test-integration.mjs';

const root = resolve('/synthetic repository');
const pgKey = 'EMAILIBRIUM_TEST_PG_URL';
const tenantKey = 'EMAILIBRIUM_TEST_MICROSOFT_TENANT_ID';
const populated = {
  PATH: process.env.PATH, HOME: process.env.HOME, CARGO_TARGET_DIR: 'target with spaces', RUSTUP_TOOLCHAIN: '1.96.0',
  [pgKey]: 'postgres://synthetic:private-password@localhost/test',
  ...Object.fromEntries([...PROVIDER_KEYS.gmail, ...PROVIDER_KEYS.outlook].map(key => [key, `private-value-${key}`])),
  [tenantKey]: 'common', JWT_SECRET: 'private-jwt', GOOGLE_API_KEY: 'private-google-api',
  EMAILIBRIUM_DATABASE_URL: 'private-real-db', OPENAI_API_KEY: 'private-openai', NODE_OPTIONS: '--require unwanted.cjs',
};
const summary = (passed = 1, failed = 0, ignored = 0) => `\ntest result: ${failed ? 'FAILED' : 'ok'}. ${passed} passed; ${failed} failed; ${ignored} ignored; 0 measured; 0 filtered out; finished in 0.01s\n`;
const successful = () => ({ code: 0, stdout: summary(), stderr: '' });
const providerArgs = name => ['test', '--locked', '--test', 'live_provider_credentials', name, '--', '--ignored', '--exact', '--nocapture', '--test-threads=1'];

test('CLI requires one known mode and accepts check-config in either position', () => {
  for (const mode of ['local', 'postgres', 'gmail', 'outlook', 'providers']) {
    assert.deepEqual(parseArguments([mode]), { mode, checkConfig: false });
    assert.deepEqual(parseArguments([mode, '--check-config']), { mode, checkConfig: true });
    assert.deepEqual(parseArguments(['--check-config', mode]), { mode, checkConfig: true });
  }
  for (const args of [[], ['unknown-secret-value'], ['local', 'gmail'], ['local', '--unknown'], ['local', '--check-config', '--check-config']]) {
    assert.throws(() => parseArguments(args), /Usage:/);
  }
});

test('each selected mode requires its own keys, including both providers before any work', () => {
  assert.deepEqual(validateConfiguration('local', {}), []);
  for (const [mode, keys] of [['postgres', [pgKey]], ['gmail', PROVIDER_KEYS.gmail], ['outlook', PROVIDER_KEYS.outlook], ['providers', [...PROVIDER_KEYS.gmail, ...PROVIDER_KEYS.outlook]]]) {
    assert.deepEqual(validateConfiguration(mode, populated), keys);
    for (const key of keys) {
      assert.throws(() => validateConfiguration(mode, { ...populated, [key]: '  ' }), error => error.message.includes(key) && !error.message.includes('private-value'));
    }
  }
  assert.throws(() => validateConfiguration('outlook', { ...populated, [tenantKey]: '' }), /MICROSOFT_TENANT_ID/);
});

test('children receive required tool settings and only the selected test credentials', () => {
  const local = childEnvironment('local', populated);
  assert.equal(local.PATH, populated.PATH);
  assert.equal(local.CARGO_TARGET_DIR, populated.CARGO_TARGET_DIR);
  assert.equal(local.CARGO_TERM_COLOR, 'never');
  for (const key of Object.keys(populated).filter(key => !['PATH', 'HOME', 'CARGO_TARGET_DIR'].includes(key))) {
    assert.equal(local[key], undefined, `local must exclude ${key}`);
  }
  const gmail = childEnvironment('gmail', populated);
  for (const key of PROVIDER_KEYS.gmail) assert.equal(gmail[key], populated[key]);
  for (const key of [...PROVIDER_KEYS.outlook, pgKey, tenantKey]) assert.equal(gmail[key], undefined);
  const outlook = childEnvironment('outlook', populated);
  assert.equal(outlook[tenantKey], 'common');
  assert.equal(childEnvironment('postgres', populated)[pgKey], populated[pgKey]);
});

test('provider plans use exact ignored tests and separate child environments', () => {
  const plan = createPlan('providers', populated, root);
  assert.deepEqual(plan.steps.map(step => [step.command, step.args]), [
    ['cargo', providerArgs('gmail_credentials_read_only')],
    ['cargo', providerArgs('outlook_credentials_read_only')],
  ]);
  assert.equal(plan.steps[0].env[PROVIDER_KEYS.outlook[0]], undefined);
  assert.equal(plan.steps[1].env[PROVIDER_KEYS.gmail[0]], undefined);
  for (const step of plan.steps) assert.equal(step.cwd, join(root, 'backend'));
});

test('PostgreSQL runs library, binary and the exact ignored binary cutoff test', () => {
  const plan = createPlan('postgres', populated, root);
  assert.deepEqual(plan.steps.map(step => step.args), [
    ['test', '--locked', '--lib', 'postgres_', '--', '--nocapture', '--test-threads=1'],
    ['test', '--locked', '--bin', 'emailibrium', 'postgres_', '--', '--nocapture', '--test-threads=1'],
    ['test', '--locked', '--bin', 'emailibrium', 'cleanup::orchestrator::expander::tests::safety_review_cutoff_handles_postgres_timestamp_formats', '--', '--ignored', '--exact', '--nocapture', '--test-threads=1'],
  ]);
});

test('local plan runs normal locked tests and gets the native executable from Cargo build output', () => {
  const plan = createPlan('local', populated, root);
  assert.deepEqual(plan.steps.map(step => step.args), [
    ['test', '--locked'],
    ['build', '--locked', '--features', 'test-vectors', '--bin', 'emailibrium', '--message-format=json'],
  ]);
  assert.equal(plan.steps[0].env[pgKey], undefined);
  assert.ok(!plan.steps.some(step => step.args.includes('--ignored')));
  assert.equal(REPO_ROOT, resolve(import.meta.dirname, '../..'));
});

test('test summaries reject zero selected tests, failures and missing summaries', () => {
  assert.deepEqual(testCounts(summary(2) + summary(0, 0, 4)), { passed: 2, failed: 0, ignored: 4 });
  for (const output of [summary(0), summary(0, 0, 1), summary(1, 1), 'compilation succeeded']) {
    assert.throws(() => testCounts(output), /test|failed/i);
  }
});

test('Cargo executable selection honors arbitrary target paths without guessing', () => {
  const executable = join(root, 'custom target $(literal)', 'debug', 'emailibrium');
  const line = JSON.stringify({ reason: 'compiler-artifact', target: { name: 'emailibrium', kind: ['bin'] }, executable });
  assert.equal(nativeExecutable(`ignored\n${line}\n`), executable);
  assert.throws(() => nativeExecutable(''), /executable/i);
  assert.throws(() => nativeExecutable(JSON.stringify({ reason: 'compiler-artifact', target: { name: 'emailibrium', kind: ['bin'] }, executable: 'relative/unsafe' })), /executable/i);
});

test('check-config and missing configuration spawn no processes and never print values', async () => {
  const calls = [], logs = [];
  const execute = async (...args) => { calls.push(args); return successful(); };
  await runIntegration('gmail', { env: populated, root, checkConfig: true, execute, log: line => logs.push(line) });
  assert.equal(calls.length, 0);
  assert.ok(logs.join('\n').includes(PROVIDER_KEYS.gmail[0]));
  for (const value of Object.values(populated).filter(value => value?.startsWith('private'))) assert.ok(!logs.join('\n').includes(value));
  await assert.rejects(runIntegration('providers', { env: { ...populated, [PROVIDER_KEYS.outlook[0]]: '' }, root, execute }), /MICROSOFT_CLIENT_ID/);
  assert.equal(calls.length, 0);
});

test('nonzero results stop later steps and child output cannot leak credentials', async () => {
  const logs = [], calls = [];
  const execute = async (command, args, options) => {
    calls.push({ command, args, options });
    return { code: 9, stdout: `unknown-new-access-token ${populated[PROVIDER_KEYS.gmail[1]]}`, stderr: populated[pgKey] };
  };
  await assert.rejects(runIntegration('providers', { env: populated, root, execute, log: line => logs.push(line) }), /exit 9/);
  assert.equal(calls.length, 1);
  assert.ok(!logs.join('\n').includes('private-value'));
  assert.ok(!logs.join('\n').includes('unknown-new-access-token'));
});

test('a nominally successful process with zero selected tests fails the run', async () => {
  await assert.rejects(runIntegration('gmail', { env: populated, root, execute: async () => ({ code: 0, stdout: summary(0, 0, 1), stderr: '' }), log() {} }), /test/i);
});

test('local invokes only the existing native HTTP probe with the actual built artifact', async () => {
  const calls = [], logs = [];
  const executable = join(root, 'chosen cache', 'debug', 'emailibrium');
  const execute = async (command, args, options) => {
    calls.push({ command, args, options });
    if (args[0] === 'build') return { code: 0, stdout: JSON.stringify({ reason: 'compiler-artifact', target: { name: 'emailibrium', kind: ['bin'] }, executable }), stderr: '' };
    if (command === 'python3') return { code: 0, stdout: JSON.stringify({ status: 'passed', checks: { health: true, auth: true } }), stderr: '' };
    return successful();
  };
  await runIntegration('local', { env: populated, root, execute, log: line => logs.push(line) });
  assert.equal(calls.length, 3);
  assert.deepEqual(calls[2].args, [join(root, 'scripts/test-local-http.py'), '--binary', executable, '--output-dir', join(root, 'test-results/integration/local')]);
  assert.equal(calls[2].options.cwd, root);
  for (const call of calls) for (const key of [...PROVIDER_KEYS.gmail, ...PROVIDER_KEYS.outlook, pgKey]) assert.equal(call.options.env[key], undefined);
});

test('Node builtin env-file handling preserves environment precedence and literal values', async () => {
  const directory = await mkdtemp(join(tmpdir(), 'integration-env-test-'));
  try {
    const file = join(directory, '.env.integration');
    await writeFile(file, 'TEST_PRECEDENCE=file-value\nTEST_ONLY_FILE=file-only\nTEST_LITERAL="$(touch should-not-exist)"\n');
    const result = spawnSync(process.execPath, [`--env-file-if-exists=${file}`, '-e', 'process.stdout.write(JSON.stringify([process.env.TEST_PRECEDENCE, process.env.TEST_ONLY_FILE, process.env.TEST_LITERAL]))'], {
      cwd: directory, env: { PATH: process.env.PATH, TEST_PRECEDENCE: 'environment-value' }, encoding: 'utf8', shell: false,
    });
    assert.equal(result.status, 0);
    assert.deepEqual(JSON.parse(result.stdout), ['environment-value', 'file-only', '$(touch should-not-exist)']);
    await assert.rejects(access(join(directory, 'should-not-exist')));
  } finally { await rm(directory, { recursive: true, force: true }); }
});

test('the command executor passes shell metacharacters as an inert argument', async () => {
  const literal = '$(echo not-executed); literal argument';
  const result = await runCommand(process.execPath, ['-e', 'process.stdout.write(process.argv[1])', literal], { cwd: REPO_ROOT, env: childEnvironment('local', process.env) });
  assert.equal(result.code, 0);
  assert.equal(result.stdout, literal);
});


test('actual CLI preflight works without Cargo and unknown arguments never echo values', () => {
  const script = join(REPO_ROOT, 'scripts/test-integration.mjs');
  const env = { ...childEnvironment('local', process.env), ...Object.fromEntries(PROVIDER_KEYS.gmail.map(key => [key, populated[key]])), PATH: '' };
  const ready = spawnSync(process.execPath, [script, 'gmail', '--check-config'], { env, encoding: 'utf8', shell: false });
  assert.equal(ready.status, 0);
  assert.ok(ready.stdout.includes('CLIENT_SECRET: present'));
  assert.ok(!ready.stdout.includes('private-value'));
  const unknown = spawnSync(process.execPath, [script, 'sensitive-accidental-argument'], { env, encoding: 'utf8', shell: false });
  assert.notEqual(unknown.status, 0);
  assert.ok(!unknown.stderr.includes('sensitive-accidental-argument'));
});

test('selected exact tests and native smoke cannot pass with an invalid result', async () => {
  await assert.rejects(runIntegration('gmail', { env: populated, root, execute: async () => ({ code: 0, stdout: summary(2), stderr: '' }), log() {} }), /exactly one/);
  await assert.rejects(runIntegration('local', { env: populated, root, log() {}, execute: async (command, args) => {
    if (args[0] === 'build') return { code: 0, stdout: JSON.stringify({ reason: 'compiler-artifact', target: { name: 'emailibrium', kind: ['bin'] }, executable: join(root, 'bin') }), stderr: '' };
    if (command === 'python3') return { code: 0, stdout: JSON.stringify({ status: 'passed', checks: {} }), stderr: '' };
    return successful();
  } }), /smoke/);
});

test('POSIX output limits kill descendants which inherit the output pipes', { skip: process.platform === 'win32' }, async () => {
  const directory = await mkdtemp(join(tmpdir(), 'integration-process-tree-'));
  const pidFile = join(directory, 'grandchild.pid');
  const grandchild = `require('node:fs').writeFileSync(process.argv[1], String(process.pid)); process.on('SIGTERM', () => {}); process.send('ready'); setInterval(() => {}, 1000);`;
  const parent = `const { spawn } = require('node:child_process'); const child = spawn(process.execPath, ['-e', ${JSON.stringify(grandchild)}, process.argv[1]], { stdio: ['ignore', 'inherit', 'inherit', 'ipc'] }); child.on('message', () => process.stdout.write('x'.repeat(4096))); setInterval(() => {}, 1000);`;
  const pending = runCommand(process.execPath, ['-e', parent, pidFile], { cwd: directory, env: childEnvironment('local', process.env), timeoutMs: 10000, maxOutputBytes: 128, killGraceMs: 50 });
  let watchdog;
  let pid;
  try {
    const result = await Promise.race([pending, new Promise(resolve => { watchdog = setTimeout(() => resolve({ hung: true }), 2000); })]);
    pid = Number(await readFile(pidFile, 'utf8'));
    assert.ok(!result.hung, 'a descendant holding inherited pipes must not keep the runner hung');
    assert.match(result.failure, /output limit/);
    const deadline = Date.now() + 2000;
    let alive = true;
    while (alive && Date.now() < deadline) {
      try { process.kill(pid, 0); } catch (error) { if (error.code === 'ESRCH') alive = false; else throw error; }
      if (alive) await new Promise(resolve => setTimeout(resolve, 20));
    }
    assert.equal(alive, false, 'the inherited-pipe descendant must be terminated');
  } finally {
    clearTimeout(watchdog);
    if (!pid) { try { pid = Number(await readFile(pidFile, 'utf8')); } catch { /* Fixture did not start. */ } }
    if (Number.isInteger(pid) && pid > 0) { try { process.kill(pid, 'SIGKILL'); } catch (error) { if (error.code !== 'ESRCH') throw error; } }
    await pending;
    await rm(directory, { recursive: true, force: true });
  }
});

test('signal handlers forward cancellation and are removed when the child closes', async () => {
  const beforeInt = process.listeners('SIGINT');
  const beforeTerm = process.listeners('SIGTERM');
  const pending = runCommand(process.execPath, ['-e', 'setInterval(() => {}, 1000)'], { cwd: REPO_ROOT, env: childEnvironment('local', process.env), timeoutMs: 300, killGraceMs: 30 });
  try {
    assert.equal(process.listenerCount('SIGINT'), beforeInt.length + 1);
    assert.equal(process.listenerCount('SIGTERM'), beforeTerm.length + 1);
    process.listeners('SIGTERM').find(handler => !beforeTerm.includes(handler))();
    const result = await pending;
    assert.equal(result.code, 143);
    assert.match(result.failure, /interrupted/);
  } finally { await pending; }
  assert.deepEqual(process.listeners('SIGINT'), beforeInt);
  assert.deepEqual(process.listeners('SIGTERM'), beforeTerm);
});
