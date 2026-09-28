#!/usr/bin/env node
// Redaction for everything the herdr driver captures (pane text, run manifests, JSONL
// transcripts) before it can leave the run's scratch directory.
//
//   node tools/herdr/lib/redact.mjs --check <file>             # scan only; exit 1 on any residual
//   node tools/herdr/lib/redact.mjs <in> <out>                 # redact <in> into <out>
//   options: --jsonl | --text (default: by extension), --username U, --hostname H,
//            --home PATH, --literal VALUE=<PLACEHOLDER> (repeatable)
//
// What it strips: home paths (this machine's, and any /home/<x>, /Users/<x>, C:\Users\<x>,
// /root, and Claude Code's mangled project-dir form), the OS username and hostname,
// e-mail addresses, and token shapes (sk-..., Bearer, GitHub/Slack/AWS/Google keys,
// JWTs, NAME_TOKEN=/NAME_KEY=/NAME_SECRET= assignments). Lines carrying a private-key block
// marker, an Authorization/Cookie header, or a system-prompt snapshot are dropped whole
// (`droppedHazardLines`); in JSONL a JSON-RPC protocol frame that hits a hazard is never
// dropped silently -- it is reported in `hazardProtocolFrames` and fails the run, because
// live wire traffic carrying such a value is a capture bug to investigate, not to absorb.
//
// After redacting, the output is scanned again: `residualLeaks` are this machine's known
// values (home, username, hostname, caller literals) still present; `residualGenericHits`
// are structural leak shapes still present regardless of whose they are. Reports name a
// rule label and a line number only -- never the matched text -- so a report can be
// printed or committed without re-leaking what it found.
// withholdResiduals() is the backstop for structured output (run manifests): a value that
// still carries a residual hit after redaction is replaced whole by `<WITHHELD: labels>`.
//
// Placeholders follow the ones in the committed G1/D6 fixtures (<USER_HOME>, <HOST>,
// <EMAIL>, <SECRET>), from docs/planning/gates/fixtures/d6-codex-protocol/
// redact.mjs.throwaway-quarantined, which this generalizes to free text.
//
// Username/hostname safety: a short or common name (root, user, runner, ...) is replaced
// only in context (a home path, `name@`, `@host`), never as a bare word, so that clean data
// such as the MCP capability key "roots" is not rewritten when the OS user is `root`.

import { readFileSync, writeFileSync } from 'node:fs';
import { homedir, hostname as osHostname, userInfo } from 'node:os';
import { pathToFileURL } from 'node:url';

export const PLACEHOLDER = {
  home: '<USER_HOME>',
  homeMangled: '<USER_HOME_MANGLED>',
  user: '<USER>',
  host: '<HOST>',
  email: '<EMAIL>',
  secret: '<SECRET>',
  hazard: '<REDACTED_HAZARD_LINE>',
};

const COMMON_NAMES = new Set([
  'root', 'user', 'users', 'admin', 'administrator', 'runner', 'ubuntu', 'debian', 'vagrant',
  'docker', 'node', 'guest', 'test', 'dev', 'build', 'home', 'default', 'app', 'localhost',
  'codespace', 'codespaces', 'runneradmin', 'owner', 'local', 'server', 'client', 'claude',
  'codex', 'agent',
]);

// One path separator as it may appear in raw text or after one or two layers of JSON
// escaping: `/`, `\/`, `\`, `\\`.
const SEP = String.raw`(?:\\{1,2}|\\?/)`;
const SEG_END = String.raw`(?![A-Za-z0-9._-])`;
const NOT_WORD_BEFORE = String.raw`(?<![A-Za-z0-9_])`;
const NOT_WORD_AFTER = String.raw`(?![A-Za-z0-9_])`;

const escapeRe = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

function pathLiteralRe(p) {
  const parts = p.split(/[\\/]+/);
  const body = parts.map(escapeRe).join(SEP);
  return new RegExp(body + SEG_END, /^[A-Za-z]:/.test(p) ? 'gi' : 'g');
}

const distinctive = (name) => typeof name === 'string' && name.length >= 3 && !COMMON_NAMES.has(name.toLowerCase());

