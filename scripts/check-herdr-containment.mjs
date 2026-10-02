#!/usr/bin/env node
// Keep herdr (Epic K, #123: dev/test-only terminal automation) contained. This is
// oac-boundaries checks 9 and 10 (.claude/skills/oac-boundaries/references/mechanical-checks.md).
//
//   node scripts/check-herdr-containment.mjs              # check this repository
//   node scripts/check-herdr-containment.mjs --root <dir> # check another git work tree
//   node scripts/check-herdr-containment.mjs --self-test  # plant violations, assert each fails
//
// Check 9  (containment): no `herdr` / `HERDR_` match (case-insensitive) in any git-tracked
//          entry under adapters/, core/, cli/, transports/, or spec/; and no workspace or
//          package manifest outside tools/herdr/ that references tools/herdr.
//          tests/integration/ (#146) may name and spawn herdr, but nothing under a product
//          path, and no manifest outside it, references it, and it never imports, compiles
//          in, or symlinks to driver code. Workflows
//          (K6 #129): no GitHub Actions workflow other than
//          .github/workflows/herdr-provider-optin.yml references tools/herdr or a label a
//          self-hosted runner carries; and that opt-in workflow's triggers are exactly
//          workflow_dispatch + push (main, docs/planning/PINS.md), its permissions exactly
//          contents: read, every action is actions/checkout or actions/upload-artifact at
//          a commit SHA (checkout without persisted credentials), with no secrets or
//          github.token use, no expression inside a `run:` block, and no direct driver
//          call or driver option (it runs tools/herdr/ci.mjs only). Drift only: a fork's
//          pull request runs its own copy of this lint.
// Check 10 (driver hygiene): no harness-credential access or harness-config mutation in any
//          git-tracked entry under tools/herdr/ -- auth.json, .credentials.json, provider
//          and harness credential variables, keyring/keychain/OS credential-store access,
//          writes to harness config files, and harness config-mutating CLI calls
//          (`integration install`, `claude mcp add`, ...). (ADR-001 boundaries 3, 4, 13.)
//
// What is scanned is the git index (what CI sees), never the work tree: every entry under a
// target, by its index mode. The tracked path itself is matched; a regular file's staged
// blob is matched; a symlink's stored target text is matched, then the link is resolved hop
// by hop through the tracked symlinks (including symlinked intermediate directories, each
// expanded before a following `..`) and the resolved repo path -- and, if it is a tracked
// file, its blob, or if a directory, every tracked entry under it -- is matched too; a
// link that resolves outside the repository fails outright. A submodule (gitlink) is
// matched by its path and its .gitmodules url. Nothing is skipped for not being a file.
//
// A target with no git-tracked files (the path is not built yet) reports PENDING, never
// clean: pending is not a pass, and the check must be re-read once the path exists.
// Hits print the tracked path, line, and rule label ONLY -- never matched text, the line,
// or a link target -- so no credential or home path can be echoed into a CI log.
//
// Exit codes: 0 = no violations (result CLEAN or PENDING, printed on the last line);
// 1 = at least one violation (or a failed self-test case); 2 = usage or environment error.

import { writeFileSync, mkdirSync, mkdtempSync, rmSync } from 'node:fs';
import { execFileSync, spawnSync } from 'node:child_process';
import { join, dirname, resolve, basename, posix } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';

const scriptPath = fileURLToPath(import.meta.url);

const PRODUCT_PATHS = ['adapters', 'core', 'cli', 'transports', 'spec'];
const DRIVER_PATH = 'tools/herdr';

// Covers `herdr`, `Herdr`, and every `HERDR_*` environment variable.
const HERDR_TOKEN = /herdr/i;
const HERDR_PATH_REF = /tools[\\/]+herdr/i;

// Top-level tests/integration/ (#146; tools/herdr/README.md "Reuse contract"): opt-in
// provider-integration tests that drive herdr as a separate process. herdr references there
// are intended and allowed; what is checked is that the tests never pull driver code in,
// and that nothing in a product path or a default build pulls the tests in.
const INTEGRATION_PATH = 'tests/integration';
// A reference that climbs out of its own directory to tests/integration (`../../tests/
// integration`); a crate's own `tests/integration.rs` or `tests/integration/` is not one.
const INTEGRATION_CLIMB_REF = /(?:\.\.[\\/]+)+tests[\\/]+integration(?![\w.-])/i;
// From a root-level manifest, any path naming tests/integration.
const INTEGRATION_ROOT_REF = /tests[\\/]+integration(?![\w.-])/i;
// Driver code imported or compiled into a test: JS import/require, Python import, Rust
// #[path]/include!/include_str!/include_bytes!. Passing tools/herdr/run.mjs to a spawned
// process is the contract, not an import, and is allowed.
const DRIVER_IMPORT = new RegExp(
  String.raw`\b(?:import|from|require)\b\s*\(?\s*['"\x60][^'"\x60]*tools[\\/]+herdr` +
    String.raw`|^\s*(?:from|import)\s+tools\.herdr\b` +
    String.raw`|#\s*\[\s*path\s*=\s*"[^"]*tools[\\/]+herdr|\binclude(?:_str|_bytes)?!\s*\([^;]*tools[\\/]+herdr`,
  'i',
);
// linkOnly rules are matched only against the repo path a symlink lands on.
const landsIn = (dir) => new RegExp(`^${dir.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}(?:/|$)`, 'i');

// GitHub Actions workflows (K6 #129). Only the opt-in workflow may reach the driver or the
// operator-owned harness runners.
const WORKFLOW_DIR = '.github/workflows';
const OPTIN_WORKFLOW = `${WORKFLOW_DIR}/herdr-provider-optin.yml`;
const isWorkflow = (p) => p.startsWith(`${WORKFLOW_DIR}/`) && /\.ya?ml$/i.test(p);

// Line numbers of `${{ }}` expressions inside a `run:` value (inline or block scalar). A
// block runs while lines are blank or indented deeper than the `run` key.
function runBlockExpressions(text) {
  const lines = text.split(/\r?\n/);
  const hits = [];
  for (let i = 0; i < lines.length; i++) {
    const m = /^(\s*(?:-\s+)?)run\s*:(.*)$/.exec(lines[i]);
    if (!m) continue;
    const keyCol = m[1].length;
    if (m[2].includes('${{')) hits.push(i + 1);
    for (let j = i + 1; j < lines.length; j++) {
      const l = lines[j];
      if (l.trim() !== '' && l.length - l.trimStart().length <= keyCol) break;
      if (l.includes('${{')) hits.push(j + 1);
    }
  }
  return hits;
}

