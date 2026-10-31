#!/usr/bin/env node
// TEST DOUBLE of the Codex CLI for the G7 self-test (tools/herdr/test/g7-tests.mjs) only: the
// three commands the G7 scenario and client run. It is not Codex.
//
//   codex --version                      codex-cli <G7_FAKE_VERSION>
//   codex app-server daemon version      one JSON object, status running (G7_FAKE_DAEMON=stopped: not running)
//   codex app-server proxy               stdio <-> the fake daemon on 127.0.0.1:<G7_FAKE_PORT>, raw bytes
//
// Any other command exits 2. It never starts, stops or restarts anything.
import { connect } from 'node:net';

const args = process.argv.slice(2);
const version = process.env.G7_FAKE_VERSION ?? '9.9.9';
if (args[0] === '--version') {
  console.log(`codex-cli ${version}`);
} else if (args[0] === 'app-server' && args[1] === 'daemon' && args[2] === 'version') {
  if (process.env.G7_FAKE_DAEMON === 'stopped') console.log(JSON.stringify({ status: 'notRunning' }));
  else console.log(JSON.stringify({ status: 'running', cliVersion: version, appServerVersion: version, managedCodexVersion: version }));
} else if (args[0] === 'app-server' && args[1] === 'proxy') {
  const s = connect({ host: '127.0.0.1', port: Number(process.env.G7_FAKE_PORT) });
  process.stdin.pipe(s);
  s.pipe(process.stdout);
  s.on('close', () => process.exit(0));
  s.on('error', () => process.exit(1));
  process.stdin.on('end', () => s.end());
} else {
  console.error(`fake codex: not modelled: ${args.join(' ')}`);
  process.exit(2);
}
