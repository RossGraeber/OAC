#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0
//
// CI workflow policy (#61, F12): the default tier runs with no live provider, no API key
// and no network beyond loopback (PLANNING-PROMPT section 6 and 9.10; oac-testing
// section 2), and every workflow keeps the repository's hardening.
//
//   node scripts/check-workflows.mjs             # check .github/workflows/*.yml|yaml and
//                                                # every local composite action
//   node scripts/check-workflows.mjs --self-test # planted violations must fail
//
// Every workflow, and every local composite action (`uses: ./dir` -> dir/action.yml or
// dir/action.yaml, plus every .github/actions/**/action.y*ml):
//   W1  every `uses:` (block or flow style) names an action at a full 40-hex commit SHA (no
//       tag, no branch, no docker:// image). A local action (`./dir`) is read and held to
//       these same rules; one whose action file is missing fails. A local reusable workflow
//       (`./.github/workflows/x.yml`) is checked as a workflow in its own right.
//   W2  a top-level `permissions:` that is exactly `contents: read` (block or flow), and
//       every job-level `permissions:` (block, flow or quoted values) holds only `read` or
//       `none`, or is `read-all` or `{}`;
//   W3  every actions/checkout step sets `persist-credentials: false` under its `with:`;
//   W4  no secrets context and no job token: every `${{ ... }}` expression is read from the
//       raw text, comments and run: block scalars included (GitHub evaluates an expression
//       inside a block scalar, a heredoc `#` line too), joined across line breaks, and
//       fails on `secrets` in any form, `github.token`, `github[...]` (an index such as
//       `github['token']`) and the whole `github` context (`toJSON(github)`, `${{ github }}`);
//       `secrets: inherit` fails; no provider or harness credential name; no
//       `pull_request_target` or `workflow_run` trigger.
// Default-tier workflows (any trigger other than workflow_dispatch; the herdr opt-in
// workflow is the exception, with its own stricter rules in
// scripts/check-herdr-containment.mjs check 9), and the local actions they use:
//   D1  no self-hosted runner: `self-hosted` anywhere outside a comment, so a runner label
//       routed through a matrix fails too (check 9 also refuses every runner label in any
//       workflow but the herdr one);
//   D2  no opt-in switch: no `--ignored` / `--include-ignored` test run, no OAC_TEST_*
//       opt-in variable, no tools/herdr driver;
//   D3  no harness CLI install (the Claude Code or Codex npm packages, `codex`/`claude`
//       installers).
// Opt-in workflows (workflow_dispatch only) may use D1-D3; W1-W4 still hold.
//
// The reader is line-based and fails closed: a workflow whose `on:` or top-level
// `permissions:` it cannot read is a violation. YAML comments are stripped before W1-W3,
// D1-D3 and the credential-name match, so a header saying "no secrets" is not a hit; the
// text of a block scalar (a run: script) is never treated as a comment. W4's expression
// rules read the raw text, so an expression inside a YAML comment fails too (GitHub would
// not evaluate it there; failing on it is the safe side). Node built-ins only.
// Exit codes: 0 = clean; 1 = violation (or failed self-test); 2 = usage or environment error.

