#!/usr/bin/env node
// TEST DOUBLE of the Beacon CLI (agent-beacon), used only by the herdr driver's self-test
// (tools/herdr/test/l3-tests.mjs) to exercise tools/herdr/scenarios/l3-beacon.mjs with no Beacon
// installed. It is not Beacon and proves nothing about Beacon: its output lines imitate the
// shapes the scenario parses (cli/beacon/cmd/version.go and endpoint_install.go at v1.3.29), and
// its "poll path" is invented.
//
// It answers EXACTLY the scenario's read-only allowlist and errors (exit 2) on anything else:
//   beacon version                        "beacon version <FAKE_BEACON_VERSION>"
//   beacon endpoint status --system       agent line, runtime-log path, service, Managed line
//   beacon endpoint claude sync --print   the fake runtime log's claude_code lines, as poll events
//   beacon endpoint codex sync --print    the fake runtime log's codex lines, as poll events
// Every invocation, allowed or not, is appended to $FAKE_BEACON_CALLS (JSONL: { argv, allowed })
// so the self-test can assert that nothing outside the allowlist was invoked. It never writes
// anything else, and reads only $FAKE_BEACON_LOG (a file in the self-test's temp dir).
//
// Environment:
//   FAKE_BEACON_CALLS     call log (required)
//   FAKE_BEACON_LOG       the fake runtime.jsonl the harness doubles append to
//   FAKE_BEACON_VERSION   default 1.3.29
//   FAKE_BEACON_MANAGED   the Managed line's value (default "not connected")

import { appendFileSync, existsSync, readFileSync } from 'node:fs';

const env = process.env;
const argv = process.argv.slice(2);
const ALLOWED = [['version'], ['endpoint', 'status', '--system'], ['endpoint', 'claude', 'sync', '--print'], ['endpoint', 'codex', 'sync', '--print']];
const allowed = ALLOWED.some((a) => a.length === argv.length && a.every((x, i) => x === argv[i]));
if (!env.FAKE_BEACON_CALLS) {
  console.error('fake-beacon: FAKE_BEACON_CALLS is not set');
  process.exit(2);
}
appendFileSync(env.FAKE_BEACON_CALLS, `${JSON.stringify({ argv, allowed })}\n`);
if (!allowed) {
  console.error(`fake-beacon: not on the read-only allowlist: ${argv.join(' ')}`);
  process.exit(2);
}

const version = env.FAKE_BEACON_VERSION || '1.3.29';
const logLines = () => (env.FAKE_BEACON_LOG && existsSync(env.FAKE_BEACON_LOG) ? readFileSync(env.FAKE_BEACON_LOG, 'utf8').split('\n').filter((l) => l.trim()) : []);

if (argv[0] === 'version') {
  console.log(`beacon version ${version}`);
} else if (argv[1] === 'status') {
  console.log(`Beacon Endpoint Agent ${version}`);
  console.log('Config: (fake)');
  console.log(`Runtime log: ${env.FAKE_BEACON_LOG || '(unset)'}`);
  console.log('Collector: grpc=true http=true');
  console.log('Service: loaded=true running=true');
  console.log('Last event: present');
  console.log(`Beacon Managed: ${env.FAKE_BEACON_MANAGED || 'not connected'}`);
} else {
  // Poll path: re-emit this harness's events with collection_method=poll, as JSON lines. Like
  // Beacon with no state file, older history comes FIRST: FAKE_BEACON_SYNC_HISTORY_BYTES of
  // invented old events precede the log's own. FAKE_BEACON_SYNC_SLEEP_MS stalls before any output
  // (a slow full-history sweep).
  const want = argv[1] === 'claude' ? 'claude_code' : 'codex';
  if (env.FAKE_BEACON_SYNC_SLEEP_MS) await new Promise((r) => setTimeout(r, Number(env.FAKE_BEACON_SYNC_SLEEP_MS)));
  const history = Number(env.FAKE_BEACON_SYNC_HISTORY_BYTES || 0);
  if (history > 0) {
    const line = `${JSON.stringify({ event: { action: 'prompt.submitted' }, harness: { name: want, collection_method: 'poll' }, session: { id: 'old-history' }, prompt: { text: 'an older session, long before the probe'.padEnd(400, '.') } })}\n`;
    const chunk = line.repeat(Math.max(1, Math.floor((1 << 20) / line.length)));
    for (let n = 0; n < history; n += chunk.length) {
      if (!process.stdout.write(chunk)) await new Promise((r) => process.stdout.once('drain', r));
    }
  }
  for (const l of logLines()) {
    let e;
    try {
      e = JSON.parse(l);
    } catch {
      continue;
    }
    if (e?.harness?.name !== want) continue;
    console.log(JSON.stringify({ ...e, harness: { ...e.harness, collection_method: 'poll' } }));
  }
}