// Lines dropped whole. Labels only; nothing matched is ever reported.
const HAZARD_RULES = [
  { label: 'private key block', re: /-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----/ },
  { label: 'authorization header', re: /\b(?:proxy-)?authorization\s*[:=]/i },
  { label: 'cookie header', re: /\b(?:set-)?cookie\s*:/i },
  { label: 'system prompt snapshot', re: /prompt_snapshot|systemPrompt/ },
];

// Token shapes. Each is replaced, and each is also a residual generic check.
const TOKEN_RULES = [
  { label: 'sk- token', re: /\bsk-[A-Za-z0-9_-]{10,}/g, to: PLACEHOLDER.secret },
  { label: 'bearer token', re: /\bBearer\s+(?!<SECRET>)[A-Za-z0-9._~+/-]{8,}=*/g, to: `Bearer ${PLACEHOLDER.secret}` },
  { label: 'github token', re: /\b(?:gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,})/g, to: PLACEHOLDER.secret },
  { label: 'slack token', re: /\bxox[abposr]-[A-Za-z0-9-]{10,}/g, to: PLACEHOLDER.secret },
  { label: 'aws access key id', re: /\b(?:AKIA|ASIA)[0-9A-Z]{16}\b/g, to: PLACEHOLDER.secret },
  { label: 'google api key', re: /\bAIza[0-9A-Za-z_-]{35}/g, to: PLACEHOLDER.secret },
  { label: 'jwt', re: /\beyJ[A-Za-z0-9_-]{8,}\.eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}/g, to: PLACEHOLDER.secret },
  {
    // NAME_TOKEN=value, "NAME_API_KEY": "value", NAME_SECRET: value -- the name is kept.
    label: 'secret assignment',
    re: /\b([A-Z][A-Z0-9_]*(?:_KEY|_TOKEN|_SECRET|PASSWORD|_PASSWD|_PAT))(["']?\s*[:=]\s*)(?!["']?<SECRET>)("[^"\n]*"|'[^'\n]*'|[^\s"',;}]+)/g,
    to: (_m, name, sep, value) => `${name}${sep}${/^["']/.test(value) ? value[0] + PLACEHOLDER.secret + value[0] : PLACEHOLDER.secret}`,
  },
];

const HAZARD_COUNT = 'hazard string replaced';

const EMAIL_RE = /[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}/g;

// Structural home-path shapes, whoever's they are.
const GENERIC_HOME_RULES = [
  { label: 'windows profile path', re: new RegExp(String.raw`[A-Za-z]:${SEP}Users${SEP}(?!<)[^\\/"'<>\s:*?|]+`, 'gi'), to: PLACEHOLDER.home },
  { label: 'unix home path', re: new RegExp(String.raw`(?<![A-Za-z0-9._-])${SEP}(?:home|Users)${SEP}(?!<)[^\\/"'<>\s:]+`, 'g'), to: PLACEHOLDER.home },
  { label: 'root home path', re: new RegExp(String.raw`(?<![A-Za-z0-9._:/-])${SEP}root(?=${SEP}|["'\s]|$)`, 'g'), to: PLACEHOLDER.home },
  { label: 'mangled home path', re: /(?<![A-Za-z0-9])(?:[A-Za-z]--Users|-home|-Users)-(?!<)[A-Za-z0-9._]+/g, to: PLACEHOLDER.homeMangled },
];

const UUID_RE = /[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/gi;
// A path SEGMENT naming a scratchpad or Claude temp/project dir, not the bare word "claude"
// (which sits next to every MCP session id in a Claude Code transcript).
const TEMP_DIR_HINT_RE = /[\\/](?:scratchpad|\.?claude)(?:[\\/]|$)/i;

function safeUserName() {
  try {
    return userInfo().username;
  } catch {
    return process.env.USER || process.env.USERNAME || '';
  }
}

export function createRedactor({ home = homedir(), username = safeUserName(), hostname = osHostname(), literals = [] } = {}) {
  const hostShort = hostname ? hostname.split('.')[0] : '';
  const userWord = distinctive(username);
  const hostWord = distinctive(hostShort);

  // Ordered: most specific first (caller literals such as the scratch dir can sit under home).
  const replaceRules = [];
  for (const { value, placeholder } of [...literals].sort((a, b) => b.value.length - a.value.length)) {
    if (value) replaceRules.push({ label: `literal ${placeholder}`, re: pathLiteralRe(value), to: placeholder });
  }
  if (home && home.length > 1) replaceRules.push({ label: 'home', re: pathLiteralRe(home), to: PLACEHOLDER.home });
  if (username) {
    const u = escapeRe(username);
    replaceRules.push({ label: 'mangled home', re: new RegExp(String.raw`(?:[A-Za-z]--Users|-home|-Users)-${u}${NOT_WORD_AFTER}`, 'gi'), to: PLACEHOLDER.homeMangled });
    replaceRules.push({ label: 'username@', re: new RegExp(String.raw`${NOT_WORD_BEFORE}${u}(?=@)`, 'g'), to: PLACEHOLDER.user });
  }
  if (hostname) {
    for (const h of new Set([hostname, hostShort])) {
      replaceRules.push({ label: '@hostname', re: new RegExp(String.raw`(?<=@)${escapeRe(h)}${NOT_WORD_AFTER}`, 'gi'), to: PLACEHOLDER.host });
    }
  }
  replaceRules.push(...GENERIC_HOME_RULES);
  if (hostname && distinctive(hostname)) {
    replaceRules.push({ label: 'hostname', re: new RegExp(NOT_WORD_BEFORE + escapeRe(hostname) + NOT_WORD_AFTER, 'gi'), to: PLACEHOLDER.host });
  }
  if (hostWord) replaceRules.push({ label: 'hostname', re: new RegExp(NOT_WORD_BEFORE + escapeRe(hostShort) + NOT_WORD_AFTER, 'gi'), to: PLACEHOLDER.host });
  if (userWord) replaceRules.push({ label: 'username', re: new RegExp(NOT_WORD_BEFORE + escapeRe(username) + NOT_WORD_AFTER, 'gi'), to: PLACEHOLDER.user });
  replaceRules.push(...TOKEN_RULES);
  replaceRules.push({ label: 'email', re: EMAIL_RE, to: PLACEHOLDER.email });

  // Residual checks: value-specific ("leaks") and structural ("generic").
  const leakRules = [];
  for (const { value, placeholder } of literals) if (value) leakRules.push({ label: `literal ${placeholder}`, re: pathLiteralRe(value) });
  if (home && home.length > 1) leakRules.push({ label: 'home path', re: pathLiteralRe(home) });
  if (username) {
    leakRules.push({ label: 'username', re: userWord ? new RegExp(NOT_WORD_BEFORE + escapeRe(username) + NOT_WORD_AFTER, 'i') : new RegExp(String.raw`${NOT_WORD_BEFORE}${escapeRe(username)}@|(?:Users|home)${SEP}${escapeRe(username)}${SEG_END}`, 'i') });
  }
  if (hostname) {
    leakRules.push({ label: 'hostname', re: hostWord ? new RegExp(NOT_WORD_BEFORE + escapeRe(hostShort) + NOT_WORD_AFTER, 'i') : new RegExp(String.raw`@${escapeRe(hostShort)}${NOT_WORD_AFTER}`, 'i') });
  }
  const genericRules = [
    ...GENERIC_HOME_RULES.map(({ label, re }) => ({ label, re })),
    ...TOKEN_RULES.map(({ label, re }) => ({ label, re })),
    { label: 'email', re: EMAIL_RE },
    ...HAZARD_RULES,
  ];

  function scrubString(s, counts) {
    let out = s;
    for (const rule of replaceRules) {
      rule.re.lastIndex = 0;
      out = out.replace(rule.re, (...args) => {
        counts[rule.label] = (counts[rule.label] ?? 0) + 1;
        return typeof rule.to === 'function' ? rule.to(...args) : rule.to;
      });
    }
    return out;
  }

  const hazardOf = (line) => HAZARD_RULES.find((r) => r.re.test(line));

  function scan(text) {
    const residualLeaks = [];
    const residualGenericHits = [];
    text.split('\n').forEach((line, i) => {
      for (const r of leakRules) {
        r.re.lastIndex = 0;
        if (r.re.test(line)) residualLeaks.push({ label: r.label, line: i + 1 });
      }
      for (const r of genericRules) {
        const re = new RegExp(r.re.source, r.re.flags.replace('g', ''));
        if (re.test(line)) residualGenericHits.push({ label: r.label, line: i + 1 });
      }
      for (const m of line.matchAll(UUID_RE)) {
        const ctx = line.slice(Math.max(0, m.index - 120), m.index + m[0].length + 120);
        if (TEMP_DIR_HINT_RE.test(ctx)) residualGenericHits.push({ label: 'uuid next to a scratchpad/claude temp path', line: i + 1 });
      }
    });
    return { residualLeaks, residualGenericHits };
  }

  function redactText(text) {
    const counts = {};
    let droppedHazardLines = 0;
    const out = [];
    for (const line of String(text).split('\n')) {
      if (hazardOf(line)) {
        droppedHazardLines += 1;
        continue;
      }
      out.push(scrubString(line, counts));
    }
    const redacted = out.join('\n');
    return { text: redacted, report: { mode: 'text', lines: out.length, droppedHazardLines, hazardProtocolFrames: [], replacements: counts, ...scan(redacted) } };
  }

  function walk(v, counts) {
    if (typeof v === 'string') {
      if (!hazardOf(v)) return scrubString(v, counts);
      counts[HAZARD_COUNT] = (counts[HAZARD_COUNT] ?? 0) + 1;
      return PLACEHOLDER.hazard;
    }
    if (Array.isArray(v)) return v.map((x) => walk(x, counts));
    if (v && typeof v === 'object') {
      const o = {};
      for (const [k, val] of Object.entries(v)) o[scrubString(k, counts)] = walk(val, counts);
      return o;
    }
    return v;
  }

  // Structured values (run manifests): strings are scrubbed in place, a hazard string is
  // replaced by a placeholder (dropping it would change the shape).
  function redactValue(value) {
    const counts = {};
    const redacted = walk(value, counts);
    const text = JSON.stringify(redacted, null, 2);
    return { value: redacted, report: { mode: 'value', droppedHazardLines: counts[HAZARD_COUNT] ?? 0, hazardProtocolFrames: [], replacements: counts, ...scan(text) } };
  }

  // Backstop for structured output: any string (or key) that still carries a residual hit
  // after redaction is replaced whole by `<WITHHELD: labels>`. Returns the JSON paths
  // withheld (paths and labels only, never values).
  function withholdResiduals(value) {
    const withheld = [];
    const hitLabels = (s) => {
      const { residualLeaks, residualGenericHits } = scan(s);
      return [...new Set([...residualLeaks, ...residualGenericHits].map((h) => h.label))];
    };
    const visit = (v, path) => {
      if (typeof v === 'string') {
        const labels = hitLabels(v);
        if (!labels.length) return v;
        withheld.push({ path, labels });
        return `<WITHHELD: ${labels.join(', ')}>`;
      }
      if (Array.isArray(v)) return v.map((x, i) => visit(x, `${path}[${i}]`));
      if (v && typeof v === 'object') {
        const o = {};
        for (const [k, val] of Object.entries(v)) {
          const labels = hitLabels(k);
          const key = labels.length ? `<WITHHELD KEY ${withheld.push({ path: `${path}.<key>`, labels })}>` : k;
          o[key] = visit(val, labels.length ? `${path}.<withheld key>` : `${path}.${k}`);
        }
        return o;
      }
      return v;
    };
    return { value: visit(value, '$'), withheld };
  }

  function isProtocolFrame(rec) {
    return !!rec && typeof rec === 'object' && (rec.jsonrpc === '2.0' || (rec.payload && typeof rec.payload === 'object' && rec.payload.jsonrpc === '2.0'));
  }

  // JSONL: one record per line; a line whose record is unchanged is kept byte-for-byte.
  function redactJsonl(text) {
    const counts = {};
    let droppedHazardLines = 0;
    const hazardProtocolFrames = [];
    const out = [];
    String(text)
      .split('\n')
      .forEach((line, i) => {
        if (line.trim() === '') return;
        let rec;
        let parsed = true;
        try {
          rec = JSON.parse(line);
        } catch {
          parsed = false;
        }
        if (hazardOf(line)) {
          if (parsed && isProtocolFrame(rec)) hazardProtocolFrames.push({ line: i + 1, method: rec.method ?? rec.payload?.method ?? null });
          else droppedHazardLines += 1;
          return;
        }
        if (!parsed) {
          out.push(scrubString(line, counts));
          return;
        }
        const before = Object.values(counts).reduce((a, b) => a + b, 0);
        const walked = walk(rec, counts);
        const after = Object.values(counts).reduce((a, b) => a + b, 0);
        out.push(after === before ? line : JSON.stringify(walked));
      });
    const redacted = out.length ? `${out.join('\n')}\n` : '';
    return { text: redacted, report: { mode: 'jsonl', lines: out.length, droppedHazardLines, hazardProtocolFrames, replacements: counts, ...scan(redacted) } };
  }

  return {
    redactText,
    redactValue,
    redactJsonl,
    withholdResiduals,
    scan,
    identity: { usernameMode: userWord ? 'word' : 'context-only', hostnameMode: hostWord ? 'word' : 'context-only' },
  };
}

export function reportIsClean(report) {
  return report.residualLeaks.length === 0 && report.residualGenericHits.length === 0 && report.hazardProtocolFrames.length === 0;
}

// One-line summary; carries counts and labels only.
export function summarize(report) {
  const labels = (hits) => [...new Set(hits.map((h) => h.label))].join(', ');
  return (
    `droppedHazardLines=${report.droppedHazardLines} hazardProtocolFrames=${report.hazardProtocolFrames.length} ` +
    `residualLeaks=${report.residualLeaks.length}${report.residualLeaks.length ? ` [${labels(report.residualLeaks)}]` : ''} ` +
    `residualGenericHits=${report.residualGenericHits.length}${report.residualGenericHits.length ? ` [${labels(report.residualGenericHits)}]` : ''}`
  );
}

// --- CLI ---------------------------------------------------------------------------

function main(argv) {
  const opts = { literals: [] };
  const positional = [];
  let check = false;
  let mode = null;
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--check') check = true;
    else if (a === '--jsonl') mode = 'jsonl';
    else if (a === '--text') mode = 'text';
    else if (a === '--username') opts.username = argv[++i];
    else if (a === '--hostname') opts.hostname = argv[++i];
    else if (a === '--home') opts.home = argv[++i];
    else if (a === '--literal') {
      const [value, placeholder] = String(argv[++i]).split(/=(?=<)/);
      opts.literals.push({ value, placeholder });
    } else positional.push(a);
  }
  const [inPath, outPath] = positional;
  if (!inPath || (!check && !outPath)) {
    console.error('usage: node tools/herdr/lib/redact.mjs --check <file> | <in> <out>  [--jsonl|--text] [--username U] [--hostname H] [--home P] [--literal VALUE=<PLACEHOLDER>]');
    return 2;
  }
  mode ??= /\.jsonl$/i.test(inPath) ? 'jsonl' : 'text';
  const r = createRedactor(opts);
  const input = readFileSync(inPath, 'utf8');
  let report;
  if (check) {
    const hazards = input.split('\n').filter((l) => HAZARD_RULES.some((h) => h.re.test(l))).length;
    report = { mode: `${mode} (check only)`, droppedHazardLines: 0, hazardLinesPresent: hazards, hazardProtocolFrames: [], replacements: {}, ...r.scan(input) };
  } else {
    const res = mode === 'jsonl' ? r.redactJsonl(input) : r.redactText(input);
    report = res.report;
    if (res.report.hazardProtocolFrames.length === 0) writeFileSync(outPath, res.text);
  }
  console.log(summarize(report));
  console.log(JSON.stringify({ ...report, identity: r.identity }));
  const clean = reportIsClean(report) && !(check && report.hazardLinesPresent);
  return clean ? 0 : 1;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  process.exit(main(process.argv.slice(2)));
}