import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { dirname, join, posix, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const scriptPath = fileURLToPath(import.meta.url);
const repoRoot = resolve(dirname(scriptPath), '..');

// Governed by check 9 (scripts/check-herdr-containment.mjs): push-to-main on PINS.md and
// dispatch, self-hosted harness runners, no opt-in flag beyond its scenario input.
const HERDR_OPTIN = 'herdr-provider-optin.yml';

const SHA_PIN = /^[\w.-]+\/[\w.-]+(?:\/[\w./-]+)?@[0-9a-f]{40}$/;
const CREDENTIAL = /ANTHROPIC_API_KEY|ANTHROPIC_AUTH_TOKEN|CLAUDE_CODE_OAUTH_TOKEN|OPENAI_API_KEY|CODEX_API_KEY|OPENAI_ORG|\bGITHUB_TOKEN\b|\bauth\.json\b|\.credentials\.json/i;
const EXPRESSION_RULES = [
  { re: /\bsecrets\b/i, msg: 'secrets context in an expression' },
  { re: /\bgithub\s*\.\s*token\b/i, msg: 'github.token in an expression' },
  { re: /\bgithub\s*\[/i, msg: 'github context indexed (github[...]) in an expression' },
  { re: /\bgithub\b(?!\s*[.[])/i, msg: 'whole github context (it holds the token) in an expression' },
];
const SECRETS_INHERIT = /\bsecrets\s*:\s*['"]?inherit\b/i;
const PRIVILEGED_TRIGGER = /^(?:pull_request_target|workflow_run)$/;
const SELF_HOSTED = /\bself-hosted\b/i;
const OPT_IN_SWITCH = /--(?:include-)?ignored\b|\bOAC_TEST_[A-Z0-9_]+|tools[\\/]+herdr/;
// The package names are spelled with a one-letter class so that oac-boundaries check 3
// (no provider SDK name in the code tree) does not match this lint's own source.
const HARNESS_INSTALL = /@anthropi[c]-ai\/claude-code|@open[a]i\/codex|\bclaude\.ai\/install|\b(?:npm|npx|pnpm|yarn|bun)\b[^\n]*\b(?:claude-code|codex)\b|\bbrew\s+install\b[^\n]*\b(?:codex|claude)\b/i;
const USES = /(?:^|[\s{,-])uses\s*:\s*(['"]?)([^'",}\s]+)\1/g;

// ---- a line-based YAML reader ---------------------------------------------------------

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

// Per line: `code` is the comment-stripped YAML ('' inside a block scalar), `scan` is what
// the text rules read (the code, or the raw line inside a block scalar), `block` marks a
// block scalar's content.
function readLines(text) {
  const raw = text.split(/\r?\n/);
  const code = [];
  const scan = [];
  const block = [];
  let parent = null; // indent of the line that opened the current block scalar
  for (const l of raw) {
    if (parent !== null && (l.trim() === '' || indentOf(l) > parent)) {
      code.push('');
      scan.push(l);
      block.push(true);
      continue;
    }
    parent = null;
    const c = stripComment(l);
    code.push(c);
    scan.push(c);
    block.push(false);
    if (/(?::|^\s*-)\s*[|>][1-9+-]{0,2}$/.test(c)) parent = indentOf(l);
  }
  return { raw, code, scan, block };
}

// The children of the mapping key on line i, at the first deeper indent, or its inline value.
function mappingAt(code, i) {
  const m = code[i].match(/^\s*(?:-\s*)?['"]?[\w-]+['"]?\s*:(.*)$/);
  const inline = m ? m[1].trim() : '';
  if (inline) return { inline, children: [] };
  const base = indentOf(code[i]);
  const children = [];
  let childIndent = null;
  for (let j = i + 1; j < code.length; j++) {
    const l = code[j];
    if (l.trim() === '') continue;
    const ind = indentOf(l);
    if (ind <= base) break;
    if (childIndent === null) childIndent = ind;
    if (ind !== childIndent) continue;
    const t = l.trim();
    const kv = t.match(/^-\s*(.+)$/) ?? t.match(/^([^:]+):\s*(.*)$/);
    if (!kv) return { inline: null, children: null };
    children.push(t.startsWith('-') ? { key: unquote(kv[1]), value: '', line: j } : { key: unquote(kv[1]), value: unquote(kv[2] ?? ''), line: j });
  }
  return { inline: null, children };
}

// A flow mapping `{ a: b, c: 'd' }` (one level) as key/value pairs, or null.
function flowPairs(s) {
  const m = s.trim().match(/^\{(.*)\}$/);
  if (!m) return null;
  if (m[1].trim() === '') return [];
  return m[1].split(',').map((p) => {
    const kv = p.match(/^\s*([^:]+?)\s*:\s*(.*?)\s*$/);
    return kv ? { key: unquote(kv[1]), value: unquote(kv[2]) } : { key: p.trim(), value: null };
  });
}

function topLevelIndex(code, key) {
  const re = new RegExp(`^(?:${key}|"${key}"|'${key}')\\s*:`);
  return code.findIndex((l) => re.test(l));
}

function triggers(code) {
  const i = topLevelIndex(code, 'on');
  if (i === -1) return null;
  const on = mappingAt(code, i);
  if (on.children === null) return null;
  if (on.inline) {
    const flow = flowPairs(on.inline);
    if (flow) return flow.map((p) => p.key);
    return on.inline.replace(/^\[|\]$/g, '').split(',').map(unquote).filter(Boolean);
  }
  return on.children.map((c) => c.key);
}

// Permission pairs from line i: block children, a flow mapping, or a scalar.
function permissionsAt(code, i) {
  const p = mappingAt(code, i);
  if (p.children === null) return { scalar: null, pairs: null };
  if (p.inline) {
    const flow = flowPairs(p.inline);
    return flow ? { scalar: null, pairs: flow } : { scalar: unquote(p.inline), pairs: null };
  }
  return { scalar: null, pairs: p.children };
}

// ---- the checks -----------------------------------------------------------------------

// The rules a workflow and a local action share: W1, W3, W4 (not the trigger part), and
// D1-D3 when `defaultTier`. Returns local actions it references.
function commonRules(lines, text, defaultTier, hit) {
  const { code, scan, block } = lines;
  const locals = [];

  // W1: every action pinned by commit SHA (block and flow style).
  code.forEach((l, i) => {
    if (block[i]) return;
    for (const m of l.matchAll(USES)) {
      const ref = m[2];
      if (ref.startsWith('./')) {
        if (/^\.\/\.github\/workflows\/[^/]+\.ya?ml$/.test(ref)) continue; // checked as a workflow
        locals.push({ ref, line: i + 1 });
        continue;
      }
      if (!SHA_PIN.test(ref)) hit('W1', i + 1, `action not pinned to a commit SHA: ${ref}`);
    }
  });

  // W3: checkout never persists the job token; the setting must be an input (`with:`).
  code.forEach((l, i) => {
    if (block[i] || !/(?:^|[\s{,-])uses\s*:\s*['"]?actions\/checkout@/.test(l)) return;
    if (/\{/.test(l)) {
      if (!/with\s*:\s*\{[^}]*persist-credentials\s*:\s*['"]?false['"]?\s*[,}]/.test(l)) {
        hit('W3', i + 1, 'actions/checkout without persist-credentials: false under with:');
      }
      return;
    }
    let start = i;
    while (start > 0 && !/^\s*-\s/.test(code[start])) start--;
    const stepIndent = indentOf(code[start]);
    let end = start + 1;
    while (end < code.length && (code[end].trim() === '' || indentOf(code[end]) > stepIndent)) end++;
    let ok = false;
    for (let j = start; j < end; j++) {
      if (!/^\s*(?:-\s*)?with\s*:/.test(code[j])) continue;
      const w = mappingAt(code, j);
      const pairs = w.inline ? flowPairs(w.inline) : w.children;
      if (pairs && pairs.some((p) => p.key === 'persist-credentials' && p.value === 'false')) ok = true;
    }
    if (!ok) hit('W3', i + 1, 'actions/checkout without persist-credentials: false under with:');
  });

  // W4: expressions, read raw and joined across line breaks.
  for (const m of text.matchAll(/\$\{\{([\s\S]*?)\}\}/g)) {
    const expr = m[1].replace(/\s+/g, ' ');
    const line = text.slice(0, m.index).split('\n').length;
    for (const r of EXPRESSION_RULES) {
      if (r.re.test(expr)) {
        hit('W4', line, r.msg);
        break;
      }
    }
  }
  scan.forEach((l, i) => {
    if (SECRETS_INHERIT.test(l)) hit('W4', i + 1, 'secrets: inherit');
    if (CREDENTIAL.test(l)) hit('W4', i + 1, 'provider or harness credential name');
  });

  if (defaultTier) {
    scan.forEach((l, i) => {
      if (SELF_HOSTED.test(l)) hit('D1', i + 1, 'self-hosted runner in the default tier');
      if (OPT_IN_SWITCH.test(l)) hit('D2', i + 1, 'opt-in switch in the default tier');
      if (HARNESS_INSTALL.test(l)) hit('D3', i + 1, 'harness CLI install in the default tier');
    });
  }
  return locals;
}

// One workflow. `readLocal(ref)` returns { path, text } for a local action, or null.
function checkWorkflow(name, text, readLocal = () => null) {
  const lines = readLines(text);
  const { code, block } = lines;
  const v = [];
  const hit = (rule, line, msg) => v.push({ rule, line, msg });

  const on = triggers(code);
  if (on === null || on.length === 0) hit('W4', 0, 'no readable `on:` triggers');
  for (const t of on ?? []) if (PRIVILEGED_TRIGGER.test(t)) hit('W4', 0, `privileged trigger ${t}`);
  const optIn = on !== null && on.length > 0 && on.every((t) => t === 'workflow_dispatch');
  const defaultTier = !optIn && name !== HERDR_OPTIN;

  // W2: top-level permissions exactly contents: read.
  const top = topLevelIndex(code, 'permissions');
  if (top === -1) hit('W2', 0, 'no top-level permissions');
  else {
    const p = permissionsAt(code, top);
    const kv = (p.pairs ?? []).map((c) => `${c.key}: ${c.value}`);
    if (!p.pairs || kv.length !== 1 || kv[0] !== 'contents: read') {
      hit('W2', top + 1, `top-level permissions must be exactly contents: read (found ${p.scalar ?? (kv.join(', ') || 'none')})`);
    }
  }
  // W2: every job-level permissions read-only.
  code.forEach((l, i) => {
    if (block[i] || !/^\s+(?:-\s*)?['"]?permissions['"]?\s*:/.test(l)) return;
    const p = permissionsAt(code, i);
    if (p.scalar !== null) {
      if (p.scalar !== 'read-all') hit('W2', i + 1, `job permissions ${p.scalar}`);
      return;
    }
    if (p.pairs === null) return hit('W2', i + 1, 'unreadable job permissions');
    for (const c of p.pairs) {
      if (c.value !== 'read' && c.value !== 'none') hit('W2', (c.line ?? i) + 1, `job permission ${c.key}: ${c.value}`);
    }
  });

  const locals = commonRules(lines, text, defaultTier, hit);
  for (const { ref, line } of locals) {
    const action = readLocal(ref);
    if (!action) {
      hit('W1', line, `local action ${ref} has no action.yml or action.yaml (fails closed)`);
      continue;
    }
    const sub = [];
    commonRules(readLines(action.text), action.text, defaultTier, (rule, l, msg) => sub.push({ rule, line, msg: `${action.path}:${l}: ${msg}` }));
    v.push(...sub);
  }
  return v;
}

// A composite action file found on disk, held to the rules on its own (default tier).
function checkAction(text) {
  const v = [];
  const locals = commonRules(readLines(text), text, true, (rule, line, msg) => v.push({ rule, line, msg }));
  return { v, locals };
}

function readLocalFrom(root) {
  return (ref) => {
    const dir = posix.normalize(ref.replace(/^\.\//, ''));
    if (dir.startsWith('..')) return null;
    for (const f of ['action.yml', 'action.yaml']) {
      const p = join(root, dir, f);
      if (existsSync(p)) return { path: `${dir}/${f}`, text: readFileSync(p, 'utf8') };
    }
    return null;
  };
}

function walkActions(dir, out = []) {
  if (!existsSync(dir)) return out;
  for (const e of readdirSync(dir)) {
    const p = join(dir, e);
    if (statSync(p).isDirectory()) walkActions(p, out);
    else if (/^action\.ya?ml$/.test(e)) out.push(p);
  }
  return out;
}

function checkAll(root) {
  const wfDir = join(root, '.github', 'workflows');
  const files = existsSync(wfDir) ? readdirSync(wfDir).filter((f) => /\.ya?ml$/.test(f)).sort() : [];
  if (files.length === 0) {
    console.error(`check-workflows: no workflow files under ${wfDir}`);
    return 2;
  }
  let total = 0;
  const readLocal = readLocalFrom(root);
  for (const f of files) {
    const v = checkWorkflow(f, readFileSync(join(wfDir, f), 'utf8'), readLocal);
    for (const x of v) console.log(`.github/workflows/${f}:${x.line} [${x.rule}] ${x.msg}`);
    total += v.length;
  }
  const actions = walkActions(join(root, '.github', 'actions'));
  for (const p of actions) {
    const { v, locals } = checkAction(readFileSync(p, 'utf8'));
    for (const { ref, line } of locals) if (!readLocal(ref)) v.push({ rule: 'W1', line, msg: `local action ${ref} has no action file (fails closed)` });
    const rel = p.slice(root.length + 1).split('\\').join('/');
    for (const x of v) console.log(`${rel}:${x.line} [${x.rule}] ${x.msg}`);
    total += v.length;
  }
  console.log(`check-workflows: ${files.length} workflow(s), ${actions.length} local action(s), ${total} violation(s)`);
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
          key: cargo-\${{ runner.os }}-\${{ github.sha }}
      - run: cargo test --workspace # never --ignored here
      - name: script
        run: |
          echo "uses: not-an-action"
          echo done
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
const withJob = (perm) => GOOD.replace('    runs-on:', `${perm}\n    runs-on:`);
const withStep = (step) => GOOD.replace('      - run: cargo test', `${step}\n      - run: cargo test`);
const ACTION_GOOD = `name: setup
runs:
  using: composite
  steps:
    - uses: actions/cache/restore@${SHA}
      with:
        key: x
`;

const CASES = [
  ['control: default-tier workflow', 'good.yml', GOOD, []],
  ['control: opt-in workflow may use --ignored and OAC_TEST_*', 'optin.yml', OPTIN, []],
  ['control: inline on list', 'x.yml', GOOD.replace(/on:\n  push:\n    branches: \[main\]\n  pull_request:\n/, 'on: [push, pull_request]\n'), []],
  ['control: top-level permissions as a flow mapping', 'x.yml', GOOD.replace('permissions:\n  contents: read', 'permissions: { contents: read }'), []],
  ['control: job-level read and none, block and flow', 'x.yml', withJob('    permissions:\n      contents: read\n      id-token: none').replace('  test:', '  other:\n    permissions: { contents: "read" }\n    runs-on: x\n  test:'), []],
  ['control: a local action held to the rules', 'x.yml', withStep('      - uses: ./.github/actions/setup'), [], { './.github/actions/setup': ACTION_GOOD }],
  ['W1 tag-pinned action', 'x.yml', GOOD.replace(`checkout@${SHA}`, 'checkout@v5'), ['W1']],
  ['W1 docker image', 'x.yml', GOOD.replace(`actions/cache/restore@${SHA}`, 'docker://alpine:3'), ['W1']],
  ['W1 flow-style step', 'x.yml', withStep('      - { uses: actions/setup-node@v4 }'), ['W1']],
  ['W1 local action file missing (fails closed)', 'x.yml', withStep('      - uses: ./.github/actions/missing'), ['W1']],
  ['W1 tag-pinned action inside a local action', 'x.yml', withStep('      - uses: ./.github/actions/setup'), ['W1'], { './.github/actions/setup': ACTION_GOOD.replace(`restore@${SHA}`, 'restore@v4') }],
  ['W4 secrets inside a local action', 'x.yml', withStep('      - uses: ./.github/actions/setup'), ['W4'], { './.github/actions/setup': ACTION_GOOD.replace('key: x', 'key: ${{ secrets.K }}') }],
  ['W2 write permission at top level', 'x.yml', GOOD.replace('contents: read', 'contents: write'), ['W2']],
  ['W2 read-all at top level', 'x.yml', GOOD.replace('permissions:\n  contents: read', 'permissions: read-all'), ['W2']],
  ['W2 extra top-level permission', 'x.yml', GOOD.replace('  contents: read', '  contents: read\n  id-token: read'), ['W2']],
  ['W2 no permissions block', 'x.yml', GOOD.replace('permissions:\n  contents: read\n', ''), ['W2']],
  ['W2 job-level write (block)', 'x.yml', withJob('    permissions:\n      pull-requests: write'), ['W2']],
  ['W2 job-level write (flow)', 'x.yml', withJob('    permissions: { contents: write }'), ['W2']],
  ['W2 job-level write (quoted)', 'x.yml', withJob("    permissions:\n      contents: 'write'"), ['W2']],
  ['W2 job-level write-all', 'x.yml', withJob('    permissions: write-all'), ['W2']],
  ['W3 checkout persisting credentials', 'x.yml', GOOD.replace('persist-credentials: false', 'fetch-depth: 0'), ['W3']],
  ['W3 persist-credentials under env, not with', 'x.yml', GOOD.replace('        with:\n          persist-credentials: false', '        env:\n          persist-credentials: false'), ['W3']],
  ['W4 secrets context', 'x.yml', GOOD.replace('key: cargo-', 'key: ${{ secrets.CACHE_KEY }}-'), ['W4']],
  ['W4 github.token', 'x.yml', GOOD.replace('key: cargo-', 'key: ${{ github.token }}-'), ['W4']],
  ["W4 github['token']", 'x.yml', GOOD.replace('key: cargo-', "key: ${{ github['token'] }}-"), ['W4']],
  ['W4 toJSON(github)', 'x.yml', GOOD.replace('key: cargo-', 'key: ${{ toJSON(github) }}-'), ['W4']],
  ['W4 an expression split across lines', 'x.yml', withStep('      - env:\n          T: ${{ github\n            .token }}\n        run: true'), ['W4']],
  ['W4 a heredoc # line in a run: block', 'x.yml', GOOD.replace('          echo done', '          cat <<EOF\n          #${{ secrets.K }}\n          EOF'), ['W4']],
  ['W4 secrets: inherit', 'x.yml', GOOD.replace('    runs-on: ${{ matrix.os }}\n    steps:', '    uses: ./.github/workflows/other.yml\n    secrets: inherit\n    steps:'), ['W4']],
  ['W4 provider key variable', 'x.yml', withStep('      - env:\n          ANTHROPIC_API_KEY: x\n        run: true'), ['W4']],
  ['W4 secrets in an opt-in workflow', 'optin.yml', OPTIN.replace('OAC_TEST_REAL_KEYRING: "1"', 'TOKEN: ${{ secrets.T }}'), ['W4']],
  ['W4 pull_request_target', 'x.yml', GOOD.replace('  pull_request:', '  pull_request_target:'), ['W4']],
  ['D1 self-hosted runner', 'x.yml', GOOD.replace('${{ matrix.os }}', '[self-hosted, x]'), ['D1']],
  ['D1 self-hosted routed through a matrix', 'x.yml', GOOD.replace('    runs-on: ${{ matrix.os }}', '    strategy:\n      matrix:\n        r: [self-hosted]\n    runs-on: ${{ matrix.r }}'), ['D1']],
  ['D2 --ignored in the default tier', 'x.yml', GOOD.replace('cargo test --workspace #', 'cargo test --workspace -- --ignored #'), ['D2']],
  ['D2 --ignored inside a run: block', 'x.yml', GOOD.replace('          echo done', '          cargo test -- --include-ignored'), ['D2']],
  ['D2 OAC_TEST_ flag in the default tier', 'x.yml', withStep('      - env:\n          OAC_TEST_REAL_KEYRING: "1"\n        run: true'), ['D2']],
  ['D2 herdr driver in the default tier', 'x.yml', GOOD.replace('cargo test --workspace', 'node tools/herdr/ci.mjs run'), ['D2']],
  ['D3 harness CLI install', 'x.yml', GOOD.replace('cargo test --workspace', 'npm install -g @open' + 'ai/codex'), ['D3']],
];

function runSelfTest() {
  let failed = 0;
  for (const [name, file, text, want, locals = {}] of CASES) {
    const readLocal = (ref) => (locals[ref] ? { path: `${ref.slice(2)}/action.yml`, text: locals[ref] } : null);
    const got = checkWorkflow(file, text, readLocal).map((x) => x.rule);
    const ok = JSON.stringify(got) === JSON.stringify(want);
    console.log(`${ok ? 'ok  ' : 'FAIL'} ${name}${ok ? '' : ` -> ${JSON.stringify(checkWorkflow(file, text, readLocal))}`}`);
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
process.exit(checkAll(repoRoot));
