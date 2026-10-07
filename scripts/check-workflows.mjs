#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0
//
// CI workflow policy (#61, F12): the default tier runs with no live provider, no API key
// and no network beyond loopback (PLANNING-PROMPT section 6 and 9.10; oac-testing
// section 2), and every workflow keeps the repository's hardening.
//
//   node scripts/check-workflows.mjs             # check .github/workflows/*.yml|yaml
//   node scripts/check-workflows.mjs --self-test # planted violations must fail
//
// Every workflow:
//   W1  every `uses:` names an action at a full 40-hex commit SHA (no tag, no branch, no
//       docker:// image);
//   W2  a top-level `permissions:` block that is exactly `contents: read`, and no job-level
//       permission above `read`;
//   W3  every actions/checkout step sets `persist-credentials: false`;
//   W4  no secrets context (`secrets.X`, `secrets[...]`, `secrets: inherit`), no
//       `github.token`, no provider or harness credential variable, and no
//       `pull_request_target` or `workflow_run` trigger.
// Default-tier workflows (any trigger other than workflow_dispatch; the herdr opt-in
// workflow is the exception, with its own stricter rules in
// scripts/check-herdr-containment.mjs check 9):
//   D1  no self-hosted runner;
//   D2  no opt-in switch: no `--ignored` / `--include-ignored` test run, no OAC_TEST_*
//       opt-in variable, no tools/herdr driver;
//   D3  no harness CLI install (the Claude Code or Codex npm packages, `codex`/`claude`
//       installers).
// Opt-in workflows (workflow_dispatch only) may use D1-D3; W1-W4 still hold.
//
// The reader is line-based and fails closed: a workflow whose `on:` or top-level
// `permissions:` it cannot read is a violation. Comments are stripped before W4 and D1-D3
// match, so a header saying "no secrets" is not a hit. Node built-ins only.
// Exit codes: 0 = clean; 1 = violation (or failed self-test); 2 = usage or environment error.