// A tolerant, fail-closed reader for the small YAML subset a workflow uses: block
// mappings, block sequences (including `- key: value` items), flow sequences `[a, b]`,
// `{}`/simple flow mappings, plain and quoted scalars, and `|`/`>` block scalars (skipped).
// Anything else throws, and the rules that need the parse then fail. Mappings are Maps of
// key -> { value, line }.
function stripYamlComment(line) {
  let quote = null;
  for (let i = 0; i < line.length; i++) {
    const c = line[i];
    if (quote) {
      if (c === quote) quote = null;
    } else if (c === '"' || c === "'") quote = c;
    else if (c === '#' && (i === 0 || /\s/.test(line[i - 1]))) return line.slice(0, i).trimEnd();
  }
  return line.trimEnd();
}
const unquote = (v) => {
  const t = v.trim();
  return /^(['"]).*\1$/.test(t) ? t.slice(1, -1) : t;
};
function parseFlow(v) {
  const t = v.trim();
  if (t.startsWith('[')) {
    if (!t.endsWith(']')) throw new Error('unterminated flow sequence');
    const inner = t.slice(1, -1).trim();
    if (/[[\]{}]/.test(inner.replace(/\$\{\{.*?\}\}/g, ''))) throw new Error('nested flow collection');
    return inner === '' ? [] : inner.split(',').map(unquote);
  }
  if (t.startsWith('{')) {
    if (!t.endsWith('}')) throw new Error('unterminated flow mapping');
    const inner = t.slice(1, -1).trim();
    const m = new Map();
    if (inner === '') return m;
    for (const part of inner.split(',')) {
      const kv = /^\s*([^:]+?)\s*:\s*(.*)$/.exec(part);
      if (!kv) throw new Error('bad flow mapping');
      m.set(unquote(kv[1]), { value: unquote(kv[2]), line: null });
    }
    return m;
  }
  return unquote(t);
}
function parseYamlLite(text) {
  const lines = [];
  text.split(/\r?\n/).forEach((raw, i) => {
    if (raw.includes('\t')) throw new Error(`tab indentation at line ${i + 1}`);
    const c = stripYamlComment(raw);
    if (c.trim() !== '') lines.push({ n: i + 1, indent: c.length - c.trimStart().length, text: c.trim() });
  });
  let pos = 0;
  const KEY = /^("[^"]*"|'[^']*'|[^\s:'"][^:]*?)\s*:(?:\s+(.*))?$/;
  function valueAfterKey(rest, indent) {
    if (rest === undefined || rest === '') {
      if (pos < lines.length && lines[pos].indent > indent) return parseBlock(lines[pos].indent);
      if (pos < lines.length && lines[pos].indent === indent && lines[pos].text.startsWith('-')) return parseBlock(indent);
      return null;
    }
    if (/^[|>][-+0-9]*$/.test(rest)) {
      while (pos < lines.length && lines[pos].indent > indent) pos++;
      return '<block scalar>';
    }
    return parseFlow(rest);
  }
  function parseBlock(indent) {
    const first = lines[pos];
    if (first.text === '-' || first.text.startsWith('- ')) {
      const seq = [];
      while (pos < lines.length && lines[pos].indent === indent && (lines[pos].text === '-' || lines[pos].text.startsWith('- '))) {
        const item = lines[pos].text.slice(1).trim();
        if (item === '') {
          pos++;
          seq.push(pos < lines.length && lines[pos].indent > indent ? parseBlock(lines[pos].indent) : null);
        } else if (KEY.test(item) && !/^\$\{\{/.test(item)) {
          lines[pos] = { n: lines[pos].n, indent: indent + 2, text: item };
          seq.push(parseBlock(indent + 2));
        } else {
          pos++;
          seq.push(parseFlow(item));
        }
      }
      return seq;
    }
    const map = new Map();
    while (pos < lines.length && lines[pos].indent === indent) {
      const { n, text: t } = lines[pos];
      if (t.startsWith('- ') || t === '-') break;
      const m = KEY.exec(t);
      if (!m) throw new Error(`cannot read line ${n}`);
      pos++;
      const key = unquote(m[1]);
      if (map.has(key)) throw new Error(`duplicate key ${key} at line ${n}`);
      map.set(key, { value: valueAfterKey(m[2], indent), line: n });
    }
    if (pos < lines.length && lines[pos].indent > indent) throw new Error(`unexpected indentation at line ${lines[pos].n}`);
    return map;
  }
  if (lines.length === 0) return new Map();
  const root = parseBlock(lines[0].indent);
  if (pos !== lines.length) throw new Error(`unexpected content at line ${lines[pos].n}`);
  if (!(root instanceof Map)) throw new Error('top level is not a mapping');
  return root;
}

// A rule over the parsed workflow: fn(root) -> line numbers; a parse failure is one hit.
const parsedRule = (fn) => (text) => {
  let root;
  try {
    root = parseYamlLite(text);
  } catch {
    return [1];
  }
  return fn(root);
};
const keysOf = (v) => (v instanceof Map ? [...v.keys()] : Array.isArray(v) ? v : typeof v === 'string' ? [v] : []);
const sameList = (v, want) => Array.isArray(v) && v.length === want.length && v.every((x, i) => x === want[i]);

// on: exactly { workflow_dispatch, push } -- an allowlist, so any other event (issues,
// watch, fork, discussion, pull_request*, workflow_run, schedule, ...) fails.
const OPTIN_TRIGGERS = ['push', 'workflow_dispatch'];
function optinTriggers(root) {
  const on = root.get('on');
  if (!on) return [1];
  const keys = keysOf(on.value);
  const bad = keys.filter((k) => !OPTIN_TRIGGERS.includes(k));
  if (bad.length) return [on.value instanceof Map ? on.value.get(bad[0]).line : on.line];
  if (keys.length !== OPTIN_TRIGGERS.length) return [on.line];
  return [];
}
// push: branches exactly [main], paths exactly [docs/planning/PINS.md], nothing else;
// workflow_dispatch: inputs only.
function optinTriggerFilters(root) {
  const on = root.get('on')?.value;
  // List or scalar form: a push listed there has no branch or path filter at all.
  if (!(on instanceof Map)) return keysOf(on).includes('push') ? [root.get('on')?.line ?? 1] : [];
  const hits = [];
  const push = on.get('push');
  if (push) {
    const pm = push.value;
    const ok =
      pm instanceof Map &&
      pm.size === 2 &&
      sameList(pm.get('branches')?.value, ['main']) &&
      sameList(pm.get('paths')?.value, ['docs/planning/PINS.md']);
    if (!ok) hits.push(push.line);
  }
  const wd = on.get('workflow_dispatch');
  if (wd && wd.value !== null && !(wd.value instanceof Map && [...wd.value.keys()].every((k) => k === 'inputs'))) hits.push(wd.line);
  return hits;
}
// permissions: exactly { contents: read } at the top, and no job-level permissions.
function optinPermissions(root) {
  const hits = [];
  const perm = root.get('permissions');
  const pm = perm?.value;
  if (!(pm instanceof Map && pm.size === 1 && pm.get('contents')?.value === 'read')) hits.push(perm?.line ?? 1);
  const jobs = root.get('jobs')?.value;
  if (jobs instanceof Map) for (const { value } of jobs.values()) if (value instanceof Map && value.has('permissions')) hits.push(value.get('permissions').line);
  return hits;
}
// Every `uses:` is a GitHub-owned action pinned by a full commit SHA; no reusable-workflow
// call, no local or docker action. checkout never persists the token.
const ALLOWED_USES = /^actions\/(?:checkout|upload-artifact)@[0-9a-f]{40}$/;
function optinUses(text) {
  const hits = [];
  text.split(/\r?\n/).forEach((raw, i) => {
    const m = /^\s*(?:-\s+)?uses\s*:\s*(.*)$/.exec(stripYamlComment(raw));
    if (m && !ALLOWED_USES.test(unquote(m[1]))) hits.push(i + 1);
  });
  return hits;
}
const optinCheckoutToken = parsedRule((root) => {
  const hits = [];
  const jobs = root.get('jobs')?.value;
  for (const { value: job } of jobs instanceof Map ? jobs.values() : []) {
    const steps = job instanceof Map ? job.get('steps')?.value : null;
    for (const step of Array.isArray(steps) ? steps : []) {
      if (!(step instanceof Map)) continue;
      const uses = step.get('uses');
      if (uses && /^actions\/checkout@/.test(String(uses.value)) && step.get('with')?.value?.get?.('persist-credentials')?.value !== 'false') hits.push(uses.line);
    }
  }
  return hits;
});
// Secrets in any spelling: secrets.X, secrets['X'], toJSON(secrets), secrets: inherit.
const SECRETS_RE = /\bsecrets\s*(?:[.[)]|:)|\(\s*secrets\b|\$\{\{[^}]*\bsecrets\b/i;
const JOB_TOKEN_RE = /\bgithub\s*(?:\.\s*token\b|\[\s*['"]token['"]\s*\])/i;

// Labels a self-hosted runner carries by default (use-in-a-workflow.md) plus ours. A job in
// another workflow whose runs-on names any of them could be routed to a harness runner.
const RUNNER_LABEL_TOKEN = /(?<![\w.-])(?:self-hosted|oac-harness|linux|windows|macos|x64|arm64|arm)(?![\w.-])/i;

const OTHER_WORKFLOW_RULES = [
  { label: `workflow other than ${OPTIN_WORKFLOW} references tools/herdr`, re: HERDR_PATH_REF },
  {
    label: `workflow other than ${OPTIN_WORKFLOW} names a label a self-hosted runner carries (self-hosted, oac-harness, linux, windows, macos, x64, arm, arm64)`,
    re: RUNNER_LABEL_TOKEN,
  },
];
const OPTIN_WORKFLOW_RULES = [
  { label: 'opt-in workflow triggers are not exactly workflow_dispatch and push', scan: parsedRule(optinTriggers) },
  { label: 'opt-in workflow push is not main-only on docs/planning/PINS.md, or workflow_dispatch has more than inputs', scan: parsedRule(optinTriggerFilters) },
  { label: 'opt-in workflow permissions are not exactly contents: read', scan: parsedRule(optinPermissions) },
  { label: 'opt-in workflow uses an action that is not actions/checkout or actions/upload-artifact pinned by commit SHA', scan: optinUses },
  { label: 'opt-in workflow checkout does not set persist-credentials: false', scan: optinCheckoutToken },
  { label: 'opt-in workflow reads the secrets context', re: SECRETS_RE },
  { label: 'opt-in workflow reads the job token (github.token)', re: JOB_TOKEN_RE },
  { label: 'opt-in workflow calls the driver directly or passes a driver option', re: /run\.mjs|--keep-scratch|--herdr-bin|--launch|--param|accept\s*=\s*driver/i },
  { label: 'opt-in workflow has an expression inside a run: block', scan: runBlockExpressions },
];

// Workspace and package manifests (and lockfiles, which record path dependencies) for the
// toolchains the C1 language decision left in play.
const MANIFEST_NAMES = new Set([
  'Cargo.toml',
  'Cargo.lock',
  'package.json',
  'package-lock.json',
  'npm-shrinkwrap.json',
  'pnpm-workspace.yaml',
  'pnpm-lock.yaml',
  'yarn.lock',
  'deno.json',
  'deno.jsonc',
  'go.mod',
  'go.work',
  'pyproject.toml',
]);

// --- harness-config write detection ---------------------------------------
//
// Harness config: ~/.claude/settings(.local).json (or under $CLAUDE_CONFIG_DIR),
// ~/.codex/config.toml (or under $CODEX_HOME), and hooks.json directly in either config
// directory. Reading or hashing these is allowed (K3 records before/after hashes); writing,
// moving, or deleting them is not. Each file name is suffix-anchored, so a driver's own
// scratch file such as hooks.json.sha256 is not config (#135).
const CONFIG_END = String.raw`(?![\w.])`;

// In JS source, path segments may be split across join() arguments: '.claude', 'settings.json'.
const CONFIG_JS = new RegExp(
  String.raw`(?:\.claude|CLAUDE_CONFIG_DIR)\W{1,6}(?:settings(?:\.local)?\.json|hooks\.json)${CONFIG_END}` +
    String.raw`|(?:\.codex|CODEX_HOME)\W{1,6}(?:config\.toml|hooks\.json)${CONFIG_END}`,
  'i',
);
// In shell text, a single path token: ~/.claude/settings.json, "$CODEX_HOME"/config.toml.
const CONFIG_SH =
  String.raw`(?:(?:\.claude|CLAUDE_CONFIG_DIR)[}'"]*[\\/](?:settings(?:\.local)?\.json|hooks\.json)` +
  String.raw`|(?:\.codex|CODEX_HOME)[}'"]*[\\/](?:config\.toml|hooks\.json))${CONFIG_END}`;
// Start of a shell command word: line start, whitespace, an operator, or an opening quote.
const SH_START = String.raw`(?:^|[\s;|&(\x60'"])`;
const SH_TOKEN_END = String.raw`['"]?\s*(?:$|[;|&)]|['"\x60]\s*(?:$|[;|&),]))`;

// JS calls whose destination argument (by index) is written. `rename` also moves its
// source away, so either argument counts.
const JS_DEST_ARG = {
  writeFile: [0],
  appendFile: [0],
  truncate: [0],
  rm: [0],
  rmdir: [0],
  unlink: [0],
  createWriteStream: [0],
  copyFile: [1],
  cp: [1],
  link: [1],
  symlink: [1],
  rename: [0, 1],
};
const JS_CALL = /\b(writeFile|appendFile|truncate|rm|rmdir|unlink|createWriteStream|copyFile|cp|link|symlink|rename|open|spawn|spawnSync|execFile|execFileSync)(?:Sync)?\s*\(/g;
const JS_WRITE_FLAG = /['"\x60](?:[wa]x?\+?|[wa]\+x?|r\+|rs\+)['"\x60]|O_(?:WRONLY|RDWR|CREAT|TRUNC|APPEND)/;
const SPAWNED_MUTATOR = /^\s*['"\x60](?:cp|mv|install|ln|rm|tee|truncate|sed|perl)['"\x60]\s*$/;

// Split a call's top-level arguments, starting just after its `(`. Quote-aware and
// bracket-balanced, so a call spread over several lines is read as one call.
function callArgs(text, start) {
  const args = [''];
  let depth = 1;
  let quote = null;
  for (let i = start; i < text.length && i < start + 8000; i++) {
    const c = text[i];
    if (quote) {
      args[args.length - 1] += c;
      if (c === '\\') args[args.length - 1] += text[++i] ?? '';
      else if (c === quote) quote = null;
      continue;
    }
    if (c === "'" || c === '"' || c === '`') quote = c;
    else if (c === '(' || c === '[' || c === '{') depth += 1;
    else if (c === ')' || c === ']' || c === '}') {
      depth -= 1;
      if (depth === 0) return args;
    } else if (c === ',' && depth === 1) {
      args.push('');
      continue;
    }
    args[args.length - 1] += c;
  }
  return args;
}

// Line numbers (1-based) of JS calls that write, move, or delete a harness config file.
function jsConfigWrites(text) {
  const lines = [];
  for (const m of text.matchAll(JS_CALL)) {
    const fn = m[1];
    const args = callArgs(text, m.index + m[0].length);
    let hit = false;
    if (JS_DEST_ARG[fn]) hit = JS_DEST_ARG[fn].some((i) => CONFIG_JS.test(args[i] ?? ''));
    else if (fn === 'open') hit = CONFIG_JS.test(args[0] ?? '') && JS_WRITE_FLAG.test(args[1] ?? '');
    else hit = SPAWNED_MUTATOR.test(args[0] ?? '') && CONFIG_JS.test(args.slice(1).join(','));
    if (hit) lines.push(text.slice(0, m.index).split('\n').length);
  }
  return lines;
}

const ARG_SEP = String.raw`['"\x60]?[\s,[]+['"\x60]?`;

// Each rule is either `re` (matched per line) or `scan` (whole text -> line numbers).
// Rules report only their label; nothing a rule matches is ever printed.
const DRIVER_RULES = [
  { label: 'harness credential file auth.json', re: /auth\.json/i },
  { label: 'harness credential file .credentials.json', re: /\.credentials\.json/i },
  { label: 'provider API key variable ANTHROPIC_API_KEY', re: /ANTHROPIC_API_KEY/i },
  { label: 'provider API key variable OPENAI_API_KEY', re: /OPENAI_API_KEY/i },
  { label: 'harness credential variable CLAUDE_CODE_OAUTH_TOKEN', re: /CLAUDE_CODE_OAUTH_TOKEN/i },
  { label: 'provider credential variable ANTHROPIC_AUTH_TOKEN', re: /ANTHROPIC_AUTH_TOKEN/i },
  { label: 'harness credential variable CODEX_API_KEY', re: /CODEX_API_KEY/i },
  { label: 'keyring access', re: /keyring/i },
  { label: 'keychain access', re: /keychain/i },
  {
    label: 'OS credential-store access',
    re: /secret-tool|libsecret|find-(?:generic|internet)-password|credential[\s_-]*manager|cmdkey/i,
  },
  // ARG_SEP lets a command pattern match a shell string and an argv array alike:
  // `herdr integration install` and ['integration', 'install'].
  { label: 'harness-config mutation `integration install`', re: new RegExp(String.raw`integration${ARG_SEP}install`, 'i') },
  {
    label: 'harness config-mutating CLI call',
    re: new RegExp(
      String.raw`\b(?:claude|codex)${ARG_SEP}(?:` +
        String.raw`mcp${ARG_SEP}(?:add[\w-]*|remove|reset[\w-]*|login|logout)` +
        String.raw`|plugins?${ARG_SEP}(?:install|uninstall|enable|disable|update|marketplace)` +
        String.raw`|config${ARG_SEP}(?:set|add|remove|unset)` +
        String.raw`|features${ARG_SEP}(?:enable|disable)` +
        String.raw`|login|logout|setup-token)\b`,
      'i',
    ),
  },
  { label: 'JS write/move/delete of a harness config file', scan: jsConfigWrites },
  {
    // cp/install/ln write their LAST operand: `cp ~/.codex/config.toml backup` is a read.
    label: 'shell copy/link onto a harness config file',
    re: new RegExp(
      String.raw`${SH_START}(?:cp|install|ln)\b(?:\s+[^\s;|&]+)*?\s+[^\s;|&]*?${CONFIG_SH}${SH_TOKEN_END}`,
      'i',
    ),
  },
  {
    label: 'shell move/delete/write of a harness config file',
    re: new RegExp(String.raw`${SH_START}(?:mv|rm|tee|truncate)\b[^;|&\n]*?${CONFIG_SH}`, 'i'),
  },
  {
    label: 'in-place edit of a harness config file',
    re: new RegExp(String.raw`${SH_START}(?:sed\s+(?:-\w*i\w*|--in-place)|perl\s+-\w*i\w*)[^|&\n]*?${CONFIG_SH}`, 'i'),
  },
  {
    // A redirect operator in shell context whose target token is the config file.
    label: 'shell redirect into a harness config file',
    re: new RegExp(String.raw`(?:^|[\s;|&(])\d?>>?\|?\s*[^\s;|&]*?${CONFIG_SH}`, 'i'),
  },
];

// --- git index access -------------------------------------------------------

// Every git index entry, with its mode: 100644/100755 regular file, 120000 symlink (the
// blob is the link target text), 160000 submodule gitlink. Paths are raw bytes in git;
// they are decoded as UTF-8 for matching and printing, and a non-UTF-8 name still keys
// its own blob by sha, so its content is never silently skipped.
function indexEntries(root) {
  const out = execFileSync('git', ['ls-files', '-s', '-z'], { cwd: root, maxBuffer: 256 * 1024 * 1024 });
  const entries = [];
  let pos = 0;
  while (pos < out.length) {
    const end = out.indexOf(0, pos);
    const rec = out.subarray(pos, end === -1 ? out.length : end);
    pos = end === -1 ? out.length : end + 1;
    if (rec.length === 0) continue;
    const tab = rec.indexOf(0x09);
    const [mode, sha] = rec.subarray(0, tab).toString('latin1').split(' ');
    entries.push({ mode, sha, path: rec.subarray(tab + 1).toString('utf8') });
  }
  return entries;
}

// sha -> Buffer for every requested blob, in one `git cat-file --batch` round trip.
function readBlobs(root, shas) {
  const unique = [...new Set(shas)];
  const blobs = new Map();
  if (unique.length === 0) return blobs;
  const out = execFileSync('git', ['cat-file', '--batch'], {
    cwd: root,
    input: `${unique.join('\n')}\n`,
    maxBuffer: 1024 * 1024 * 1024,
  });
  let pos = 0;
  for (let n = 0; n < unique.length; n++) {
    const nl = out.indexOf(0x0a, pos);
    const [sha, type, size] = out.toString('latin1', pos, nl).split(' ');
    pos = nl + 1;
    if (type === 'missing' || size === undefined) continue;
    blobs.set(sha, out.subarray(pos, pos + Number(size)));
    pos += Number(size) + 1;
  }
  return blobs;
}

// Resolve a symlink entry to a repo-relative path by following tracked symlinks hop by hop,
// including symlinked intermediate directories. Walks one segment at a time, as realpath
// does: a symlinked segment is expanded BEFORE any following `..` applies, so
// `scripts/dl/../lib` with `scripts/dl -> ../tools/herdr/lib` lands in tools/herdr/
// (#135). Returns { rel } ('' is the repository root) or { outside: true }.
function resolveLink(root, linkPath, byPath, blobs) {
  const rootPosix = resolve(root).split('\\').join('/');
  const done = [];
  let pending = linkPath.split('/');
  let hops = 0;
  while (pending.length) {
    const seg = pending.shift();
    if (seg === '' || seg === '.') continue;
    if (seg === '..') {
      if (done.length === 0) return { outside: true };
      done.pop();
      continue;
    }
    done.push(seg);
    const e = byPath.get(done.join('/'));
    if (!e || e.mode !== '120000') continue;
    if (++hops > 64) return { outside: true }; // symlink loop
    const target = (blobs.get(e.sha)?.toString('utf8') ?? '').trim().split('\\').join('/');
    done.pop();
    if (posix.isAbsolute(target) || /^[A-Za-z]:\//.test(target)) {
      if (target !== rootPosix && !target.startsWith(`${rootPosix}/`)) return { outside: true };
      done.length = 0;
      pending = [...target.slice(rootPosix.length).split('/'), ...pending];
    } else {
      pending = [...target.split('/'), ...pending];
    }
  }
  return { rel: done.join('/') };
}

// --- matching ----------------------------------------------------------------

function matchText(text, where, rules, hits) {
  const lines = text.split(/\r?\n/);
  for (const rule of rules) {
    if (rule.scan) {
      for (const n of rule.scan(text)) hits.push({ where: where(n), rule: rule.label });
      continue;
    }
    lines.forEach((line, i) => {
      if (rule.re.test(line)) hits.push({ where: where(i + 1), rule: rule.label });
    });
  }
}

// submodule path -> { url, line } from the staged .gitmodules, if any.
function readGitmodules(ctx) {
  const map = new Map();
  const e = ctx.byPath.get('.gitmodules');
  const blob = e && ctx.blobs.get(e.sha);
  if (!blob) return map;
  let path = null;
  let url = null;
  const flush = () => {
    if (path !== null && url !== null) map.set(path, url);
    path = null;
    url = null;
  };
  blob.toString('utf8').split(/\r?\n/).forEach((line, i) => {
    if (/^\s*\[/.test(line)) flush();
    const m = /^\s*(path|url)\s*=\s*(.*?)\s*$/.exec(line);
    if (m && m[1] === 'path') path = m[2];
    if (m && m[1] === 'url') url = { text: m[2], line: i + 1 };
  });
  flush();
  return map;
}

// `seen` holds the directories already entered through a directory symlink on this chain,
// so a link cycle terminates.
function scanEntry(ctx, entry, allRules, hits, { matchPath, seen = new Set() }) {
  const rel = entry.path;
  // linkOnly rules judge only where a symlink lands, never a path, blob, or target text.
  const rules = allRules.filter((r) => !r.linkOnly);
  if (matchPath) matchText(rel, () => `${rel} (tracked path)`, rules, hits);

  if (entry.mode === '160000') {
    const url = ctx.gitmodules.get(rel);
    if (url) matchText(url.text, () => `.gitmodules:${url.line} (url of submodule ${rel})`, rules, hits);
    return;
  }

  const blob = ctx.blob(entry.sha);
  if (entry.mode === '120000') {
    // Stored target text first; if that is clean, where the link really lands.
    const before = hits.length;
    matchText(blob?.toString('utf8') ?? '', () => `${rel} (symlink target)`, rules, hits);
    const landed = resolveLink(ctx.root, rel, ctx.byPath, ctx.blobs);
    if (landed.outside) {
      if (hits.length === before) hits.push({ where: `${rel} (symlink)`, rule: 'symlink resolves outside the repository' });
      return;
    }
    if (hits.length === before) matchText(landed.rel, () => `${rel} (symlink resolves to a matching repo path)`, allRules, hits);
    if (hits.length !== before) return; // the link itself is the violation
    const dest = ctx.byPath.get(landed.rel);
    if (dest) {
      const destBlob = dest.mode !== '160000' && dest.mode !== '120000' && ctx.blob(dest.sha);
      if (destBlob) matchText(destBlob.toString('utf8'), (n) => `${rel}:${n} (content via symlink)`, rules, hits);
      return;
    }
    // A directory (#135): scan every tracked entry under it as if it sat under the link --
    // path, blob, and, for a nested link, where that lands. Printed paths are the paths
    // seen through the link, never the link target.
    if (seen.has(landed.rel)) return;
    const prefix = landed.rel === '' ? '' : `${landed.rel}/`;
    const inside = ctx.entries.filter((e) => e.path.startsWith(prefix));
    ctx.preload(inside.filter((e) => e.mode !== '160000').map((e) => e.sha));
    const nextSeen = new Set(seen).add(landed.rel);
    for (const e of inside) {
      scanEntry(ctx, { ...e, path: `${rel}/${e.path.slice(prefix.length)}` }, allRules, hits, { matchPath, seen: nextSeen });
    }
    return;
  }

  if (blob) matchText(blob.toString('utf8'), (n) => `${rel}:${n}`, rules, hits);
}

function runChecks(root) {
  const results = []; // { check, target, status: 'ok'|'pending'|'fail', detail, hits }
  const entries = indexEntries(root);
  const byPath = new Map(entries.map((e) => [e.path, e]));
  const under = (dir) => entries.filter((e) => e.path.startsWith(`${dir}/`));
  const manifests = entries.filter(
    (e) => MANIFEST_NAMES.has(basename(e.path)) && !e.path.startsWith(`${DRIVER_PATH}/`),
  );

  // One batch read: everything scanned, every symlink (for resolution), and .gitmodules.
  const workflows = entries.filter((e) => isWorkflow(e.path));
  const otherWorkflows = workflows.filter((e) => e.path !== OPTIN_WORKFLOW);
  const optinWorkflow = workflows.filter((e) => e.path === OPTIN_WORKFLOW);
  const wanted = [
    ...PRODUCT_PATHS.flatMap(under),
    ...under(DRIVER_PATH),
    ...under(INTEGRATION_PATH),
    ...manifests,
    ...workflows,
    ...entries.filter((e) => e.mode === '120000' || e.path === '.gitmodules'),
  ].filter((e) => e.mode !== '160000');
  const ctx = { root, entries, byPath, blobs: readBlobs(root, wanted.map((e) => e.sha)), gitmodules: null };
  // A symlink can land on any tracked file or directory; read those blobs on demand.
  ctx.preload = (shas) => {
    for (const [k, v] of readBlobs(root, shas.filter((s) => !ctx.blobs.has(s)))) ctx.blobs.set(k, v);
  };
  ctx.blob = (sha) => {
    ctx.preload([sha]);
    return ctx.blobs.get(sha);
  };
  ctx.gitmodules = readGitmodules(ctx);

  const scanTarget = (check, target, list, rules, matchPath, pendingDetail, noun) => {
    if (list.length === 0) {
      results.push({ check, target, status: 'pending', detail: pendingDetail });
      return;
    }
    const hits = [];
    for (const e of list) scanEntry(ctx, e, typeof rules === 'function' ? rules(e) : rules, hits, { matchPath });
    results.push({
      check,
      target,
      status: hits.length ? 'fail' : 'ok',
      detail: `${list.length} ${noun}, ${hits.length} hit(s)`,
      hits,
    });
  };

  // Check 9a: herdr in product paths, and no product path reaching tests/integration/.
  const productRule = [
    { label: 'herdr reference in a product path', re: HERDR_TOKEN },
    { label: `product path references ${INTEGRATION_PATH}`, re: INTEGRATION_CLIMB_REF },
    { label: `product path symlink lands in ${INTEGRATION_PATH}`, re: landsIn(INTEGRATION_PATH), linkOnly: true },
  ];
  for (const dir of PRODUCT_PATHS) {
    scanTarget('9', `${dir}/`, under(dir), productRule, true, 'no git-tracked files (path not built yet)', 'tracked entries');
  }

  // Check 9b: manifests outside tools/herdr/ referencing tools/herdr; manifests outside
  // tests/integration/ referencing it (a default build must not compile those tests in).
  const manifestRules = (e) => [
    { label: `manifest references ${DRIVER_PATH}`, re: HERDR_PATH_REF },
    ...(e.path.startsWith(`${INTEGRATION_PATH}/`)
      ? []
      : [{ label: `manifest outside ${INTEGRATION_PATH}/ references it`, re: e.path.includes('/') ? INTEGRATION_CLIMB_REF : INTEGRATION_ROOT_REF }]),
  ];
  scanTarget(
    '9',
    `manifests outside ${DRIVER_PATH}/`,
    manifests,
    manifestRules,
    false,
    'no workspace or package manifests tracked yet',
    'manifest(s)',
  );

  // Check 9d (#146): tests/integration/ may name and spawn herdr, but never imports or
  // compiles in driver code and never symlinks into tools/herdr/.
  scanTarget(
    '9',
    `${INTEGRATION_PATH}/`,
    under(INTEGRATION_PATH),
    [
      { label: `integration test imports or compiles in driver code from ${DRIVER_PATH}`, re: DRIVER_IMPORT },
      { label: `integration test symlink lands in ${DRIVER_PATH}`, re: landsIn(DRIVER_PATH), linkOnly: true },
    ],
    true,
    'no git-tracked files (the first Stage 4/5 integration test creates it)',
    'tracked entries',
  );

  // Check 9c: workflows (K6). Only the opt-in workflow reaches the driver or the harness
  // runners, and it has no fork-reachable trigger, no secrets, and no run-block expression.
  scanTarget(
    '9',
    `workflows other than ${OPTIN_WORKFLOW}`,
    otherWorkflows,
    OTHER_WORKFLOW_RULES,
    false,
    'no other workflows tracked',
    'workflow(s)',
  );
  scanTarget('9', OPTIN_WORKFLOW, optinWorkflow, OPTIN_WORKFLOW_RULES, false, 'opt-in workflow not tracked (K6 creates it)', 'workflow');

  // Check 10: credential access / harness-config mutation in the driver.
  scanTarget('10', `${DRIVER_PATH}/`, under(DRIVER_PATH), DRIVER_RULES, true, 'no git-tracked files (K3 creates the driver)', 'tracked entries');

  return results;
}

function report(root, results) {
  const tag = { ok: 'ok     ', pending: 'PENDING', fail: 'FAIL   ' };
  console.log(`herdr containment -- oac-boundaries checks 9 and 10 (root: ${root})`);
  for (const r of results) {
    console.log(`  ${tag[r.status]}  check ${r.check.padEnd(2)}  ${r.target}: ${r.detail}`);
    for (const h of r.hits ?? []) console.log(`             FAIL  ${h.where}: ${h.rule}`);
  }
  const fails = results.filter((r) => r.status === 'fail');
  const pending = results.filter((r) => r.status === 'pending');
  const hitCount = fails.reduce((n, r) => n + r.hits.length, 0);
  if (fails.length) {
    console.log(`\nResult: FAIL -- ${hitCount} violation(s) in ${fails.length} target(s). Stop and cite the boundary (oac-boundaries).`);
    return 1;
  }
  if (pending.length) {
    console.log(
      `\nResult: PENDING -- 0 violations; ${pending.length} of ${results.length} target(s) not built yet. ` +
        'Pending is not a pass: re-run once those paths exist.',
    );
    return 0;
  }
  console.log(`\nResult: CLEAN -- 0 violations across all ${results.length} targets.`);
  return 0;
}

// --- self-test -------------------------------------------------------------

// Each case builds a throwaway git work tree, runs this script against it with --root, and
// asserts the exit code and the result line. Violation cases plant exactly one hit.
// `links` / `gitlinks` plant tracked symlinks / submodules straight into the git index (no
// OS symlink support needed); `afterAdd` rewrites work-tree files after staging (the index
// is what counts); `rawFiles` writes byte-exact (e.g. non-UTF-8) file names; `mustNotPrint`
// lists strings that must never appear in the script's output.
const OPTIN_CLEAN = [
  'name: herdr-provider-optin',
  'on:',
  '  workflow_dispatch:',
  '    inputs:',
  '      scenario:',
  '        type: choice',
  '        options: [smoke]',
  '  push:',
  '    branches: [main]',
  '    paths: [docs/planning/PINS.md]',
  'permissions:',
  '  contents: read',
  'jobs:',
  '  herdr:',
  '    name: herdr ${{ matrix.os }}',
  '    runs-on: [self-hosted, oac-harness, linux]',
  '    steps:',
  '      - uses: actions/checkout@1111111111111111111111111111111111111111 # v7.0.1',
  '        with:',
  '          persist-credentials: false',
  '      - name: Run',
  '        env:',
  '          OAC_HERDR_SCENARIO: ${{ inputs.scenario }}',
  '        run: node tools/herdr/ci.mjs run',
  '      - uses: actions/upload-artifact@0000000000000000000000000000000000000000',
  '        with:',
  '          path: ${{ steps.stage.outputs.upload-dir }}',
  '      - run: |',
  '          node tools/herdr/ci.mjs cleanup',
  '        if: ${{ always() }}',
  '',
].join('\n');
const optin = (insert, after = '  push:') => OPTIN_CLEAN.replace(`${after}\n`, `${after}\n${insert}\n`);

const CLEAN_BASE = {
  'adapters/claude/src/lib.rs': '// adapter\n',
  'core/src/lib.rs': '// core\n',
  'cli/src/main.rs': 'fn main() {}\n',
  'transports/zenoh/src/lib.rs': '// transport\n',
  'spec/session-channels.md': '# Session Channels\n',
  'tools/herdr/run.mjs': "// driver\nimport { spawnSync } from 'node:child_process';\n",
  'tools/herdr/lib/util.mjs': '// util\n',
  // Allowed in the driver: reading, hashing, or backing up harness config; writing herdr's
  // own config; launching a harness. Present in every case, so every case also proves
  // check 10 raises no false positive on these lines.
  'tools/herdr/lib/allowed.mjs': [
    "const before = sha256(readFileSync(join(home, '.claude', 'settings.json')));",
    "const hashes = paths.map((p) => sha256(readFileSync(join(home, '.codex', 'config.toml'))));",
    "if (n > 0) h = sha256(readFileSync(join(home, '.claude', 'settings.json')));",
    "const linked = readFileSync(join(home, '.codex', 'config.toml'));",
    "copyFileSync(join(home, '.claude', 'settings.json'), join(scratch, 'settings.before.json'));",
    "const fd = openSync(join(home, '.codex', 'config.toml'), 'r');",
    "writeFileSync(join(scratch, 'herdr', 'config.toml'), herdrConfig);",
    "spawnSync('claude', ['--channels', 'plugin:oac@local']);",
    "spawnSync('codex', ['app-server']);",
    '',
  ].join('\n'),
  'tools/herdr/lib/allowed.sh': [
    'cp ~/.codex/config.toml "$SCRATCH/config.before.toml"',
    'sha256sum ~/.claude/settings.json > "$SCRATCH/hashes.txt"',
    'test "$n" > 0 && cat "$CODEX_HOME/config.toml"',
    '',
  ].join('\n'),
  // A manifest inside tools/herdr/ may name its own path.
  'tools/herdr/scenarios.json': '{ "root": "tools/herdr" }\n',
  'Cargo.toml': '[workspace]\nmembers = ["core", "cli"]\n',
  // A crate's own tests/ directory is not the top-level tests/integration/ (#146).
  'adapters/claude/Cargo.toml': '[package]\nname = "oac-adapter-claude"\n\n[[test]]\nname = "integration"\npath = "tests/integration.rs"\n',
  'adapters/claude/tests/integration.rs': '// crate-local integration test\n',
  // tests/integration/ may name herdr and spawn the driver by path (README reuse contract).
  'tests/integration/g4-claude.mjs': [
    '// Opt-in, herdr-driven provider-integration test (HERDR_SESSION is set by the driver).',
    "spawnSync(process.execPath, ['tools/herdr/run.mjs', '--scenario', 'tests/integration/scenarios/g4.mjs']);",
    "const manifest = JSON.parse(readFileSync(join(runDir, 'run-manifest.json'), 'utf8'));",
    '',
  ].join('\n'),
  // herdr is allowed outside product paths (docs, scripts, the driver itself).
  'docs/planning/herdr-notes.md': 'herdr is test tooling; HERDR_SESSION is set by the driver.\n',
  // Workflows: a default one that runs this lint, and a clean opt-in one. Expressions in
  // env:/with:/name: are allowed; only run: blocks are checked.
  '.github/workflows/lint.yml': [
    'on: [push]',
    'jobs:',
    '  lint:',
    '    runs-on: ubuntu-latest',
    '    steps:',
    '      - run: node scripts/check-herdr-containment.mjs',
    '',
  ].join('\n'),
  '.github/workflows/herdr-provider-optin.yml': OPTIN_CLEAN,
};

function violation(name, path, content, extra = {}) {
  return { name, expect: 'fail', files: { ...CLEAN_BASE, ...(path ? { [path]: content } : {}) }, ...extra };
}

const SECRET = 'sk-ant-api03-TOPSECRETVALUE';

const SELF_TEST_CASES = [
  // Check 9: one per product path, plus the HERDR_ spelling.
  violation('9 herdr in adapters/', 'adapters/claude/src/lib.rs', 'use herdr_client;\n'),
  violation('9 HERDR_ in core/', 'core/src/lib.rs', 'const S: &str = "HERDR_SESSION";\n'),
  violation('9 herdr in cli/', 'cli/src/main.rs', '// spawn herdr for tests\nfn main() {}\n'),
  violation('9 herdr in transports/', 'transports/zenoh/src/lib.rs', 'mod Herdr;\n'),
  violation('9 herdr in spec/', 'spec/session-channels.md', 'Harnesses MAY be driven by herdr.\n'),
  // Check 9: entries that are not a regular file's content.
  violation('9 herdr in a tracked file name', 'adapters/claude/src/herdr_bridge.rs', '// bridge\n'),
  violation('9 symlink into tools/herdr', null, null, { links: { 'adapters/claude/vendor': '../../tools/herdr/lib' } }),
  violation('9 two-hop symlink chain into tools/herdr', null, null, {
    links: { 'scripts/dl': '../tools/herdr/lib', 'adapters/claude/vendor': '../../scripts/dl' },
  }),
  violation('9 symlink through a symlinked directory', null, null, {
    links: { 'scripts/dl': '../tools/herdr/lib', 'adapters/claude/util.mjs': '../../scripts/dl/util.mjs' },
  }),
  violation('9 symlink to a file whose content names herdr', 'scripts/helper.rs', 'use herdr;\n', {
    links: { 'adapters/claude/src/extra.rs': '../../../scripts/helper.rs' },
  }),
  violation('9 symlink escaping the repository', null, null, {
    links: { 'adapters/claude/sys': '../../../outside/lib' },
  }),
  // #135 gap 1: `..` after a symlinked segment applies to where that symlink lands.
  violation('9 symlink target with `..` after a symlinked directory', null, null, {
    links: { 'scripts/dl': '../tools/herdr/lib', 'adapters/claude/v': '../../scripts/dl/../lib' },
  }),
  // #135 gap 2: a symlink to a directory scans the tracked entries under it.
  violation('9 symlink to a directory whose file names herdr', 'scripts/drv/run.mjs', "spawnSync('herdr', ['agent', 'read']);\n", {
    links: { 'adapters/claude/drv': '../../scripts/drv' },
  }),
  violation('9 symlink to a directory holding a herdr-named file', 'scripts/drv2/herdr_shim.rs', '// shim\n', {
    links: { 'adapters/claude/drv': '../../scripts/drv2' },
  }),
  violation('9 symlink cycle through a directory link terminates', 'scripts/loop/x.rs', 'use herdr;\n', {
    links: { 'adapters/claude/loop': '../../scripts/loop', 'scripts/loop/back': '../../adapters/claude' },
  }),
  violation('9 submodule named herdr', null, null, { gitlinks: ['adapters/herdr'] }),
  violation(
    '9 submodule whose url names herdr',
    '.gitmodules',
    '[submodule "bridge"]\n\tpath = adapters/vendor/bridge\n\turl = https://example.invalid/herdr-bridge.git\n',
    { gitlinks: ['adapters/vendor/bridge'] },
  ),
  violation('9 non-UTF-8 file name', null, null, {
    rawFiles: [[Buffer.from('core/caf\xe9.rs', 'latin1'), 'use herdr;\n']],
  }),
  violation('9 staged herdr, work tree reverted', 'core/src/lib.rs', 'use herdr;\n', {
    afterAdd: { 'core/src/lib.rs': '// core\n' },
  }),
  // Check 9: manifests outside tools/herdr/ referencing tools/herdr.
  violation('9 Cargo.toml references tools/herdr', 'Cargo.toml', '[workspace]\nmembers = ["core", "tools/herdr"]\n'),
  violation('9 package.json references tools/herdr', 'package.json', '{ "workspaces": ["tools/herdr"] }\n'),
  violation('9 nested manifest references tools\\herdr', 'tools/other/pyproject.toml', 'path = "..\\\\tools\\\\herdr"\n'),
  // Check 9 (#146): tests/integration/ stays a leaf -- nothing in a product path or a default
  // build reaches it, and it never pulls driver code in.
  violation('9 product path compiles in a tests/integration file', 'adapters/claude/src/lib.rs',
    '#[path = "../../../tests/integration/common.rs"]\nmod common;\n'),
  violation('9 product path symlink chain lands in tests/integration', null, null, {
    links: { 'scripts/it': '../tests/integration', 'core/it': '../scripts/it' },
  }),
  violation('9 root workspace compiles tests/integration in', 'Cargo.toml', '[workspace]\nmembers = ["core", "cli", "tests/integration"]\n'),
  violation('9 nested manifest depends on tests/integration', 'tools/other/package.json', '{ "devDependencies": { "it": "file:../../tests/integration" } }\n'),
  violation('9 integration test imports driver code (JS)', 'tests/integration/g4-claude.mjs', "import { drive } from '../../tools/herdr/lib/drive.mjs';\n"),
  violation('9 integration test requires driver code (JS)', 'tests/integration/g4-claude.mjs', "const lib = require('../../tools/herdr/lib/util.mjs');\n"),
  violation('9 integration test compiles in driver code (Rust)', 'tests/integration/src/lib.rs',
    'include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tools/herdr/lib/x.rs"));\n'),
  violation('9 integration test symlink lands in tools/herdr', null, null, {
    links: { 'tests/integration/drv': '../../tools/herdr/lib' },
  }),
  // Check 10: credentials.
  violation('10 auth.json', 'tools/herdr/run.mjs', "readFileSync(join(home, '.codex', 'auth.json'));\n"),
  violation('10 .credentials.json', 'tools/herdr/run.mjs', "const f = '~/.claude/.credentials.json';\n"),
  violation('10 ANTHROPIC_API_KEY', 'tools/herdr/lib/env.mjs', 'const k = process.env.ANTHROPIC_API_KEY;\n'),
  violation('10 OPENAI_API_KEY', 'tools/herdr/lib/env.mjs', "env['OPENAI_API_KEY'] = token;\n"),
  violation('10 CLAUDE_CODE_OAUTH_TOKEN', 'tools/herdr/lib/env.mjs', 'env.CLAUDE_CODE_OAUTH_TOKEN = t;\n'),
  violation('10 ANTHROPIC_AUTH_TOKEN (secret never printed)', 'tools/herdr/lib/env.mjs', `const t = process.env.ANTHROPIC_AUTH_TOKEN || '${SECRET}';\n`, {
    mustNotPrint: [SECRET],
  }),
  violation('10 CODEX_API_KEY', 'tools/herdr/lib/env.mjs', 'export CODEX_API_KEY="$KEY"\n'),
  violation('10 keyring', 'tools/herdr/lib/auth.mjs', "import keyring from 'keyring';\n"),
  violation('10 keychain', 'tools/herdr/lib/auth.mjs', '// read the macOS Keychain entry\n'),
  violation('10 OS credential store', 'tools/herdr/lib/auth.mjs', "spawnSync('secret-tool', ['lookup']);\n"),
  violation('10 credential file by tracked name', 'tools/herdr/fixtures/auth.json', '{}\n'),
  violation('10 symlink to a credential file (target never printed)', null, null, {
    links: { 'tools/herdr/fixtures/creds': '/home/operator/.codex/auth.json' },
    mustNotPrint: ['/home/operator'],
  }),
  violation('10 symlink escaping the repository', null, null, {
    links: { 'tools/herdr/fixtures/home': '/home/operator' },
    mustNotPrint: ['/home/operator'],
  }),
  // Check 10: harness config mutation via CLI.
  violation('10 integration install (shell)', 'tools/herdr/setup.sh', 'herdr integration install claude\n'),
  violation('10 integration install (argv)', 'tools/herdr/run.mjs', "spawnSync('herdr', ['integration', 'install']);\n"),
  violation('10 claude mcp add (argv)', 'tools/herdr/run.mjs', "execFileSync('claude', ['mcp', 'add', 'oac', '--', 'node', 'x.mjs']);\n"),
  violation('10 codex mcp remove (shell)', 'tools/herdr/setup.sh', 'codex mcp remove oac\n'),
  violation('10 claude plugin install (shell)', 'tools/herdr/setup.sh', 'claude plugin install oac@local\n'),
  violation('10 claude config set (shell)', 'tools/herdr/setup.sh', 'claude config set -g theme dark\n'),
  // Check 10: harness config writes.
  violation('10 sed -i on settings.json (secret never printed)', 'tools/herdr/setup.sh',
    `sed -i 's/"apiKeyHelper": ".*"/"apiKeyHelper": "echo ${SECRET}"/' ~/.claude/settings.json\n`, {
      mustNotPrint: [SECRET, 'apiKeyHelper'],
    }),
  violation('10 writeFileSync settings.json', 'tools/herdr/run.mjs', "writeFileSync(join(home, '.claude', 'settings.json'), JSON.stringify(s));\n"),
  violation('10 multi-line writeFileSync settings.json', 'tools/herdr/run.mjs',
    "writeFileSync(\n  join(home, '.claude', 'settings.json'),\n  JSON.stringify(s),\n);\n"),
  violation('10 openSync config.toml for write', 'tools/herdr/run.mjs', "const fd = openSync(join(codexHome, '.codex', 'config.toml'), 'w');\n"),
  violation('10 copyFileSync onto hooks.json', 'tools/herdr/run.mjs', "copyFileSync(src, join(home, '.codex', 'hooks.json'));\n"),
  violation('10 spawned cp onto config.toml (argv)', 'tools/herdr/run.mjs', "execFileSync('cp', [src, join(home, '.codex', 'config.toml')]);\n"),
  violation('10 shell cp onto settings.json', 'tools/herdr/setup.sh', 'cp "$SCRATCH/settings.json" ~/.claude/settings.json\n'),
  violation('10 shell mv of config.toml', 'tools/herdr/setup.sh', 'mv "$CODEX_HOME/config.toml" /tmp/x\n'),
  violation('10 shell install onto hooks.json', 'tools/herdr/setup.sh', 'install -m 644 hooks.src "$CODEX_HOME"/hooks.json\n'),
  violation('10 append to config.toml', 'tools/herdr/setup.sh', "echo '[mcp_servers.oac]' >> ~/.codex/config.toml\n"),
  // Check 9: workflows (K6).
  violation('9 other workflow references tools/herdr', '.github/workflows/lint.yml', 'on: [push]\njobs:\n  a:\n    steps:\n      - run: node tools/herdr/run.mjs --scenario smoke\n'),
  violation('9 other workflow targets the oac-harness label', '.github/workflows/extra.yaml', 'jobs:\n  a:\n    runs-on: [oac-harness]\n'),
  violation('9 other workflow targets a self-hosted runner', '.github/workflows/extra.yml', 'jobs:\n  a:\n    runs-on: self-hosted\n'),
  violation('9 opt-in workflow gains a PR-event trigger', '.github/workflows/herdr-provider-optin.yml', optin('  pull_request_target:', 'on:')),
  violation('9 opt-in workflow gains a chained trigger', '.github/workflows/herdr-provider-optin.yml', optin('  workflow_run:', 'on:')),
  violation('9 opt-in workflow gains a schedule', '.github/workflows/herdr-provider-optin.yml', optin('  schedule:', 'on:')),
  violation('9 opt-in workflow reads a secret', '.github/workflows/herdr-provider-optin.yml', optin('          KEY: ${{ secrets.KEY }}', '        env:')),
  violation('9 opt-in workflow: expression in an inline run', '.github/workflows/herdr-provider-optin.yml',
    OPTIN_CLEAN.replace('run: node tools/herdr/ci.mjs run', 'run: node tools/herdr/ci.mjs run ${{ inputs.scenario }}')),
  violation('9 opt-in workflow: expression in a block run', '.github/workflows/herdr-provider-optin.yml',
    OPTIN_CLEAN.replace('          node tools/herdr/ci.mjs cleanup', '          node tools/herdr/ci.mjs cleanup\n          echo "${{ github.event.head_commit.message }}"')),
  violation('9 opt-in workflow calls the driver directly', '.github/workflows/herdr-provider-optin.yml',
    OPTIN_CLEAN.replace('node tools/herdr/ci.mjs cleanup', 'node tools/herdr/run.mjs --scenario smoke')),
  violation('9 opt-in workflow passes accept=driver', '.github/workflows/herdr-provider-optin.yml',
    OPTIN_CLEAN.replace('OAC_HERDR_SCENARIO: ${{ inputs.scenario }}', 'OAC_HERDR_PARAMS: accept=driver')),
  // Triggers are an allowlist: any event a stranger can fire (open an issue, star, fork,
  // start a discussion) fails, not only the named PR events.
  violation('9 opt-in workflow gains an issues trigger', '.github/workflows/herdr-provider-optin.yml', optin('  issues:', 'on:')),
  violation('9 opt-in workflow gains a watch trigger', '.github/workflows/herdr-provider-optin.yml', optin('  watch:', 'on:')),
  violation('9 opt-in workflow gains a fork trigger', '.github/workflows/herdr-provider-optin.yml', optin('  fork:', 'on:')),
  violation('9 opt-in workflow gains a discussion trigger', '.github/workflows/herdr-provider-optin.yml', optin('  discussion:\n    types: [created]', 'on:')),
  violation('9 opt-in workflow trigger list form with an extra event', '.github/workflows/herdr-provider-optin.yml',
    OPTIN_CLEAN.replace(/^on:\n[\s\S]*?(?=^permissions:)/m, 'on: [workflow_dispatch, issues]\n')),
  violation('9 opt-in workflow trigger list form: unfiltered push', '.github/workflows/herdr-provider-optin.yml',
    OPTIN_CLEAN.replace(/^on:\n[\s\S]*?(?=^permissions:)/m, 'on: [push, workflow_dispatch]\n')),
  violation('9 opt-in workflow push widened to another branch', '.github/workflows/herdr-provider-optin.yml', OPTIN_CLEAN.replace('branches: [main]', 'branches: [main, dev]')),
  violation('9 opt-in workflow push gains tags', '.github/workflows/herdr-provider-optin.yml', optin('    tags: [v*]', '  push:')),
  violation('9 opt-in workflow reads toJSON(secrets)', '.github/workflows/herdr-provider-optin.yml', optin('          ALL: ${{ toJSON(secrets) }}', '        env:')),
  violation('9 opt-in workflow reads github.token', '.github/workflows/herdr-provider-optin.yml', optin('          T: ${{ github.token }}', '        env:')),
  violation('9 opt-in workflow sets contents: write', '.github/workflows/herdr-provider-optin.yml', OPTIN_CLEAN.replace('  contents: read', '  contents: write')),
  violation('9 opt-in workflow adds id-token: write', '.github/workflows/herdr-provider-optin.yml', optin('  id-token: write', 'permissions:')),
  violation('9 opt-in workflow uses write-all', '.github/workflows/herdr-provider-optin.yml', OPTIN_CLEAN.replace('permissions:\n  contents: read', 'permissions: write-all')),
  violation('9 opt-in workflow adds job-level permissions', '.github/workflows/herdr-provider-optin.yml', optin('    permissions:\n      contents: write', '  herdr:')),
  violation('9 opt-in workflow pins checkout by a tag', '.github/workflows/herdr-provider-optin.yml', OPTIN_CLEAN.replace('actions/checkout@1111111111111111111111111111111111111111', 'actions/checkout@v7')),
  violation('9 opt-in workflow pins upload-artifact by a branch', '.github/workflows/herdr-provider-optin.yml', OPTIN_CLEAN.replace('actions/upload-artifact@0000000000000000000000000000000000000000', 'actions/upload-artifact@main')),
  violation('9 opt-in workflow uses a third-party action', '.github/workflows/herdr-provider-optin.yml', optin('      - uses: someone/setup@2222222222222222222222222222222222222222', '    steps:')),
  violation('9 opt-in workflow checkout persists the token', '.github/workflows/herdr-provider-optin.yml', OPTIN_CLEAN.replace('          persist-credentials: false\n', '          fetch-depth: 1\n')),
  // Other workflows: a bare default label routes to a self-hosted runner that carries it.
  violation('9 other workflow runs on bare default labels', '.github/workflows/extra.yml', 'jobs:\n  a:\n    runs-on: [linux, x64]\n'),
  violation('9 other workflow matrix names a bare windows label', '.github/workflows/extra.yml',
    'jobs:\n  a:\n    strategy:\n      matrix:\n        os: [ubuntu-latest, "windows"]\n    runs-on: ${{ matrix.os }}\n'),
  // Controls: these must NOT fail.
  { name: 'control: empty tree reports PENDING', expect: 'pending', files: { 'README.md': '# empty\n' } },
  { name: 'control: clean full tree reports CLEAN', expect: 'clean', files: CLEAN_BASE },
  {
    name: 'control: other workflow on GitHub-hosted labels is clean',
    expect: 'clean',
    files: {
      ...CLEAN_BASE,
      '.github/workflows/matrix.yml': 'jobs:\n  a:\n    strategy:\n      matrix:\n        os: [ubuntu-latest, windows-latest, macos-15, ubuntu-24.04-arm]\n    runs-on: ${{ matrix.os }}\n',
    },
  },
  {
    // #135 gap 3: a driver's own scratch file merely ending in hooks.json, or a hooks.json
    // outside a harness config directory, is not harness config.
    name: 'control: driver scratch hooks.json files are not harness config',
    expect: 'clean',
    files: {
      ...CLEAN_BASE,
      'tools/herdr/lib/scratch.mjs': [
        "writeFileSync(join(runDir, 'hooks.json.sha256'), h);",
        "writeFileSync(join(runDir, 'hooks.json'), JSON.stringify(hooks));",
        "copyFileSync(join(home, '.codex', 'hooks.json'), join(runDir, 'hooks.json.before'));",
        '',
      ].join('\n'),
      'tools/herdr/lib/scratch.sh': ['sha256sum "$CODEX_HOME"/hooks.json > "$RUN"/hooks.json.sha256', 'cp hooks.src "$RUN"/hooks.json', ''].join('\n'),
    },
  },
  {
    name: 'control: untracked herdr file is ignored',
    expect: 'clean',
    files: CLEAN_BASE,
    afterAdd: { 'adapters/claude/scratch.rs': 'herdr\n' },
  },
  {
    name: 'control: clean symlinks and submodule in a product path',
    expect: 'clean',
    files: CLEAN_BASE,
    links: { 'adapters/claude/src/alias.rs': 'lib.rs', 'adapters/claude/core': '../../core/src' },
    gitlinks: ['adapters/vendor/dep'],
  },
];

function writeTree(dir, files) {
  for (const [rel, content] of Object.entries(files)) {
    const abs = join(dir, rel);
    mkdirSync(dirname(abs), { recursive: true });
    writeFileSync(abs, content);
  }
}

function plantIndexEntries(dir, tc) {
  const git = (args, input) => execFileSync('git', args, { cwd: dir, encoding: 'utf8', input }).trim();
  for (const [rel, target] of Object.entries(tc.links ?? {})) {
    const sha = git(['hash-object', '-w', '--stdin'], target);
    git(['update-index', '--add', '--cacheinfo', `120000,${sha},${rel}`]);
  }
  for (const rel of tc.gitlinks ?? []) {
    git(['update-index', '--add', '--cacheinfo', `160000,${'1'.repeat(40)},${rel}`]);
  }
}

// false when the file system rejects the byte-exact name (e.g. non-UTF-8 on some systems).
function writeRawFiles(dir, rawFiles) {
  try {
    for (const [relBuf, content] of rawFiles ?? []) {
      const abs = Buffer.concat([Buffer.from(`${dir}/`), relBuf]);
      mkdirSync(dirname(join(dir, relBuf.toString('latin1'))), { recursive: true });
      writeFileSync(abs, content);
    }
    return true;
  } catch {
    return false;
  }
}

function runSelfTest() {
  let failed = 0;
  let skipped = 0;
  for (const tc of SELF_TEST_CASES) {
    const dir = mkdtempSync(join(tmpdir(), 'oac-herdr-selftest-'));
    try {
      execFileSync('git', ['init', '-q'], { cwd: dir });
      writeTree(dir, tc.files);
      if (!writeRawFiles(dir, tc.rawFiles)) {
        skipped += 1;
        console.log(`  skip  ${tc.name}  (this file system rejects the byte-exact file name)`);
        continue;
      }
      execFileSync('git', ['add', '-A'], { cwd: dir });
      plantIndexEntries(dir, tc);
      if (tc.afterAdd) writeTree(dir, tc.afterAdd);
      const run = spawnSync(process.execPath, [scriptPath, '--root', dir], { encoding: 'utf8' });
      const output = run.stdout + run.stderr;
      const resultLine = (run.stdout.match(/^Result: (\w+)/m) ?? [])[1];
      const hitLines = (run.stdout.match(/^ {13}FAIL {2}/gm) ?? []).length;
      const leaked = (tc.mustNotPrint ?? []).filter((s) => output.includes(s));
      const ok =
        leaked.length === 0 &&
        (tc.expect === 'fail'
          ? run.status === 1 && resultLine === 'FAIL' && hitLines === 1
          : run.status === 0 && resultLine === tc.expect.toUpperCase());
      if (!ok) failed += 1;
      const leakNote = tc.mustNotPrint ? `, ${leaked.length ? `LEAKED ${leaked.length} secret string(s)` : 'nothing leaked'}` : '';
      console.log(`  ${ok ? 'ok  ' : 'FAIL'}  ${tc.name}  (exit ${run.status}, Result: ${resultLine ?? 'none'}${leakNote})`);
      if (!ok && leaked.length === 0) console.log(output);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  }
  const total = SELF_TEST_CASES.length;
  const violations = SELF_TEST_CASES.filter((tc) => tc.expect === 'fail').length;
  console.log(
    `\nself-test: ${total - failed - skipped}/${total} cases as expected` +
      (skipped ? `, ${skipped} skipped` : '') +
      ` (${violations} planted violations must exit 1 with exactly one hit; ` +
      `${total - violations} controls must exit 0; no case may print its secret strings).`,
  );
  return failed ? 1 : 0;
}

// --- main ------------------------------------------------------------------

const argv = process.argv.slice(2);
if (argv.includes('--self-test')) {
  process.exit(runSelfTest());
}

let root = join(dirname(scriptPath), '..');
const rootIdx = argv.indexOf('--root');
if (rootIdx !== -1) {
  if (!argv[rootIdx + 1]) {
    console.error('usage: node scripts/check-herdr-containment.mjs [--root <dir>] [--self-test]');
    process.exit(2);
  }
  root = resolve(argv[rootIdx + 1]);
}

let results;
try {
  results = runChecks(root);
} catch (err) {
  console.error(`Could not read the git index under ${root}: ${err.message}`);
  process.exit(2);
}
process.exit(report(root, results));
