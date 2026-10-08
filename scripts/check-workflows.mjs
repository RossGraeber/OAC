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
//   W0  the structural reader can read it (fails closed): block and flow mappings and
//       sequences, quoted and plain keys and scalars, block scalars. Anchors (`&a`),
//       aliases (`*a`) and merge keys (`<<`) are refused outright (PR #336 review B4): an
//       alias would carry text a rule read only where it was anchored.
//   W1  every `uses:` (block or flow style, the key quoted or not) names an action at a full
//       40-hex commit SHA (no tag, no branch, no docker:// image). A local action (`./dir`)
//       is read and held to these same rules, and so is every local action it uses in turn,
//       wherever it lives (#332); one whose action file is missing fails. A local reusable
//       workflow (`./.github/workflows/x.yml`) is checked as a workflow in its own right.
//   W2  a top-level `permissions:` that is exactly `contents: read` (block or flow), and
//       every job's `permissions:` (`jobs.<id>.permissions`, the job written in block or
//       flow style, #332) holds only `read` or `none`, or is `read-all` or `{}`. A key
//       named `permissions` anywhere else (an action input under `with:`) is not a job's.
//   W3  every actions/checkout step sets `persist-credentials: false` under its `with:`;
//   W4  no secrets context and no job token: every `${{ ... }}` expression is read from the
//       raw text, comments and run: block scalars included (GitHub evaluates an expression
//       inside a block scalar, a heredoc `#` line too), joined across line breaks, and
//       fails on `secrets` in any form, `github.token`, `github[...]` (an index such as
//       `github['token']`) and the whole `github` context (`toJSON(github)`, `${{ github }}`);
//       an `if:` written without `${{ }}` is an expression too and is read the same way
//       (#332); `secrets: inherit` fails; no provider or harness credential name; no
//       `pull_request_target` or `workflow_run` trigger.
//   W5  no script injection (#332): inside a `run:` script, an actions/github-script
//       `script:` input or a `shell:` (defence in depth), where GitHub pastes an
//       expression's text in before it runs, an expression may read only an allowlist
//       (PR #336 re-review R3): `runner.*`, `github.sha`, `github.workspace`,
//       `github.run_id`, `github.run_number`, `github.run_attempt`, `github.event_name`,
//       `steps.<id>.outcome` and `.conclusion`, `strategy.job-index` and `.job-total`,
//       literals, and the built-in functions. Every other expression is refused, `matrix.*`
//       (a matrix value can be an expression over untrusted text, PR #336 third review S2),
//       `env.*`, `steps.*.outputs.*`, `inputs.*`, `vars.*`, `needs.*` and `github.event...`
//       included, and so is any index (`x[..]`): a value reaches a script through `env:`
//       and a shell variable (`"$X"`), never pasted in. Step outputs are refused rather
//       than tracked for taint.
// Default-tier workflows (any trigger other than workflow_dispatch: a `schedule` runs
// unattended, with no opt-in, so it is the default tier too, oac-testing section 2; the
// herdr opt-in workflow is the exception, with its own stricter rules in
// scripts/check-herdr-containment.mjs check 9), and the local actions they use:
//   D1  no self-hosted runner: `self-hosted` anywhere outside a comment, so a runner label
//       routed through a matrix fails too (check 9 also refuses every runner label in any
//       workflow but the herdr one);
//   D2  no opt-in switch: no `--ignored` / `--include-ignored` test run, no OAC_TEST_*
//       opt-in variable, no tools/herdr driver. One exception (#345): a `run:` line that is
//       exactly `[$LOOPBACK_ONLY ]node tools/herdr/run.mjs --self-test`, the driver's
//       offline self-test against its test doubles (no harness; check 9 holds the same line),
//       and only when the structural reader's whole `run:` value is exactly that command
//       (a folded continuation, or a block, quoted or flow form, fails: PR #349 B1), in a
//       step with no env:, working-directory:, second run: or shell other than bash, under
//       no job env:/defaults:/container:/services:, workflow defaults: or non-CARGO_*
//       workflow env: (#353). Out of reach of any static check: an earlier step writing
//       NODE_OPTIONS or LOOPBACK_ONLY to $GITHUB_ENV (ci.yml itself sets LOOPBACK_ONLY so);
//   D3  no harness CLI install (the Claude Code or Codex npm packages, `codex`/`claude`
//       installers).
// Opt-in workflows (workflow_dispatch only) may use D1-D3; W0-W5 still hold.
//
// Two readers, both failing closed. The structural one parses the YAML into mappings,
// sequences and scalars for W0-W3, the `if:` part of W4 and W5; a workflow whose `on:` or
// top-level `permissions:` it cannot find is a violation. The line one reads the text for
// W1 too (so a `uses:` either reader sees is checked), and for W4 and D1-D3: YAML comments
// are stripped before W1, D1-D3 and the credential-name match, so a header saying "no
// secrets" is not a hit; the text of a block scalar (a run: script) is never treated as a
// comment. W4's expression rules read the raw text, so an expression inside a YAML comment
// fails too (GitHub would not evaluate it there; failing on it is the safe side).
// Tags (`!!str`) are read as plain text. The reader is linear in the length of a line.
// Node built-ins only.
// Exit codes: 0 = clean; 1 = violation (or failed self-test); 2 = usage or environment error.

