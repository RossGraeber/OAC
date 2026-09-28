#!/usr/bin/env node
// Runs INSIDE a herdr pane (typed there by `herdr pane run`) to record what environment the
// pane's processes get:
//
//   node env-probe.mjs <out.json> <nonce>
//
// Writes { names, values } to <out.json>: every variable NAME, and VALUES only for HERDR_*,
// TERM and COLORTERM. No other value is read, so a credential in the environment is never
// touched. Then prints `ENVPROBE-OK-<nonce>` as a whole line; the driver waits for that line
// with an anchored regex, which the echoed command line itself can never satisfy.

import { writeFileSync } from 'node:fs';

const [out, nonce] = process.argv.slice(2);
if (!out || !nonce) {
  console.error('usage: node env-probe.mjs <out.json> <nonce>');
  process.exit(2);
}
const names = Object.keys(process.env).sort();
const values = {};
for (const n of names) {
  if (/^HERDR_/i.test(n) || n === 'TERM' || n === 'COLORTERM') values[n] = process.env[n];
}
writeFileSync(out, JSON.stringify({ names, values }, null, 2));
console.log(`ENVPROBE-OK-${nonce}`);