import { readdirSync, readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const scriptPath = fileURLToPath(import.meta.url);
const repoRoot = resolve(dirname(scriptPath), '..');
const WORKFLOWS = join(repoRoot, '.github', 'workflows');

// Governed by check 9 (scripts/check-herdr-containment.mjs): push-to-main on PINS.md and
// dispatch, self-hosted harness runners, no opt-in flag beyond its scenario input.
const HERDR_OPTIN = 'herdr-provider-optin.yml';

const SHA_PIN = /^[\w.-]+\/[\w.-]+(?:\/[\w./-]+)?@[0-9a-f]{40}$/;
const CREDENTIAL = /ANTHROPIC_API_KEY|ANTHROPIC_AUTH_TOKEN|CLAUDE_CODE_OAUTH_TOKEN|OPENAI_API_KEY|CODEX_API_KEY|OPENAI_ORG|\bauth\.json\b|\.credentials\.json/i;
const SECRETS = /\bsecrets\s*[.[]|\bsecrets\s*:\s*inherit|toJSON\(\s*secrets\s*\)|\bgithub\.token\b/i;
const PRIVILEGED_TRIGGER = /^(?:pull_request_target|workflow_run)$/;
const OPT_IN_SWITCH = /--(?:include-)?ignored\b|\bOAC_TEST_[A-Z0-9_]+|tools[\\/]+herdr/;
const HARNESS_INSTALL = /@anthropic-ai\/claude-code|@openai\/codex|\bclaude\.ai\/install|\b(?:npm|npx|pnpm|yarn|bun)\b[^\n]*\b(?:claude-code|codex)\b|\bbrew\s+install\b[^\n]*\b(?:codex|claude)\b/i;

// Strip a YAML comment: a `#` at line start or after whitespace, outside quotes.
function stripComment(line) {
  let q = null;
  for (let i = 0; i < line.length; i++) {
    const c = line[i];
    if (q) {
      if (c === q) q = null;
    } else if (c === '"' || c === "'") {
      q = c;
    } else if (c === '#' && (i === 0 || /\s/.test(line[i - 1]))) {
      return line.slice(0, i).replace(/\s+$/, '');
    }
  }
  return line.replace(/\s+$/, '');
}

const indentOf = (l) => l.match(/^ */)[0].length;
const unquote = (s) => s.trim().replace(/^(['"])(.*)\1$/, '$2');

// The child keys of a top-level mapping key (`on`, `permissions`), or its inline value.
function topLevel(lines, key) {
  const re = new RegExp(`^(?:${key}|"${key}"|'${key}')\\s*:(.*)$`);
  const i = lines.findIndex((l) => re.test(l));
  if (i === -1) return null;
  const inline = lines[i].match(re)[1].trim();
  if (inline) return { inline, children: [] };
  const children = [];
  let childIndent = null;
  for (let j = i + 1; j < lines.length; j++) {
    const l = lines[j];
    if (l.trim() === '') continue;
    const ind = indentOf(l);
    if (ind === 0) break;
    if (childIndent === null) childIndent = ind;
    if (ind !== childIndent) continue;
    const t = l.trim();
    const m = t.match(/^-\s*(.+)$/) ?? t.match(/^([^:]+):\s*(.*)$/);
    if (!m) return { inline: null, children: null };
    children.push({ key: unquote(t.startsWith('-') ? m[1] : m[1]), value: t.startsWith('-') ? '' : unquote(m[2] ?? '') });
  }
  return { inline: null, children };
}

function triggers(lines) {
  const on = topLevel(lines, 'on');
  if (!on || on.children === null) return null;
  if (on.inline) {
    const v = on.inline.replace(/^\[|\]$/g, '');
    return v.split(',').map(unquote).filter(Boolean);
  }
  return on.children.map((c) => c.key);
}

function checkWorkflow(name, text) {
  const raw = text.split(/\r?\n/);
  const lines = raw.map(stripComment);
  const v = [];
  const hit = (rule, line, msg) => v.push({ rule, line, msg });

  // W1: every action pinned by commit SHA.
  lines.forEach((l, i) => {
    const m = l.match(/^\s*(?:-\s*)?uses\s*:\s*(.+)$/);
    if (!m) return;
    const ref = unquote(m[1]);
    if (ref.startsWith('./')) return; // a local action is part of this commit
    if (!SHA_PIN.test(ref)) hit('W1', i + 1, `action not pinned to a commit SHA: ${ref}`);
  });

  // W2: top-level permissions exactly contents: read; no job-level write.
  const perms = topLevel(lines, 'permissions');
  if (!perms || perms.children === null) hit('W2', 0, 'no readable top-level permissions block');
  else if (perms.inline !== null && perms.inline !== undefined && perms.inline !== '') {
    hit('W2', 0, `top-level permissions must be the block "contents: read", not ${perms.inline}`);
  } else {
    const kv = perms.children.map((c) => `${c.key}: ${c.value}`);
    if (kv.length !== 1 || kv[0] !== 'contents: read') hit('W2', 0, `top-level permissions must be exactly contents: read (found ${kv.join(', ') || 'none'})`);
  }
  lines.forEach((l, i) => {
    if (/^\s+permissions\s*:\s*write-all\b/.test(l) || /^\s+[\w-]+\s*:\s*write\s*$/.test(l)) {
      hit('W2', i + 1, 'a permission above read');
    }
  });

  // W3: checkout never persists the job token.
  lines.forEach((l, i) => {
    const m = l.match(/^(\s*)(-\s*)?uses\s*:\s*['"]?actions\/checkout@/);
    if (!m) return;
    // The step: from its `- ` line to the next line at the step's indent or shallower.
    let start = i;
    while (start > 0 && !/^\s*-\s/.test(lines[start])) start--;
    const stepIndent = indentOf(lines[start]);
    let end = start + 1;
    while (end < lines.length && (lines[end].trim() === '' || indentOf(lines[end]) > stepIndent)) end++;
    const step = lines.slice(start, end).join('\n');
    if (!/^\s*persist-credentials\s*:\s*false\s*$/m.test(step)) hit('W3', i + 1, 'actions/checkout without persist-credentials: false');
  });

  // W4: no secrets, job token or credentials; no privileged trigger.
  lines.forEach((l, i) => {
    if (SECRETS.test(l)) hit('W4', i + 1, 'secrets context or github.token');
    if (CREDENTIAL.test(l)) hit('W4', i + 1, 'provider or harness credential name');
  });
  const on = triggers(lines);
  if (on === null || on.length === 0) {
    hit('W4', 0, 'no readable `on:` triggers');
    return v;
  }
  for (const t of on) if (PRIVILEGED_TRIGGER.test(t)) hit('W4', 0, `privileged trigger ${t}`);

  // Default tier: any trigger other than workflow_dispatch.
  const optIn = on.every((t) => t === 'workflow_dispatch');
  if (optIn || name === HERDR_OPTIN) return v;
  lines.forEach((l, i) => {
    if (/^\s*runs-on\s*:.*self-hosted/.test(l) || /^\s*-\s*self-hosted\s*$/.test(l)) hit('D1', i + 1, 'self-hosted runner in a default-tier workflow');
    if (OPT_IN_SWITCH.test(l)) hit('D2', i + 1, 'opt-in switch in a default-tier workflow');
    if (HARNESS_INSTALL.test(l)) hit('D3', i + 1, 'harness CLI install in a default-tier workflow');
  });
  return v;
}

function checkAll(dir) {
  const files = readdirSync(dir).filter((f) => /\.ya?ml$/.test(f)).sort();
  if (files.length === 0) {
    console.error(`check-workflows: no workflow files under ${dir}`);
    return 2;
  }
  let total = 0;
  for (const f of files) {
    const v = checkWorkflow(f, readFileSync(join(dir, f), 'utf8'));
    for (const x of v) console.log(`.github/workflows/${f}:${x.line} [${x.rule}] ${x.msg}`);
    total += v.length;
  }
  console.log(`check-workflows: ${files.length} workflow(s), ${total} violation(s)`);
  console.log(total === 0 ? 'Result: CLEAN' : 'Result: FAIL');
  return total === 0 ? 0 : 1;
}

// ---- self-test ------------------------------------------------------------------------

const SHA = 'a'.repeat(40);
const GOOD = `# Default tier. No secrets, no provider.
name: good
on:
  push:
    branches: [main]
  pull_request:
permissions:
  contents: read
jobs:
  test:
    runs-on: \${{ matrix.os }}
    steps:
      - name: Check out
        uses: actions/checkout@${SHA} # v7.0.1
        with:
          persist-credentials: false
      - uses: actions/cache/restore@${SHA}
        with:
          key: cargo
      - run: cargo test --workspace # never --ignored here
`;
const OPTIN = `name: optin
on:
  workflow_dispatch:
permissions:
  contents: read
jobs:
  real:
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@${SHA}
        with:
          persist-credentials: false
      - env:
          OAC_TEST_REAL_KEYRING: "1"
        run: cargo test -- --ignored
`;

const CASES = [
  ['control: default-tier workflow', 'good.yml', GOOD, []],
  ['control: opt-in workflow may use --ignored and OAC_TEST_*', 'optin.yml', OPTIN, []],
  ['control: inline on list', 'x.yml', GOOD.replace(/on:\n  push:\n    branches: \[main\]\n  pull_request:\n/, 'on: [push, pull_request]\n'), []],
  ['W1 tag-pinned action', 'x.yml', GOOD.replace(`checkout@${SHA}`, 'checkout@v5'), ['W1']],
  ['W1 docker image', 'x.yml', GOOD.replace(`actions/cache/restore@${SHA}`, 'docker://alpine:3'), ['W1']],
  ['W2 write permission at top level', 'x.yml', GOOD.replace('contents: read', 'contents: write'), ['W2', 'W2']],
  ['W2 read-all inline', 'x.yml', GOOD.replace('permissions:\n  contents: read', 'permissions: read-all'), ['W2']],
  ['W2 extra top-level permission', 'x.yml', GOOD.replace('  contents: read', '  contents: read\n  id-token: read'), ['W2']],
  ['W2 no permissions block', 'x.yml', GOOD.replace('permissions:\n  contents: read\n', ''), ['W2']],
  ['W2 job-level write', 'x.yml', GOOD.replace('    runs-on:', '    permissions:\n      pull-requests: write\n    runs-on:'), ['W2']],
  ['W3 checkout persisting credentials', 'x.yml', GOOD.replace('persist-credentials: false', 'fetch-depth: 0'), ['W3']],
  ['W4 secrets context', 'x.yml', GOOD.replace('key: cargo', 'key: ${{ secrets.CACHE_KEY }}'), ['W4']],
  ['W4 github.token', 'x.yml', GOOD.replace('key: cargo', 'key: ${{ github.token }}'), ['W4']],
  ['W4 provider key variable', 'x.yml', GOOD.replace('- run: cargo test', '- env:\n          ANTHROPIC_API_KEY: x\n        run: cargo test'), ['W4']],
  ['W4 secrets in an opt-in workflow', 'optin.yml', OPTIN.replace('OAC_TEST_REAL_KEYRING: "1"', 'TOKEN: ${{ secrets.T }}'), ['W4']],
  ['W4 pull_request_target', 'x.yml', GOOD.replace('  pull_request:', '  pull_request_target:'), ['W4']],
  ['D1 self-hosted runner', 'x.yml', GOOD.replace('${{ matrix.os }}', '[self-hosted, linux]'), ['D1']],
  ['D2 --ignored in the default tier', 'x.yml', GOOD.replace('cargo test --workspace #', 'cargo test --workspace -- --ignored #'), ['D2']],
  ['D2 OAC_TEST_ flag in the default tier', 'x.yml', GOOD.replace('- run: cargo test', '- env:\n          OAC_TEST_REAL_KEYRING: "1"\n        run: cargo test'), ['D2']],
  ['D2 herdr driver in the default tier', 'x.yml', GOOD.replace('cargo test --workspace', 'node tools/herdr/ci.mjs run'), ['D2']],
  ['D3 harness CLI install', 'x.yml', GOOD.replace('cargo test --workspace', 'npm install -g @openai/codex'), ['D3']],
];

function runSelfTest() {
  let failed = 0;
  for (const [name, file, text, want] of CASES) {
    const got = checkWorkflow(file, text).map((x) => x.rule);
    const ok = JSON.stringify(got) === JSON.stringify(want);
    console.log(`${ok ? 'ok  ' : 'FAIL'} ${name}${ok ? '' : ` -> ${JSON.stringify(got)}`}`);
    if (!ok) failed++;
  }
  console.log(`self-test: ${CASES.length - failed}/${CASES.length} passed`);
  return failed === 0 ? 0 : 1;
}

const args = process.argv.slice(2);
if (args.length === 1 && args[0] === '--self-test') process.exit(runSelfTest());
if (args.length !== 0) {
  console.error('usage: node scripts/check-workflows.mjs [--self-test]');
  process.exit(2);
}
process.exit(checkAll(WORKFLOWS));