import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { dirname, join, posix, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const scriptPath = fileURLToPath(import.meta.url);
const repoRoot = resolve(dirname(scriptPath), '..');

// Governed by check 9 (scripts/check-herdr-containment.mjs): push-to-main on PINS.md and
// dispatch, self-hosted harness runners, no opt-in flag beyond its scenario input.
const HERDR_OPTIN = 'herdr-provider-optin.yml';
// The trigger of an opt-in workflow: a person starts it. A `schedule` is not one (PR #336
// review B5): it runs unattended, so a scheduled workflow is held to the default tier.
const OPT_IN_TRIGGERS = new Set(['workflow_dispatch']);

const SHA_PIN = /^[\w.-]+\/[\w.-]+(?:\/[\w./-]+)?@[0-9a-f]{40}$/;
const CREDENTIAL = /ANTHROPIC_API_KEY|ANTHROPIC_AUTH_TOKEN|CLAUDE_CODE_OAUTH_TOKEN|OPENAI_API_KEY|CODEX_API_KEY|OPENAI_ORG|\bGITHUB_TOKEN\b|\bauth\.json\b|\.credentials\.json/i;
const EXPRESSION_RULES = [
  { re: /\bsecrets\b/i, msg: 'secrets context in an expression' },
  { re: /\bgithub\s*\.\s*token\b/i, msg: 'github.token in an expression' },
  { re: /\bgithub\s*\[/i, msg: 'github context indexed (github[...]) in an expression' },
  { re: /\bgithub\b(?!\s*[.[])/i, msg: 'whole github context (it holds the token) in an expression' },
];
// W5: the only expressions that may be pasted into code a step runs (PR #336 re-review R3).
// An allowlist of values no outsider or caller can choose; everything else, `env.*` and
// `steps.*.outputs.*` included, reaches a script through `env:` and a shell variable.
// Not `matrix.*`: a matrix value can itself be an expression over untrusted text
// (`t: ["${{ github.event.issue.title }}"]`, `matrix: ${{ fromJSON(needs.x.outputs.m) }}`),
// which GitHub evaluates and then pastes (PR #336 third review S2).
const SAFE_IN_SCRIPT = [
  /^runner(?:\.[\w-]+)*$/i,
  /^github\.(?:sha|workspace|run_id|run_number|run_attempt|event_name)$/i,
  /^steps\.[\w-]+\.(?:outcome|conclusion)$/i,
  /^strategy\.(?:job-index|job-total)$/i,
  /^(?:true|false|null)$/i,
];
const SAFE_FUNCTIONS = new Set(['contains', 'startswith', 'endswith', 'format', 'join', 'tojson', 'fromjson', 'hashfiles', 'success', 'always', 'cancelled', 'failure']);

// Why `expr` (the text inside `${{ }}`) may not be pasted into a script, or null.
function unsafeInScript(expr) {
  // String literals are data: drop them ('' is an escaped quote inside one).
  const bare = expr.replace(/'(?:[^']|'')*'/g, "''");
  if (/[[\]]/.test(bare)) return 'an index (`x[..]`)';
  if (/'/.test(bare.replace(/''/g, ''))) return 'an unterminated string';
  for (const m of bare.matchAll(/(?<![\w.-])[A-Za-z_][\w-]*(?:\s*\.\s*[\w*-]+)*/g)) {
    const path = m[0].replace(/\s+/g, '');
    const after = bare.slice(m.index + m[0].length).trimStart();
    if (after.startsWith('(')) {
      if (!SAFE_FUNCTIONS.has(path.toLowerCase())) return `the function ${path}()`;
      continue;
    }
    if (!SAFE_IN_SCRIPT.some((re) => re.test(path))) return `\`${path}\``;
  }
  return null;
}
const SECRETS_INHERIT = /\bsecrets\s*:\s*['"]?inherit\b/i;
const PRIVILEGED_TRIGGER = /^(?:pull_request_target|workflow_run)$/;
const SELF_HOSTED = /\bself-hosted\b/i;
const OPT_IN_SWITCH = /--(?:include-)?ignored\b|\bOAC_TEST_[A-Z0-9_]+|tools[\\/]+herdr/;
// #345: the one tools/herdr line the default tier may hold: a `run:` that is exactly the
// driver's offline self-test (its test doubles only; it drives no harness), optionally behind
// the loopback-only wrapper. Nothing else on the line, no other driver argument or entry
// point, and not inside a run: block scalar. Kept in step with check 9's exception in
// scripts/check-herdr-containment.mjs (DRIVER_SELFTEST_LINE there).
const DRIVER_SELFTEST_LINE = /^\s*(?:-\s+)?run:\s+(?:\$LOOPBACK_ONLY\s+)?node\s+tools\/herdr\/run\.mjs\s+--self-test\s*$/;
// PR #349 review B1: YAML folds a deeper-indented next line into a plain `run:` value, so the
// physical line alone does not decide. A line is exempt only when ALL of these hold: it
// matches DRIVER_SELFTEST_LINE (an unquoted plain value: no block indicator, quote or flow
// collection); the next non-blank line (comments included) is not indented deeper than the
// `run` key; and the structural reader's `run` pair whose key is on this line has a value
// starting on this line that is exactly DRIVER_SELFTEST_COMMAND (the reader joins plain,
// quoted and flow continuations, so any folded-in text breaks the match). A document the
// reader cannot read (W0) gets no exemption.
const DRIVER_SELFTEST_COMMAND = /^(?:\$LOOPBACK_ONLY )?node tools\/herdr\/run\.mjs --self-test$/;
function continuesBelow(lines, i) {
  const keyCol = lines[i].indexOf('run:');
  for (let j = i + 1; j < lines.length; j++) {
    if (lines[j].trim() === '') continue;
    return indentOf(lines[j]) > keyCol;
  }
  return false;
}
// #353 (PR #349 re-review hardening): nothing around the line may change what it runs. The
// `run` pair must sit directly in a step of a job's `steps:` whose keys are only
// SELFTEST_STEP_KEYS, each once (no `env:`, no `working-directory:`, no second `run:`), whose
// `shell:`, if any, is exactly `bash`; the job has no `env:` or `defaults:`; the workflow has no
// top-level `defaults:` and its top-level `env:` sets only CARGO_* names. YAML anchors, aliases
// and merge keys are already W0 (the reader refuses them), and an unreadable document gets no
// exemption. Kept in step with check 9 in scripts/check-herdr-containment.mjs.
const SELFTEST_STEP_KEYS = new Set(['name', 'id', 'if', 'shell', 'run', 'timeout-minutes']);
const SELFTEST_WORKFLOW_ENV = /^CARGO_[A-Z0-9_]+$/;
function selftestStepOk(step, job, doc) {
  const keys = step.pairs.map((p) => p.key);
  if (new Set(keys).size !== keys.length || !keys.every((k) => SELFTEST_STEP_KEYS.has(k))) return false;
  const shell = get(step, 'shell');
  if (shell && !(shell.value.t === 'str' && shell.value.v === 'bash')) return false;
  if (job?.t !== 'map' || new Set(job.pairs.map((p) => p.key)).size !== job.pairs.length || ['env', 'defaults', 'container', 'services'].some((k) => get(job, k))) return false;
  if (new Set(doc.pairs.map((p) => p.key)).size !== doc.pairs.length || get(doc, 'defaults')) return false;
  const env = get(doc, 'env')?.value;
  if (env && !(env.t === 'map' && env.pairs.every((p) => SELFTEST_WORKFLOW_ENV.test(p.key))) && !(env.t === 'str' && env.v === '')) return false;
  return true;
}
// The 1-based lines whose `run:` is exactly the driver self-test, as above.
function driverSelftestLines(text, doc) {
  const out = new Set();
  if (doc?.t !== 'map') return out;
  const lines = text.split(/\r?\n/);
  const jobs = get(doc, 'jobs')?.value;
  for (const j of jobs?.t === 'map' ? jobs.pairs : []) {
    const steps = get(j.value, 'steps')?.value;
    for (const step of steps?.t === 'seq' ? steps.items : []) {
      if (step.t !== 'map') continue;
      for (const p of step.pairs) {
        if (p.key !== 'run' || p.value.t !== 'str' || p.value.line !== p.line || !DRIVER_SELFTEST_COMMAND.test(p.value.v)) continue;
        if (!selftestStepOk(step, j.value, doc)) continue;
        const i = p.line - 1;
        if (DRIVER_SELFTEST_LINE.test(lines[i] ?? '') && !continuesBelow(lines, i)) out.add(p.line);
      }
    }
  }
  return out;
}
const optInSwitch = (line, n, exempt) => OPT_IN_SWITCH.test(line) && !(DRIVER_SELFTEST_LINE.test(line) && exempt.has(n));
// The package names are spelled with a one-letter class so that oac-boundaries check 3
// (no provider SDK name in the code tree) does not match this lint's own source.
const HARNESS_INSTALL = /@anthropi[c]-ai\/claude-code|@open[a]i\/codex|\bclaude\.ai\/install|\b(?:npm|npx|pnpm|yarn|bun)\b[^\n]*\b(?:claude-code|codex)\b|\bbrew\s+install\b[^\n]*\b(?:codex|claude)\b/i;
// A `uses:` key, quoted or not (#332), and its value.
const USES = /(?:^|[\s{,-])(['"]?)uses\1\s*:\s*(['"]?)([^'",}\s]+)\2/g;

// ---- the line reader ------------------------------------------------------------------

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
      // trimEnd, not a /\s+$/ regex: that is quadratic on a long run of inner spaces.
      return line.slice(0, i).trimEnd();
    }
  }
  return line.trimEnd();
}

const indentOf = (l) => l.match(/^ */)[0].length;

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

// ---- the structural reader -------------------------------------------------------------
//
// Nodes: { t: 'map', line, pairs: [{ key, line, value }] }, { t: 'seq', line, items },
// { t: 'str', line, v }. Lines are 1-based. Throws YamlError on what it cannot read.

class YamlError extends Error {
  constructor(line, msg) {
    super(msg);
    this.line = line;
  }
}

const str = (line, v) => ({ t: 'str', line, v });

// A quoted scalar at s[i] (s[i] is the quote): its value and the index after it.
function readQuoted(s, i, line) {
  const q = s[i];
  let out = '';
  let j = i + 1;
  while (j < s.length) {
    const c = s[j];
    if (q === "'" && c === "'" && s[j + 1] === "'") {
      out += "'";
      j += 2;
    } else if (c === q) {
      return { v: out, end: j + 1 };
    } else if (q === '"' && c === '\\' && j + 1 < s.length) {
      out += s[j + 1];
      j += 2;
    } else {
      out += c;
      j++;
    }
  }
  throw new YamlError(line, 'unterminated quoted scalar');
}

// A flow collection or scalar at s[i]; returns { node, end }.
function readFlow(s, i, line) {
  const ws = () => {
    while (i < s.length && /\s/.test(s[i])) i++;
  };
  ws();
  const c = s[i];
  if (c === '{' || c === '[') {
    const close = c === '{' ? '}' : ']';
    const node = c === '{' ? { t: 'map', line, pairs: [] } : { t: 'seq', line, items: [] };
    i++;
    for (;;) {
      ws();
      if (s[i] === close) return { node, end: i + 1 };
      if (i >= s.length) throw new YamlError(line, `unterminated flow ${c}`);
      if (node.t === 'map') {
        let key;
        if (s[i] === '"' || s[i] === "'") {
          const q = readQuoted(s, i, line);
          key = q.v;
          i = q.end;
        } else {
          let j = i;
          while (j < s.length && !':,{}[]'.includes(s[j])) j++;
          key = s.slice(i, j).trim();
          i = j;
          refuseAnchor(key, line);
          refuseMergeKey(key, false, line);
        }
        ws();
        let value = str(line, '');
        if (s[i] === ':') {
          const r = readFlow(s, i + 1, line);
          value = r.node;
          i = r.end;
        }
        node.pairs.push({ key, line, value });
      } else {
        const r = readFlow(s, i, line);
        node.items.push(r.node);
        i = r.end;
      }
      ws();
      if (s[i] === ',') i++;
      else if (s[i] !== close) throw new YamlError(line, `expected , or ${close} in a flow collection`);
    }
  }
  if (c === '"' || c === "'") {
    const q = readQuoted(s, i, line);
    return { node: str(line, q.v), end: q.end };
  }
  // A plain scalar, up to a flow indicator at depth 0; a `${{ ... }}` is taken whole.
  let j = i;
  let out = '';
  while (j < s.length) {
    if (s.startsWith('${{', j)) {
      const e = s.indexOf('}}', j);
      if (e === -1) throw new YamlError(line, 'unterminated ${{ in a flow scalar');
      out += s.slice(j, e + 2);
      j = e + 2;
      continue;
    }
    if (/[,{}[\]]/.test(s[j])) break;
    out += s[j];
    j++;
  }
  refuseAnchor(out.trim(), line);
  return { node: str(line, out.trim()), end: j };
}

// Is `s` balanced in its flow brackets (outside quotes and `${{ }}`)?
function flowDepth(s) {
  let d = 0;
  let q = null;
  for (let i = 0; i < s.length; i++) {
    const c = s[i];
    if (q) {
      if (c === q) q = null;
    } else if (c === '"' || c === "'") q = c;
    else if (s.startsWith('${{', i)) {
      const e = s.indexOf('}}', i);
      if (e === -1) return d + 1;
      i = e + 1;
    } else if (c === '{' || c === '[') d++;
    else if (c === '}' || c === ']') d--;
  }
  return d;
}

const isSpace = (c) => c === ' ' || c === '\t' || c === '\r' || c === '\n';

// The key of a block mapping line's content, or null: { key, rest, quoted }. A linear
// scan (PR #336 review N5): the first `:` followed by whitespace or the end, before any `#`.
function splitKey(content, line) {
  if (content[0] === '"' || content[0] === "'") {
    const q = readQuoted(content, 0, line);
    let j = q.end;
    while (j < content.length && isSpace(content[j])) j++;
    if (content[j] !== ':' || (j + 1 < content.length && !isSpace(content[j + 1]))) return null;
    return { key: q.v, rest: content.slice(j + 1).trimStart(), quoted: true };
  }
  if (/^[-?:,[\]{}#&*!|>%@`]/.test(content) && !/^-[^\s]/.test(content)) return null;
  for (let j = 0; j < content.length; j++) {
    const c = content[j];
    if (c === '#') return null;
    if (c === ':' && (j + 1 === content.length || isSpace(content[j + 1]))) {
      const key = content.slice(0, j).trimEnd();
      return key === '' ? null : { key, rest: content.slice(j + 1).trimStart(), quoted: false };
    }
  }
  return null;
}

// Anchors, aliases and merge keys are refused outright (PR #336 review B4).
function refuseAnchor(raw, line) {
  // After any tags (`!!str &t ..`), the first character tells (PR #336 re-review).
  const text = raw.replace(/^(?:![^\s]*\s+)+/, '');
  if (text[0] === '&' || text[0] === '*') {
    throw new YamlError(line, `a YAML ${text[0] === '&' ? 'anchor' : 'alias'} (${text.split(/\s/)[0]}): refused, an alias would carry text the rules read only where it was anchored`);
  }
}

function refuseMergeKey(key, quoted, line) {
  if (!quoted && key === '<<') throw new YamlError(line, 'a YAML merge key (<<): refused');
}

function parseYaml(text) {
  const raw = text.split(/\r?\n/);
  const lines = raw.map((l) => {
    if (/\t/.test(l.match(/^\s*/)[0])) throw new YamlError(0, 'tab in indentation');
    return l;
  });
  let i = 0;
  const content = (k) => stripComment(lines[k]).trim();
  const blankAt = (k) => content(k) === '';
  const skip = () => {
    while (i < lines.length && blankAt(i)) i++;
  };

  // A block scalar's lines after line `i` (the indicator line), deeper than `ind`.
  function blockScalar(ind) {
    const out = [];
    let j = i + 1;
    let inner = null;
    while (j < lines.length) {
      const l = lines[j];
      if (l.trim() === '') {
        out.push('');
        j++;
        continue;
      }
      if (indentOf(l) <= ind) break;
      if (inner === null) inner = indentOf(l);
      out.push(l.slice(Math.min(inner, indentOf(l))));
      j++;
    }
    i = j;
    while (out.length && out[out.length - 1] === '') out.pop();
    return out.join('\n');
  }

  // A value that starts on line i at text `rest` (after `key:` or `- `), for a node at `ind`.
  function inlineValue(rest, ind) {
    const line = i + 1;
    refuseAnchor(rest, line);
    if (/^[|>][1-9+-]{0,2}$/.test(rest)) return str(line, blockScalar(ind));
    if (rest[0] === '{' || rest[0] === '[') {
      let s = rest;
      let j = i;
      while (flowDepth(s) > 0 && j + 1 < lines.length) {
        j++;
        s += ` ${content(j)}`;
      }
      const r = readFlow(s, 0, line);
      if (s.slice(r.end).trim() !== '') throw new YamlError(line, 'text after a flow collection');
      i = j + 1;
      return r.node;
    }
    if (rest[0] === '"' || rest[0] === "'") {
      let s = rest;
      let j = i;
      for (;;) {
        try {
          const q = readQuoted(s, 0, line);
          i = j + 1;
          return str(line, q.v);
        } catch (e) {
          if (j + 1 >= lines.length) throw e;
          j++;
          s += ` ${lines[j].trim()}`;
        }
      }
    }
    // A plain scalar, continued on deeper lines.
    let v = rest;
    i++;
    while (i < lines.length && !blankAt(i) && indentOf(lines[i]) > ind) {
      v += ` ${content(i)}`;
      i++;
    }
    return str(line, v);
  }

  function parseMap(ind) {
    const node = { t: 'map', line: i + 1, pairs: [] };
    for (;;) {
      skip();
      if (i >= lines.length) break;
      const l = lines[i];
      const li = indentOf(l);
      if (li < ind) break;
      if (li > ind) throw new YamlError(i + 1, 'unexpected indentation');
      const c = content(i);
      if (c.startsWith('- ') || c === '-') break;
      const kv = splitKey(c, i + 1);
      if (!kv) throw new YamlError(i + 1, `not a mapping key: ${c}`);
      refuseMergeKey(kv.key, kv.quoted, i + 1);
      const keyLine = i + 1;
      let value;
      if (kv.rest === '') {
        i++;
        skip();
        if (i < lines.length && indentOf(lines[i]) === ind && /^-(\s|$)/.test(content(i))) value = parseSeq(ind);
        else if (i < lines.length && indentOf(lines[i]) > ind) value = parseNode(indentOf(lines[i]));
        else value = str(keyLine, '');
      } else {
        value = inlineValue(kv.rest, ind);
      }
      node.pairs.push({ key: kv.key, line: keyLine, value });
    }
    return node;
  }

  function parseSeq(ind) {
    const node = { t: 'seq', line: i + 1, items: [] };
    for (;;) {
      skip();
      if (i >= lines.length) break;
      const l = lines[i];
      if (indentOf(l) !== ind || !/^-(\s|$)/.test(content(i))) {
        if (indentOf(l) > ind) throw new YamlError(i + 1, 'unexpected indentation in a sequence');
        break;
      }
      const after = stripComment(l).slice(ind + 1);
      const rest = after.trim();
      if (rest === '') {
        i++;
        skip();
        node.items.push(i < lines.length && indentOf(lines[i]) > ind ? parseNode(indentOf(lines[i])) : str(i, ''));
        continue;
      }
      const col = ind + 1 + (after.length - after.trimStart().length);
      if (!/^[{[]/.test(rest) && splitKey(rest, i + 1)) {
        // A compact mapping: its first key sits at `col`.
        lines[i] = ' '.repeat(col) + lines[i].slice(col);
        node.items.push(parseMap(col));
      } else if (/^-(\s|$)/.test(rest)) {
        lines[i] = ' '.repeat(col) + lines[i].slice(col);
        node.items.push(parseSeq(col));
      } else {
        node.items.push(inlineValue(rest, ind));
      }
    }
    return node;
  }

  function parseNode(ind) {
    skip();
    if (i >= lines.length) return str(i, '');
    const c = content(i);
    if (/^-(\s|$)/.test(c)) return parseSeq(ind);
    if (splitKey(c, i + 1)) return parseMap(ind);
    return inlineValue(c, ind - 1);
  }

  skip();
  if (i >= lines.length) return { t: 'map', line: 1, pairs: [] };
  if (indentOf(lines[i]) !== 0) throw new YamlError(i + 1, 'the document does not start at column 0');
  const doc = parseNode(0);
  skip();
  if (i < lines.length) throw new YamlError(i + 1, 'text the reader could not place');
  return doc;
}

const get = (node, key) => (node?.t === 'map' ? node.pairs.find((p) => p.key === key) : undefined);

// Every (key, value) pair in the tree, depth first.
function* pairsOf(node) {
  if (!node) return;
  if (node.t === 'map') {
    for (const p of node.pairs) {
      yield p;
      yield* pairsOf(p.value);
    }
  } else if (node.t === 'seq') {
    for (const n of node.items) yield* pairsOf(n);
  }
}

// The steps of every job (a workflow) or of `runs:` (a composite action).
function stepsOf(doc) {
  const out = [];
  const add = (seq) => {
    if (seq?.t === 'seq') for (const s of seq.items) if (s.t === 'map') out.push(s);
  };
  const jobs = get(doc, 'jobs')?.value;
  if (jobs?.t === 'map') for (const j of jobs.pairs) add(get(j.value, 'steps')?.value);
  add(get(get(doc, 'runs')?.value, 'steps')?.value);
  return out;
}

// ---- the checks -----------------------------------------------------------------------

// The `${{ ... }}` expressions in `s`, with their offset.
const expressions = (s) => [...s.matchAll(/\$\{\{([\s\S]*?)\}\}/g)].map((m) => ({ expr: m[1].replace(/\s+/g, ' '), index: m.index }));

// The rules a workflow and a local action share: W0, W1, W3, W4 (not the trigger part), W5,
// and D1-D3 when `defaultTier`. Returns the parsed document (or null) and the local actions
// it references.
function commonRules(text, defaultTier, hit) {
  const lines = readLines(text);
  const { code, scan, block } = lines;
  const locals = [];
  const seenUses = new Set();
  const usesRef = (ref, line) => {
    if (seenUses.has(`${line}\n${ref}`)) return;
    seenUses.add(`${line}\n${ref}`);
    if (ref.startsWith('./')) {
      if (/^\.\/\.github\/workflows\/[^/]+\.ya?ml$/.test(ref)) return; // checked as a workflow
      locals.push({ ref, line });
      return;
    }
    if (!SHA_PIN.test(ref)) hit('W1', line, `action not pinned to a commit SHA: ${ref}`);
  };

  let doc = null;
  try {
    doc = parseYaml(text);
  } catch (e) {
    if (!(e instanceof YamlError)) throw e;
    hit('W0', e.line, `the structural reader cannot read this file (fails closed): ${e.message}`);
  }

  // W1: every action pinned by commit SHA, as either reader sees it.
  code.forEach((l, i) => {
    if (block[i]) return;
    for (const m of l.matchAll(USES)) usesRef(m[3], i + 1);
  });
  for (const p of pairsOf(doc)) {
    if (p.key === 'uses' && p.value.t === 'str') usesRef(p.value.v, p.line);
  }

  if (doc) {
    // W3: checkout never persists the job token; the setting must be an input (`with:`).
    for (const s of stepsOf(doc)) {
      const uses = get(s, 'uses')?.value;
      if (uses?.t !== 'str' || !/^actions\/checkout@/.test(uses.v)) continue;
      const pc = get(get(s, 'with')?.value, 'persist-credentials')?.value;
      if (!(pc?.t === 'str' && pc.v === 'false')) hit('W3', uses.line, 'actions/checkout without persist-credentials: false under with:');
    }
    // W5 for code a step runs: a `run:` script, and actions/github-script's `script:`.
    const injection = (node, where) => {
      for (const { expr, index } of expressions(node.v)) {
        const why = unsafeInScript(expr);
        if (why) {
          const offset = node.v.slice(0, index).split('\n').length - 1;
          const line = node.line + (node.v.includes('\n') || offset > 0 ? offset + 1 : 0);
          hit('W5', line, `\${{${expr}}} inside ${where}: ${why} is not on the allowlist of values that may be pasted into code (script injection); pass it through env: and a shell variable`);
        }
      }
    };
    for (const s of stepsOf(doc)) {
      const uses = get(s, 'uses')?.value;
      if (uses?.t !== 'str' || !/^actions\/github-script@/.test(uses.v)) continue;
      const script = get(get(s, 'with')?.value, 'script')?.value;
      if (script?.t === 'str') injection(script, 'an actions/github-script script: input');
    }
    for (const p of pairsOf(doc)) {
      // W4: an `if:` without `${{ }}` is an expression all the same (#332).
      if (p.key === 'if' && p.value.t === 'str' && !p.value.v.includes('${{')) {
        for (const r of EXPRESSION_RULES) {
          if (r.re.test(p.value.v)) {
            hit('W4', p.line, `${r.msg} (a bare if: condition)`);
            break;
          }
        }
      }
      // W5: untrusted text pasted into a script (#332).
      if (p.key === 'run' && p.value.t === 'str') injection(p.value, 'a run: script');
      // `shell:` (a step's, or `defaults.run.shell`): defence in depth (N-c).
      if (p.key === 'shell' && p.value.t === 'str') injection(p.value, 'a shell:');
    }
  }

  // W4: expressions, read raw and joined across line breaks.
  for (const { expr, index } of expressions(text)) {
    const line = text.slice(0, index).split('\n').length;
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
    const selftestExempt = driverSelftestLines(text, doc);
    scan.forEach((l, i) => {
      if (SELF_HOSTED.test(l)) hit('D1', i + 1, 'self-hosted runner in the default tier');
      if (optInSwitch(l, i + 1, selftestExempt)) hit('D2', i + 1, 'opt-in switch in the default tier');
      if (HARNESS_INSTALL.test(l)) hit('D3', i + 1, 'harness CLI install in the default tier');
    });
  }
  return { doc, locals };
}

// The local actions `locals` reference, checked with the rules, and every local action they
// use in turn (#332), each action once. `report(rule, line, msg)` gets each violation with
// `line` the referencing line in the top file.
function checkLocals(locals, defaultTier, readLocal, report, seen = new Set()) {
  for (const { ref, line } of locals) {
    const action = readLocal(ref);
    if (!action) {
      report('W1', line, `local action ${ref} has no action.yml or action.yaml (fails closed)`);
      continue;
    }
    if (seen.has(action.path)) continue;
    seen.add(action.path);
    const sub = commonRules(action.text, defaultTier, (rule, l, msg) => report(rule, line, `${action.path}:${l}: ${msg}`));
    checkLocals(sub.locals, defaultTier, readLocal, (rule, _l, msg) => report(rule, line, `${action.path}: ${msg}`), seen);
  }
}

// The trigger names of `on:`: a scalar, a sequence or a mapping.
function triggers(doc) {
  const on = get(doc, 'on')?.value;
  if (!on) return null;
  if (on.t === 'str') return on.v === '' ? [] : [on.v];
  if (on.t === 'seq') return on.items.map((n) => (n.t === 'str' ? n.v : '?'));
  return on.pairs.map((p) => p.key);
}

// A permission value: read and none are read-only.
const readOnly = (v) => v?.t === 'str' && (v.v === 'read' || v.v === 'none');

// One workflow. `readLocal(ref)` returns { path, text } for a local action, or null.
function checkWorkflow(name, text, readLocal = () => null) {
  const v = [];
  const hit = (rule, line, msg) => v.push({ rule, line, msg });

  const { doc, locals } = commonRules(text, null, hit);
  // (An unreadable document is already a W0 violation.)
  const on = doc ? triggers(doc) : null;
  if (doc && (on === null || on.length === 0)) hit('W4', 0, 'no readable `on:` triggers');
  for (const t of on ?? []) if (PRIVILEGED_TRIGGER.test(t)) hit('W4', 0, `privileged trigger ${t}`);
  const optIn = on !== null && on.length > 0 && on.every((t) => OPT_IN_TRIGGERS.has(t));
  const defaultTier = !optIn && name !== HERDR_OPTIN;

  if (doc) {
    // W2: top-level permissions exactly contents: read.
    const top = get(doc, 'permissions');
    if (!top) hit('W2', 0, 'no top-level permissions');
    else {
      const p = top.value;
      const kv = p.t === 'map' ? p.pairs.map((c) => `${c.key}: ${c.value.t === 'str' ? c.value.v : '?'}`) : [];
      if (p.t !== 'map' || kv.length !== 1 || kv[0] !== 'contents: read') {
        hit('W2', top.line, `top-level permissions must be exactly contents: read (found ${p.t === 'str' ? p.v : kv.join(', ') || 'none'})`);
      }
    }
    // W2: every job's permissions read-only, the job in block or flow style.
    const jobs = get(doc, 'jobs')?.value;
    for (const job of jobs?.t === 'map' ? jobs.pairs : []) {
      const perm = get(job.value, 'permissions');
      if (!perm) continue;
      const p = perm.value;
      if (p.t === 'str') {
        if (p.v !== 'read-all') hit('W2', perm.line, `job ${job.key} permissions ${p.v}`);
      } else if (p.t === 'map') {
        for (const c of p.pairs) if (!readOnly(c.value)) hit('W2', c.line, `job ${job.key} permission ${c.key}: ${c.value.t === 'str' ? c.value.v : '(not a scalar)'}`);
      } else hit('W2', perm.line, `job ${job.key}: unreadable permissions`);
    }
  }

  // D1-D3 need the tier, known only now: run them over the workflow's own text again.
  if (defaultTier) {
    const { scan } = readLines(text);
    const selftestExempt = driverSelftestLines(text, doc);
    scan.forEach((l, i) => {
      if (SELF_HOSTED.test(l)) hit('D1', i + 1, 'self-hosted runner in the default tier');
      if (optInSwitch(l, i + 1, selftestExempt)) hit('D2', i + 1, 'opt-in switch in the default tier');
      if (HARNESS_INSTALL.test(l)) hit('D3', i + 1, 'harness CLI install in the default tier');
    });
  }
  checkLocals(locals, defaultTier, readLocal, hit);
  return v;
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

// A composite action file found on disk, held to the rules on its own (default tier), with
// every local action it uses.
function checkAction(text, readLocal) {
  const v = [];
  const hit = (rule, line, msg) => v.push({ rule, line, msg });
  const { locals } = commonRules(text, true, hit);
  checkLocals(locals, true, readLocal, hit);
  return v;
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
    const v = checkAction(readFileSync(p, 'utf8'), readLocal);
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
const withJobs = (jobs) => GOOD.replace(/jobs:\n[\s\S]*$/, `jobs:\n${jobs}\n`);
// #353: the exempt herdr self-test step as ci.yml writes it, with optional extra lines (no
// trailing newline, for withStep).
const SELFTEST_STEP = ({ shell = '        shell: bash\n', before = '', after = '' } = {}) =>
  `      - name: herdr self-test\n${shell}${before}        run: $LOOPBACK_ONLY node tools/herdr/run.mjs --self-test\n${after}`.replace(/\n$/, '');
const ACTION_GOOD = `name: setup
runs:
  using: composite
  steps:
    - uses: actions/cache/restore@${SHA}
      with:
        key: x
`;
const ACTION_NESTING = (inner) => `name: outer
runs:
  using: composite
  steps:
    - uses: ${inner}
`;
const SETUP = { './.github/actions/setup': ACTION_GOOD };

const CASES = [
  ['control: default-tier workflow', 'good.yml', GOOD, []],
  ['control: opt-in workflow may use --ignored and OAC_TEST_*', 'optin.yml', OPTIN, []],
  ['control: a scheduled workflow with nothing D1-D3 refuses', 'x.yml', GOOD.replace(/on:\n  push:\n    branches: \[main\]\n  pull_request:\n/, "on:\n  schedule:\n    - cron: '0 5 * * 1'\n  workflow_dispatch:\n"), []],
  ['control: github-script reading the event through env:', 'x.yml', withStep(`      - uses: actions/github-script@${SHA}\n        env:\n          TITLE: \${{ github.event.issue.title }}\n        with:\n          script: console.log(process.env.TITLE)`), []],
  ['control: inline on list', 'x.yml', GOOD.replace(/on:\n  push:\n    branches: \[main\]\n  pull_request:\n/, 'on: [push, pull_request]\n'), []],
  ['control: top-level permissions as a flow mapping', 'x.yml', GOOD.replace('permissions:\n  contents: read', 'permissions: { contents: read }'), []],
  ['control: job-level read and none, block and flow', 'x.yml', withJob('    permissions:\n      contents: read\n      id-token: none').replace('  test:', '  other:\n    permissions: { contents: "read" }\n    runs-on: x\n  test:'), []],
  ['control: a local action held to the rules', 'x.yml', withStep('      - uses: ./.github/actions/setup'), [], SETUP],
  ['control: an action input named permissions under with: (block)', 'x.yml', withStep(`      - uses: actions/github-script@${SHA}\n        with:\n          permissions: write`), []],
  ['control: an action input named permissions under with: (flow)', 'x.yml', withStep(`      - uses: actions/github-script@${SHA}\n        with: { permissions: write-all }`), []],
  ['control: a bare if: on a safe context', 'x.yml', withStep("      - if: github.event_name == 'push' && matrix.os == 'ubuntu-latest'\n        run: true"), []],
  ['control: the event payload through env:, and event_name in run:', 'x.yml', withStep('      - env:\n          TITLE: ${{ github.event.issue.title }}\n        run: echo "$TITLE ${{ github.event_name }}"'), []],
  ['control: a local action that uses itself (no loop)', 'x.yml', withStep('      - uses: ./.github/actions/self'), [], { './.github/actions/self': ACTION_NESTING('./.github/actions/self') }],
  ['W1 tag-pinned action', 'x.yml', GOOD.replace(`checkout@${SHA}`, 'checkout@v5'), ['W1']],
  ['W1 docker image', 'x.yml', GOOD.replace(`actions/cache/restore@${SHA}`, 'docker://alpine:3'), ['W1']],
  ['W1 flow-style step', 'x.yml', withStep('      - { uses: actions/setup-node@v4 }'), ['W1']],
  ['W1 quoted "uses" key (#332)', 'x.yml', withStep('      - "uses": actions/setup-node@v4'), ['W1']],
  ["W1 single-quoted 'uses' key in a flow step (#332)", 'x.yml', withStep("      - { 'uses': actions/setup-node@v4 }"), ['W1']],
  ['W1 local action file missing (fails closed)', 'x.yml', withStep('      - uses: ./.github/actions/missing'), ['W1']],
  ['W1 tag-pinned action inside a local action', 'x.yml', withStep('      - uses: ./.github/actions/setup'), ['W1'], { './.github/actions/setup': ACTION_GOOD.replace(`restore@${SHA}`, 'restore@v4') }],
  ['W1 tag-pinned action in a local action outside .github/actions/, used by a local action (#332)', 'x.yml', withStep('      - uses: ./.github/actions/outer'), ['W1'], { './.github/actions/outer': ACTION_NESTING('./tools/inner'), './tools/inner': ACTION_GOOD.replace(`restore@${SHA}`, 'restore@v4') }],
  ['W1 nested local action file missing (#332)', 'x.yml', withStep('      - uses: ./.github/actions/outer'), ['W1'], { './.github/actions/outer': ACTION_NESTING('./tools/gone') }],
  ['W4 secrets two local actions deep (#332)', 'x.yml', withStep('      - uses: ./.github/actions/outer'), ['W4'], { './.github/actions/outer': ACTION_NESTING('./tools/inner'), './tools/inner': ACTION_GOOD.replace('key: x', 'key: ${{ secrets.K }}') }],
  ['W4 secrets inside a local action', 'x.yml', withStep('      - uses: ./.github/actions/setup'), ['W4'], { './.github/actions/setup': ACTION_GOOD.replace('key: x', 'key: ${{ secrets.K }}') }],
  ['W2 write permission at top level', 'x.yml', GOOD.replace('contents: read', 'contents: write'), ['W2']],
  ['W2 read-all at top level', 'x.yml', GOOD.replace('permissions:\n  contents: read', 'permissions: read-all'), ['W2']],
  ['W2 extra top-level permission', 'x.yml', GOOD.replace('  contents: read', '  contents: read\n  id-token: read'), ['W2']],
  ['W2 no permissions block', 'x.yml', GOOD.replace('permissions:\n  contents: read\n', ''), ['W2']],
  ['W2 job-level write (block)', 'x.yml', withJob('    permissions:\n      pull-requests: write'), ['W2']],
  ['W2 job-level write (flow)', 'x.yml', withJob('    permissions: { contents: write }'), ['W2']],
  ['W2 job-level write (quoted)', 'x.yml', withJob("    permissions:\n      contents: 'write'"), ['W2']],
  ['W2 job-level write-all', 'x.yml', withJob('    permissions: write-all'), ['W2']],
  ['W2 job-level quoted "permissions" key (#332)', 'x.yml', withJob('    "permissions": write-all'), ['W2']],
  ['W2 a job written as one flow mapping (#332)', 'x.yml', withJobs('  test: { permissions: write-all, runs-on: ubuntu-latest, steps: [ { run: "true" } ] }'), ['W2']],
  ['W2 a flow-style job with nested write (#332)', 'x.yml', withJobs('  test: { runs-on: x, permissions: { contents: read, pull-requests: write }, steps: [] }'), ['W2']],
  ['W2 a flow-style job over several lines (#332)', 'x.yml', withJobs('  test: {\n    runs-on: x,\n    permissions: write-all,\n    steps: []\n  }'), ['W2']],
  ['W2 the jobs mapping itself in flow style (#332)', 'x.yml', withJobs('  { test: { "permissions": { "contents": "write" }, runs-on: x } }').replace('jobs:\n  {', 'jobs: {'), ['W2']],
  ['W3 checkout persisting credentials', 'x.yml', GOOD.replace('persist-credentials: false', 'fetch-depth: 0'), ['W3']],
  ['W3 persist-credentials under env, not with', 'x.yml', GOOD.replace('        with:\n          persist-credentials: false', '        env:\n          persist-credentials: false'), ['W3']],
  ['W3 a flow-style checkout step without the setting', 'x.yml', withStep(`      - { uses: actions/checkout@${SHA}, with: { fetch-depth: 0 } }`), ['W3']],
  ['W4 secrets context', 'x.yml', GOOD.replace('key: cargo-', 'key: ${{ secrets.CACHE_KEY }}-'), ['W4']],
  ['W4 github.token', 'x.yml', GOOD.replace('key: cargo-', 'key: ${{ github.token }}-'), ['W4']],
  ["W4 github['token']", 'x.yml', GOOD.replace('key: cargo-', "key: ${{ github['token'] }}-"), ['W4']],
  ['W4 toJSON(github)', 'x.yml', GOOD.replace('key: cargo-', 'key: ${{ toJSON(github) }}-'), ['W4']],
  ['W4 an expression split across lines', 'x.yml', withStep('      - env:\n          T: ${{ github\n            .token }}\n        run: true'), ['W4']],
  ['W4 a heredoc # line in a run: block', 'x.yml', GOOD.replace('          echo done', '          cat <<EOF\n          #${{ secrets.K }}\n          EOF'), ['W5', 'W4']],
  ['W4 secrets: inherit', 'x.yml', GOOD.replace('    runs-on: ${{ matrix.os }}\n    steps:', '    uses: ./.github/workflows/other.yml\n    secrets: inherit\n    steps:'), ['W4']],
  ['W4 provider key variable', 'x.yml', withStep('      - env:\n          ANTHROPIC_API_KEY: x\n        run: true'), ['W4']],
  ['W4 secrets in an opt-in workflow', 'optin.yml', OPTIN.replace('OAC_TEST_REAL_KEYRING: "1"', 'TOKEN: ${{ secrets.T }}'), ['W4']],
  ['W4 pull_request_target', 'x.yml', GOOD.replace('  pull_request:', '  pull_request_target:'), ['W4']],
  ['W4 bare if: on github.token (#332)', 'x.yml', withStep("      - if: startsWith(github.token, 'ghs_')\n        run: true"), ['W4']],
  ['W4 bare if: on the whole github context (#332)', 'x.yml', withStep("      - if: contains(toJSON(github), 'x')\n        run: true"), ['W4']],
  ['W4 bare job-level if: on secrets (#332)', 'x.yml', withJob("    if: secrets.K != ''"), ['W4']],
  ['W4 bare if: under a quoted key, over two lines (#332)', 'x.yml', withStep("      - \"if\": always() &&\n          github.token != ''\n        run: true"), ['W4']],
  ['W4 bare if: as a block scalar (#332)', 'x.yml', withStep("      - if: >\n          github['token'] != ''\n        run: true"), ['W4']],
  ['W5 github.event in an inline run: (#332)', 'x.yml', withStep('      - run: echo "${{ github.event.issue.title }}"'), ['W5']],
  ['W5 github.event in a run: block scalar (#332)', 'x.yml', GOOD.replace('          echo done', '          echo "${{ github.event.pull_request.head.ref }}"'), ['W5']],
  ['W5 spaced, upper-case and split github . event (#332)', 'x.yml', GOOD.replace('          echo done', '          echo "${{ GITHUB .\n            EVENT.comment.body }}"'), ['W5']],
  ['W5 toJSON(github.event) in run: (#332)', 'x.yml', withStep('      - run: echo \'${{ toJSON(github.event) }}\''), ['W5']],
  ['W5 a flow-style step with a quoted run: (#332)', 'x.yml', withStep('      - { "run": "echo ${{ github.event.head_commit.message }}" }'), ['W5']],
  ['W5 github.event in a run: inside a local action (#332)', 'x.yml', withStep('      - uses: ./.github/actions/setup'), ['W5'], { './.github/actions/setup': `${ACTION_GOOD}    - shell: bash\n      run: echo "\${{ github.event.issue.body }}"\n` }],
  // PR #336 review N1: W5 also covers the head branch name, inputs, and github-script.
  ['W5 github.head_ref in run: (PR #336 N1)', 'x.yml', withStep('      - run: git checkout "${{ github.head_ref }}"'), ['W5']],
  ['W5 inputs.x in run: (PR #336 N1)', 'x.yml', withStep('      - run: echo ${{ inputs.scenario }}'), ['W5']],
  ["W5 inputs['x'] in run: (PR #336 N1)", 'x.yml', withStep("      - run: echo ${{ inputs['scenario'] }}"), ['W5']],
  ['W5 github.event in a github-script script: (PR #336 N1)', 'x.yml', withStep(`      - uses: actions/github-script@${SHA}\n        with:\n          script: |\n            const t = "\${{ github.event.issue.title }}";`), ['W5']],
  ['W5 inputs in a flow-style github-script step (PR #336 N1)', 'x.yml', withStep(`      - { uses: actions/github-script@${SHA}, with: { script: "core.info('\${{ inputs.x }}')" } }`), ['W5']],
  // PR #336 re-review R3: only an allowlist may be pasted into a script.
  ["W5 the re-review's env.X after a step env: (R3)", 'x.yml', withStep('      - env:\n          X: ${{ github.event.issue.title }}\n        run: echo "${{ env.X }}"'), ['W5']],
  ["W5 the re-review's env.X after a job env: (R3)", 'x.yml', withJob('    env:\n      X: ${{ github.event.issue.title }}').replace('      - run: cargo test', '      - run: echo "${{ env.X }}"\n      - run: cargo test'), ['W5']],
  ['W5 a step output in run: (R3)', 'x.yml', withStep('      - run: echo "${{ steps.s.outputs.t }}"'), ['W5']],
  ['W5 needs.*.outputs in run:', 'x.yml', withStep('      - run: echo "${{ needs.a.outputs.b }}"'), ['W5']],
  ['W5 vars.* in run:', 'x.yml', withStep('      - run: echo "${{ vars.NAME }}"'), ['W5']],
  ['W5 github.ref_name in run:', 'x.yml', withStep('      - run: echo "${{ github.ref_name }}"'), ['W5']],
  ['W5 an allowlisted root, indexed', 'x.yml', withStep("      - run: echo \"${{ runner['os'] }}\""), ['W5']],
  ['W5 an unknown function', 'x.yml', withStep('      - run: echo "${{ fromJSONx(runner.os) }}"'), ['W5']],
  ['W5 an allowlisted value next to a refused one', 'x.yml', withStep("      - run: echo \"${{ format('{0}-{1}', runner.os, env.X) }}\""), ['W5']],
  // PR #336 third review S2: a matrix value can carry untrusted text.
  ["W5 the third review's matrix plant (S2)", 'x.yml', GOOD.replace('    runs-on: ${{ matrix.os }}', '    strategy:\n      matrix:\n        t: ["${{ github.event.issue.title }}"]\n    runs-on: ubuntu-latest').replace('      - run: cargo test', '      - run: echo ${{ matrix.t }}\n      - run: cargo test'), ['W5']],
  ['W5 a matrix from fromJSON(needs..) (S2)', 'x.yml', GOOD.replace('    runs-on: ${{ matrix.os }}', '    strategy:\n      matrix: ${{ fromJSON(needs.x.outputs.m) }}\n    runs-on: ubuntu-latest').replace('      - run: cargo test', '      - run: echo "${{ matrix.t }}"\n      - run: cargo test'), ['W5']],
  ['W5 matrix.os in a github-script script:', 'x.yml', withStep(`      - uses: actions/github-script@${SHA}\n        with:\n          script: core.info('\${{ matrix.os }}')`), ['W5']],
  // N-c: `shell:` too, as defence in depth.
  ['W5 event text in a step shell: (N-c)', 'x.yml', withStep('      - shell: bash -c "${{ github.event.issue.title }} {0}"\n        run: true'), ['W5']],
  ['W5 event text in defaults.run.shell (N-c)', 'x.yml', GOOD.replace('jobs:\n  test:\n', 'defaults:\n  run:\n    shell: "${{ github.head_ref }} {0}"\njobs:\n  test:\n'), ['W5']],
  ['control: allowlisted expressions in run:', 'x.yml', withStep("      - run: |\n          echo \"${{ runner.os }} ${{ runner.temp }} ${{ github.sha }} ${{ github.run_id }}\"\n          echo \"${{ steps.s.outcome == 'success' && 'yes' || 'no' }} ${{ format('{0}-x', runner.os) }}\"\n          echo \"${{ hashFiles('Cargo.lock') }} ${{ 3 }} ${{ true }} ${{ 'it''s env.X, a string' }}\""), []],
  ['control: matrix.* through env: and a plain shell:', 'x.yml', withStep('      - shell: bash\n        env:\n          OS: ${{ matrix.os }}\n        run: echo "$OS"'), []],
  ['W0 an anchor after a tag', 'x.yml', withStep('      - env:\n          T: !!str &t echo hi\n        run: true'), ['W0']],
  // PR #336 review B4: anchors, aliases and merge keys are refused (W0).
  ["W0 the review's alias evasion of W5", 'x.yml', GOOD.replace('jobs:\n  test:\n', 'env:\n  T: &t echo "${{ github.event.issue.title }}"\njobs:\n  test:\n').replace('      - run: cargo test', '      - run: *t\n      - run: cargo test'), ['W0']],
  ['W0 an anchor on a block mapping', 'x.yml', withJob('    permissions: &p\n      contents: read'), ['W0']],
  ['W0 an alias as a sequence item', 'x.yml', withStep('      - *step'), ['W0']],
  ['W0 an alias in a flow mapping', 'x.yml', withStep(`      - { uses: actions/checkout@${SHA}, with: *w }`), ['W0']],
  ['W0 an anchored flow collection', 'x.yml', withStep(`      - uses: actions/checkout@${SHA}\n        with: &w { persist-credentials: false }`), ['W0']],
  ['W0 a merge key', 'x.yml', withJob('    <<: { permissions: write-all }'), ['W0']],
  ['W0 a merge key in a flow mapping', 'x.yml', withJobs('  test: { <<: { permissions: write-all }, runs-on: x }'), ['W0']],
  ['W0 a workflow the structural reader cannot read', 'x.yml', GOOD.replace('jobs:\n  test:', 'jobs:\n  test: {'), ['W0']],
  ['D1 self-hosted runner', 'x.yml', GOOD.replace('${{ matrix.os }}', '[self-hosted, x]'), ['D1']],
  ['D1 self-hosted routed through a matrix', 'x.yml', GOOD.replace('    runs-on: ${{ matrix.os }}', '    strategy:\n      matrix:\n        r: [self-hosted]\n    runs-on: ${{ matrix.r }}'), ['D1']],
  ['D2 --ignored in the default tier', 'x.yml', GOOD.replace('cargo test --workspace #', 'cargo test --workspace -- --ignored #'), ['D2']],
  ['D2 --ignored inside a run: block', 'x.yml', GOOD.replace('          echo done', '          cargo test -- --include-ignored'), ['D2']],
  ['D2 OAC_TEST_ flag in the default tier', 'x.yml', withStep('      - env:\n          OAC_TEST_REAL_KEYRING: "1"\n        run: true'), ['D2']],
  ['D2 herdr driver in the default tier', 'x.yml', GOOD.replace('cargo test --workspace', 'node tools/herdr/ci.mjs run'), ['D2']],
  // #345: only the exact offline self-test line is allowed.
  ['control: the herdr driver self-test line (#345)', 'x.yml', withStep('      - run: $LOOPBACK_ONLY node tools/herdr/run.mjs --self-test'), []],
  ['control: the herdr driver self-test line, no wrapper (#345)', 'x.yml', withStep('      - shell: bash\n        run: node tools/herdr/run.mjs --self-test'), []],
  ['D2 the driver with a scenario beside --self-test (#345)', 'x.yml', withStep('      - run: node tools/herdr/run.mjs --self-test --scenario smoke'), ['D2']],
  ['D2 the driver run with a scenario (#345)', 'x.yml', withStep('      - run: node tools/herdr/run.mjs --scenario g2-codex-inject'), ['D2']],
  ['D2 the self-test chained to another driver call (#345)', 'x.yml', withStep('      - run: node tools/herdr/run.mjs --self-test && node tools/herdr/ci.mjs run'), ['D2']],
  ['D2 the self-test line inside a run: block (#345)', 'x.yml', GOOD.replace('          echo done', '          node tools/herdr/run.mjs --self-test'), ['D2']],
  ['D2 the self-test of another driver entry point (#345)', 'x.yml', withStep('      - run: node tools/herdr/ci.mjs --self-test'), ['D2']],
  // #353 (PR #349 re-review): the exempt step, its job or the workflow changes what the line
  // runs. check 9 in scripts/check-herdr-containment.mjs plants the same set.
  ['control: the ci.yml shape keeps its exemption (#353)', 'x.yml', withStep(SELFTEST_STEP()), []],
  ['D2 step env: overrides LOOPBACK_ONLY (#353)', 'x.yml', withStep(SELFTEST_STEP({ before: "        env:\n          LOOPBACK_ONLY: sh -c 'echo x' --\n" })), ['D2']],
  ['D2 step env: sets NODE_OPTIONS (#353)', 'x.yml', withStep(SELFTEST_STEP({ before: '        env:\n          NODE_OPTIONS: --require ./x.js\n' })), ['D2']],
  ['D2 step working-directory: (#353)', 'x.yml', withStep(SELFTEST_STEP({ before: '        working-directory: vendor/other\n' })), ['D2']],
  ['D2 shell: node {0} (#353)', 'x.yml', withStep(SELFTEST_STEP({ shell: '        shell: node {0}\n' })), ['D2']],
  ['D2 shell: sh (#353)', 'x.yml', withStep(SELFTEST_STEP({ shell: '        shell: sh\n' })), ['D2']],
  ['D2 shell: bash with its own arguments (#353)', 'x.yml', withStep(SELFTEST_STEP({ shell: '        shell: bash --rcfile ./x.sh {0}\n' })), ['D2']],
  ['D2 a second run: key after the exempt one (#353)', 'x.yml', withStep(SELFTEST_STEP({ after: '        run: echo replaced\n' })), ['D2']],
  ['D2 a second run: key before the exempt one (#353)', 'x.yml', withStep(SELFTEST_STEP({ before: '        run: echo first\n' })), ['D2']],
  ['D2 a uses: on the exempt step (#353)', 'x.yml', withStep(SELFTEST_STEP({ before: '        uses: ./.github/actions/setup\n' })), ['D2'], SETUP],
  ['D2 job env: (#353)', 'x.yml', withJob('    env:\n      NODE_OPTIONS: --require ./x.js').replace('      - run: cargo test', `${SELFTEST_STEP()}\n      - run: cargo test`), ['D2']],
  ['D2 job defaults: run: working-directory (#353)', 'x.yml', withJob('    defaults:\n      run:\n        working-directory: vendor').replace('      - run: cargo test', `${SELFTEST_STEP()}\n      - run: cargo test`), ['D2']],
  ['D2 job container: (#353 review)', 'x.yml', withJob('    container: node:20').replace('      - run: cargo test', `${SELFTEST_STEP()}\n      - run: cargo test`), ['D2']],
  ['D2 job services: (#353 review)', 'x.yml', withJob('    services:\n      x:\n        image: alpine').replace('      - run: cargo test', `${SELFTEST_STEP()}\n      - run: cargo test`), ['D2']],
  ['D2 workflow defaults: run: shell (#353)', 'x.yml', withStep(SELFTEST_STEP()).replace('jobs:\n  test:\n', 'defaults:\n  run:\n    shell: sh\njobs:\n  test:\n'), ['D2']],
  ['D2 workflow env: BASH_ENV (#353)', 'x.yml', withStep(SELFTEST_STEP()).replace('jobs:\n  test:\n', 'env:\n  BASH_ENV: ./x.sh\njobs:\n  test:\n'), ['D2']],
  ['control: a CARGO_* workflow env keeps the exemption (#353)', 'x.yml', withStep(SELFTEST_STEP()).replace('jobs:\n  test:\n', 'env:\n  CARGO_TERM_COLOR: always\njobs:\n  test:\n'), []],
  ['D2 a YAML anchor elsewhere in the workflow (#353)', 'x.yml', withStep(SELFTEST_STEP()).replace('jobs:\n  test:\n', 'env:\n  CARGO_X: &x echo hi\njobs:\n  test:\n'), ['W0', 'D2']],
  ['D2 a YAML alias step after the exempt one (#353)', 'x.yml', withStep(`${SELFTEST_STEP()}\n      - run: *x`).replace('jobs:\n  test:\n', 'env:\n  CARGO_X: &x echo hi\njobs:\n  test:\n'), ['W0', 'D2']],
  ['D2 a YAML alias as a sequence item (#353)', 'x.yml', withStep(`${SELFTEST_STEP()}\n      - *s`), ['W0', 'D2']],
  ['D2 a YAML merge key in the job (#353)', 'x.yml', withJob('    <<: { env: { NODE_OPTIONS: x } }').replace('      - run: cargo test', `${SELFTEST_STEP()}\n      - run: cargo test`), ['W0', 'D2']],
  // PR #349 review B1: text YAML folds into the exempt line's value, or a line inside another
  // scalar that looks like the exempt line. Each passed the single-line check alone.
  ['D2 a plain continuation line folds a scenario into the self-test (PR #349 B1)', 'x.yml', withStep('      - run: node tools/herdr/run.mjs --self-test\n          --scenario g2-codex-inject'), ['D2']],
  ['D2 a continuation after a blank line (PR #349 B1)', 'x.yml', withStep('      - run: $LOOPBACK_ONLY node tools/herdr/run.mjs --self-test\n\n          --scenario smoke'), ['W0', 'D2']],
  ['D2 a continuation under a key-form run: (PR #349 B1)', 'x.yml', withStep('      - shell: bash\n        run: node tools/herdr/run.mjs --self-test\n          --scenario smoke'), ['D2']],
  ['D2 the line inside a | block scalar (PR #349 B1)', 'x.yml', withStep('      - run: |\n          run: node tools/herdr/run.mjs --self-test\n          echo next'), ['D2']],
  ['D2 the line inside a > block scalar (PR #349 B1)', 'x.yml', withStep('      - run: >\n          run: node tools/herdr/run.mjs --self-test'), ['D2']],
  ['D2 the line inside a multi-line double-quoted scalar (PR #349 B1)', 'x.yml', withStep('      - run: "echo start\n          run: node tools/herdr/run.mjs --self-test\n          --scenario smoke"'), ['D2']],
  ["D2 the line inside a multi-line single-quoted scalar (PR #349 B1)", 'x.yml', withStep("      - run: 'echo start\n          run: node tools/herdr/run.mjs --self-test\n          --scenario smoke'"), ['D2']],
  ['D2 the line inside a multi-line flow mapping (PR #349 B1)', 'x.yml', withStep('      - { name: x,\n          run: node tools/herdr/run.mjs --self-test\n          --scenario smoke }'), ['D2']],
  // PR #336 review B5: a schedule runs unattended, so it is the default tier.
  ['D2 D3 a schedule-only workflow is the default tier', 'x.yml', `name: s\non: { schedule: [ { cron: '0 * * * *' } ] }\npermissions:\n  contents: read\njobs:\n  j:\n    runs-on: ubuntu-latest\n    steps:\n      - run: npm install -g @open${'ai'}/codex && cargo test -- --include-ignored && OAC_TEST_REAL=1 node tools/herdr/ci.mjs run\n`, ['D2', 'D3']],
  ['D2 a schedule with workflow_dispatch is still the default tier', 'optin.yml', OPTIN.replace('  workflow_dispatch:', "  schedule:\n    - cron: '0 5 * * 1'\n  workflow_dispatch:"), ['D2', 'D2']],
  ['D2 a schedule beside a push is still the default tier', 'x.yml', GOOD.replace('  pull_request:', "  pull_request:\n  schedule:\n    - cron: '0 5 * * 1'").replace('cargo test --workspace #', 'cargo test -- --ignored #'), ['D2']],
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
