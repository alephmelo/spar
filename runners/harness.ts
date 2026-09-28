import assert from 'node:assert/strict';
type CheckResult = { name: string; status: string; stdout: string; stderr: string; detail: string };
const results: CheckResult[] = [];
let remaining = 8192;
const rawOut = process.stdout.write.bind(process.stdout);
const rawErr = process.stderr.write.bind(process.stderr);
function bounded(text: string): string {
  text = text.toWellFormed();
  let kept = '';
  let count = 0;
  const limit = Math.min(remaining, 4096);
  for (const character of text) {
    if (count >= limit) break;
    kept += character;
    count++;
  }
  remaining -= count;
  return kept + (kept.length < text.length ? '\n[output truncated]' : '');
}
function capture(result: CheckResult) {
  const writer = (stream: 'stdout' | 'stderr') => (chunk: any, encoding?: any, callback?: any): boolean => {
    if (!result[stream].endsWith('[output truncated]')) {
      result[stream] += bounded(typeof chunk === 'string' ? chunk : Buffer.from(chunk).toString('utf8'));
    }
    const done = typeof encoding === 'function' ? encoding : callback;
    if (typeof done === 'function') done();
    return true;
  };
  process.stdout.write = writer('stdout') as typeof process.stdout.write;
  process.stderr.write = writer('stderr') as typeof process.stderr.write;
}
function restore() { process.stdout.write = rawOut; process.stderr.write = rawErr; }
function detail(error: unknown): string {
  return bounded(error instanceof Error ? (error.stack || error.message) : String(error));
}
const startup: CheckResult = { name: 'Module load', status: 'passed', stdout: '', stderr: '', detail: '' };
let solve: any;
let loadFailed = false;
capture(startup);
try { ({ solve } = await import('./solution.ts')); }
catch (error) { loadFailed = true; startup.detail = detail(error); }
finally { restore(); }
async function check(name: string, body: () => Promise<void>) {
  const result: CheckResult = { name, status: 'passed', stdout: '', stderr: '', detail: '' };
  if (results.length === 0) { result.stdout = startup.stdout; result.stderr = startup.stderr; }
  capture(result);
  try {
    if (loadFailed) {
      result.status = 'error';
      result.detail = results.length === 0 ? startup.detail : 'Module could not load';
    } else { await body(); }
  } catch (error) {
    result.status = error instanceof assert.AssertionError ? 'failed' : 'error';
    result.detail = detail(error);
  } finally { restore(); }
  results.push(result);
}
