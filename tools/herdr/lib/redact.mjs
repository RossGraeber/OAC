#!/usr/bin/env node
// Redaction for everything the herdr driver captures (pane text, run manifests, JSONL
// transcripts) before it can leave the run's scratch directory.
//
//   node tools/herdr/lib/redact.mjs --check <file>             # scan only; exit 1 on any residual
//   node tools/herdr/lib/redact.mjs <in> <out>                 # redact <in> into <out>
//   options: --jsonl | --text (default: by extension), --username U, --hostname H,
//            --home PATH, --literal VALUE=<PLACEHOLDER> (repeatable; e.g. an installation id)
//
// What it strips:
//   - home paths: this machine's, and any /home/<x>, /Users/<x>, C:\Users\<x>, WSL
//     /mnt/<d>/Users/<x>, /root, Claude Code's mangled project-dir form; raw, JSON-escaped,
//     or percent-encoded;
//   - the OS username and hostname (see "identity" below), and caller literals;
//   - e-mail addresses and token shapes (sk-, Bearer, GitHub, GitLab, npm, Slack, AWS,
//     Google, JWT);
//   - secret-named assignments in any case or spelling -- NAME_TOKEN=, "access_token": ,
//     apiKey: , aws_secret_access_key = , x-api-key: , --api-key <v>, bare TOKEN= -- the
//     name is kept, the value becomes <SECRET>; in structured data the same names are
//     matched as object keys.
// Dropped whole (`droppedHazardLines`): a private-key block from its BEGIN line through its
// END line (an unterminated block drops everything after BEGIN -- fail closed), a bare line
// of base64 key material, an Authorization/Cookie header line (raw or JSON-shaped), and a
// system-prompt snapshot. In JSONL, a JSON-RPC protocol frame that carries a hazard is
// never dropped silently: it is reported in `hazardProtocolFrames` and fails the run, since
// live wire traffic carrying such a value is a capture bug to investigate, not to absorb.
//
// After redacting, the output is scanned again: `residualLeaks` are this machine's known
// values (home, username, hostname, caller literals) still present; `residualGenericHits`
// are structural leak shapes still present regardless of whose they are. Reports name a
// rule label and a line number only -- never the matched text -- so a report can be
// printed or committed without re-leaking what it found. withholdResiduals() is the
// backstop for structured output (run manifests): a value that still carries a residual
// hit after redaction is replaced whole by `<WITHHELD: labels>`.
//
// Placeholders follow the ones in the committed G1/D6 fixtures (<USER_HOME>, <HOST>,
// <EMAIL>, <SECRET>), from docs/planning/gates/fixtures/d6-codex-protocol/
// redact.mjs.throwaway-quarantined, which this generalizes to free text.
//
// Identity matching: a username/hostname of 4+ characters that is not a common name is
// replaced wherever it occurs, as a substring (so `rossg_dev`, `xrossg`, `Ross-MBP_2`
// cannot slip past a word boundary); a 3-character one as a whole word; a common or
// shorter one (root, user, runner, vm, ...) only in context -- a home path, `name@`
// (including right after an ANSI colour code), `@host` -- so that clean data such as the
// MCP capability key "roots" is not rewritten when the OS user is `root`.

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
  'codex', 'agent', 'host', 'main', 'work', 'linux', 'macbook', 'desktop', 'laptop',
]);

// One path separator as it may appear raw, after one or two layers of JSON escaping, or
// percent-encoded: `/`, `\/`, `\`, `\\`, `%2F`, `%5C`.
const SEP = String.raw`(?:\\{1,2}|\\?/|%2[Ff]|%5[Cc])`;
const DRIVE = String.raw`[A-Za-z](?::|%3[Aa])`;
const SEG_END = String.raw`(?![A-Za-z0-9._-])`;
const SEG_CHARS = String.raw`[^\\/"'<>\s:*?|%]+`;
const NOT_WORD_BEFORE = String.raw`(?<![A-Za-z0-9_])`;
const NOT_WORD_AFTER = String.raw`(?![A-Za-z0-9_])`;
// Start of a token: not preceded by a word character, or preceded by an ANSI SGR sequence.
const TOKEN_START = String.raw`(?:(?<=\x1b\[[0-9;]*m)|(?<![A-Za-z0-9_]))`;

const escapeRe = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

function pathLiteralRe(p) {
  const parts = p.split(/[\\/]+/);
  const drive = /^[A-Za-z]:$/.test(parts[0]);
  const body = parts.map((part, i) => (i === 0 && drive ? `${escapeRe(part[0])}(?::|%3[Aa])` : escapeRe(part))).join(SEP);
  return new RegExp(body + SEG_END, drive ? 'gi' : 'g');
}

// Caller-supplied literals (scratch dir, run ids, installation ids): matched
// case-insensitively and with NO trailing boundary, so a literal embedded in a longer token
// (`rollout-...-<uuid>.jsonl`, `ref_<uuid>_x`, `<uuid>-rollout`, an upper-cased copy) is
// still replaced. Separators inside a path-shaped literal accept every escaped spelling.
function callerLiteralRe(value) {
  const parts = value.split(/[\\/]+/);
  const drive = /^[A-Za-z]:$/.test(parts[0]);
  const body = parts.map((part, i) => (i === 0 && drive ? `${escapeRe(part[0])}(?::|%3[Aa])` : escapeRe(part))).join(SEP);
  return new RegExp(body, 'gi');
}

function identityMode(name) {
  if (typeof name !== 'string' || !name || COMMON_NAMES.has(name.toLowerCase())) return 'context-only';
  if (name.length >= 4) return 'substring';
  if (name.length === 3) return 'word';
  return 'context-only';
}
function identityRe(name, mode, flags) {
  if (mode === 'substring') return new RegExp(escapeRe(name), flags);
  if (mode === 'word') return new RegExp(TOKEN_START + escapeRe(name) + NOT_WORD_AFTER, flags);
  return null;
}

// --- secret-named assignments ----------------------------------------------------------

const SECRET_SUFFIX = String.raw`(?:api[_-]?key|access[_-]?key(?:[_-]?id)?|secret[_-]?(?:access[_-]?)?key|client[_-]?secret|(?:access|refresh|id|auth|session|bearer|api|oauth)[_-]?token|private[_-]?key|password|passwd|secret|token|credentials?|_pat)`;
const SECRET_NAME_ONLY = new RegExp(String.raw`^(?:--?)?(?:[A-Za-z][A-Za-z0-9_.-]*?)?${SECRET_SUFFIX}$`, 'i');
const HEADER_NAME_ONLY = /^(?:proxy-)?authorization$|^(?:set-)?cookie$/i;
// Names that end like a secret but are protocol bookkeeping, not credentials.
const BENIGN_NAMES = new Set(['progresstoken', 'pagetoken', 'nextpagetoken', 'continuationtoken', 'cursortoken', 'maxtoken', 'tokentype', 'token_type']);
const ASSIGNMENT_RE = new RegExp(
  String.raw`(?<![A-Za-z0-9_.-])(--?)?((?:[A-Za-z][A-Za-z0-9_.-]*?)?${SECRET_SUFFIX})(\\?["']?)(\s*[:=]\s*|\s+)(?!\\?["']?<)(\\"(?:[^"\\\n]|\\[^"])*\\"|"[^"\n]*"|'[^'\n]*'|[^\s"'\\,;}\]]+)`,
  'gi',
);

function secretValueLike(v) {
  const inner = String(v).replace(/^\\?["']|\\?["']$/g, '');
  return inner.length >= 6 && !/^\d+$/.test(inner) && !/^(?:true|false|null|none|undefined)$/i.test(inner) && !inner.startsWith('<') && !/^\$\{?[A-Za-z_]/.test(inner);
}

function assignmentMatches(s) {
  const out = [];
  for (const m of s.matchAll(ASSIGNMENT_RE)) {
    const [, dash, name, , sep, value] = m;
    if (BENIGN_NAMES.has(name.toLowerCase())) continue;
    if (!/[:=]/.test(sep) && !dash) continue; // whitespace separator only for --flag value
    if (!secretValueLike(value)) continue;
    out.push({ index: m.index, length: m[0].length, keep: m[0].slice(0, m[0].length - value.length), value });
  }
  return out;
}

const SECRET_ASSIGNMENT_RULE = {
  label: 'secret assignment',
  apply(s, counts) {
    const hits = assignmentMatches(s);
    if (!hits.length) return s;
    let out = '';
    let pos = 0;
    for (const h of hits) {
      const q = (/^\\?["']/.exec(h.value) ?? [''])[0];
      out += s.slice(pos, h.index) + h.keep + q + PLACEHOLDER.secret + q;
      pos = h.index + h.length;
    }
    counts[this.label] = (counts[this.label] ?? 0) + hits.length;
    return out + s.slice(pos);
  },
  detect: (line) => assignmentMatches(line).length > 0,
};

// --- rules -------------------------------------------------------------------------------

function reRule(label, re, to) {
  return {
    label,
    re,
    apply(s, counts) {
      re.lastIndex = 0;
      return s.replace(re, (...args) => {
        counts[label] = (counts[label] ?? 0) + 1;
        return typeof to === 'function' ? to(...args) : to;
      });
    },
    detect(line) {
      return new RegExp(re.source, re.flags.replace('g', '')).test(line);
    },
  };
}

const KEY_BEGIN = /-----BEGIN [A-Z0-9 ]*PRIVATE KEY(?: BLOCK)?-----/;
const KEY_END = /-----END [A-Z0-9 ]*PRIVATE KEY(?: BLOCK)?-----/;

// Lines dropped whole. Labels only; nothing matched is ever reported.
const HAZARD_RULES = [
  { label: 'private key block', re: KEY_BEGIN },
  { label: 'private key block end', re: KEY_END },
  // A line that is nothing but long mixed-case base64: key material outside its markers.
  { label: 'base64 key material line', re: /^\s*(?=[A-Za-z0-9+/]*[A-Z])(?=[A-Za-z0-9+/]*[a-z])(?=[A-Za-z0-9+/]*[0-9])[A-Za-z0-9+/]{64,}={0,2}\s*$/ },
  // Header name, optional closing quote (JSON), separator, then a value that is not already a placeholder.
  // Also JSON-escaped (\"Authorization\":...) and header tuples (["authorization","Basic ..."]).
  { label: 'authorization header', re: /\b(?:proxy-)?authorization(?:\\?["']\s*,|\\?["']?\s*[:=])\s*\\?["']?(?!<SECRET>)[^\s"'\\,}\]]/i },
  { label: 'cookie header', re: /\b(?:set-)?cookie(?:\\?["']\s*,|\\?["']?\s*:)\s*\\?["']?(?!<SECRET>)[^\s"'\\,}\]]/i },
  { label: 'system prompt snapshot', re: /prompt_snapshot|systemPrompt/ },
];

const TOKEN_RULES = [
  reRule('sk- token', /\bsk-[A-Za-z0-9_-]{10,}/g, PLACEHOLDER.secret),
  reRule('bearer token', /\bBearer\s+(?!<SECRET>)[A-Za-z0-9._~+/-]{8,}=*/g, `Bearer ${PLACEHOLDER.secret}`),
  reRule('github token', /\b(?:gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,})/g, PLACEHOLDER.secret),
  reRule('gitlab token', /\bgl(?:pat|ptt|dt)-[A-Za-z0-9_-]{20,}/g, PLACEHOLDER.secret),
  reRule('npm token', /\bnpm_[A-Za-z0-9]{36}\b/g, PLACEHOLDER.secret),
  reRule('slack token', /\bxox[abposr]-[A-Za-z0-9-]{10,}/g, PLACEHOLDER.secret),
  reRule('aws access key id', /\b(?:AKIA|ASIA)[0-9A-Z]{16}\b/g, PLACEHOLDER.secret),
  reRule('google api key', /\bAIza[0-9A-Za-z_-]{35}/g, PLACEHOLDER.secret),
  reRule('jwt', /\beyJ[A-Za-z0-9_-]{8,}\.eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}/g, PLACEHOLDER.secret),
  SECRET_ASSIGNMENT_RULE,
];

const HAZARD_COUNT = 'hazard string replaced';
const SECRET_KEY_COUNT = 'secret-named key value';

const EMAIL_RE = /[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}/g;

// Structural home-path shapes, whoever's they are.
const GENERIC_HOME_RULES = [
  reRule('wsl windows profile path', new RegExp(String.raw`(?<![A-Za-z0-9._-])${SEP}mnt${SEP}[A-Za-z]${SEP}Users${SEP}(?!<)${SEG_CHARS}`, 'gi'), PLACEHOLDER.home),
  reRule('windows profile path', new RegExp(String.raw`${DRIVE}${SEP}Users${SEP}(?!<)${SEG_CHARS}`, 'gi'), PLACEHOLDER.home),
  reRule('unix home path', new RegExp(String.raw`(?<![A-Za-z0-9._-])${SEP}(?:home|Users)${SEP}(?!<)${SEG_CHARS}`, 'g'), PLACEHOLDER.home),
  reRule('root home path', new RegExp(String.raw`(?<![A-Za-z0-9._:/%-])${SEP}root(?=${SEP}|["'\s]|$)`, 'g'), PLACEHOLDER.home),
  reRule('mangled home path', /(?<![A-Za-z0-9])(?:[A-Za-z]--Users|-home|-Users)-(?!<)[A-Za-z0-9._]+/g, PLACEHOLDER.homeMangled),
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

export function isSecretKeyName(k) {
  return (SECRET_NAME_ONLY.test(k) || HEADER_NAME_ONLY.test(k)) && !BENIGN_NAMES.has(String(k).toLowerCase());
}

export function createRedactor({ home = homedir(), username = safeUserName(), hostname = osHostname(), literals = [] } = {}) {
  const hostShort = hostname ? hostname.split('.')[0] : '';
  const userMode = identityMode(username);
  const hostMode = identityMode(hostShort);
  const lits = literals.filter((l) => l && l.value);

  // Ordered: most specific first (caller literals such as the scratch dir can sit under home).
  const replaceRules = [];
  for (const { value, placeholder } of [...lits].sort((a, b) => b.value.length - a.value.length)) {
    replaceRules.push(reRule(`literal ${placeholder}`, callerLiteralRe(value), placeholder));
  }
  if (home && home.length > 1) replaceRules.push(reRule('home', pathLiteralRe(home), PLACEHOLDER.home));
  if (username) {
    const u = escapeRe(username);
    replaceRules.push(reRule('mangled home', new RegExp(String.raw`(?:[A-Za-z]--Users|-home|-Users)-${u}${NOT_WORD_AFTER}`, 'gi'), PLACEHOLDER.homeMangled));
    replaceRules.push(reRule('username@', new RegExp(String.raw`${TOKEN_START}${u}(?=@)`, 'gi'), PLACEHOLDER.user));
  }
  if (hostname) {
    for (const h of new Set([hostname, hostShort])) {
      replaceRules.push(reRule('@hostname', new RegExp(String.raw`(?<=@)${escapeRe(h)}${NOT_WORD_AFTER}`, 'gi'), PLACEHOLDER.host));
    }
  }
  replaceRules.push(...GENERIC_HOME_RULES);
  // Full hostname before its short form, so `box.corp.example` goes in one piece.
  if (hostname && hostname !== hostShort && identityMode(hostname) !== 'context-only') {
    replaceRules.push(reRule('hostname', identityRe(hostname, identityMode(hostname), 'gi'), PLACEHOLDER.host));
  }
  if (hostMode !== 'context-only') replaceRules.push(reRule('hostname', identityRe(hostShort, hostMode, 'gi'), PLACEHOLDER.host));
  if (userMode !== 'context-only') replaceRules.push(reRule('username', identityRe(username, userMode, 'gi'), PLACEHOLDER.user));
  replaceRules.push(...TOKEN_RULES);
  replaceRules.push(reRule('email', EMAIL_RE, PLACEHOLDER.email));

  // Residual checks: value-specific ("leaks") and structural ("generic").
  const leakRules = [];
  // Fail closed for caller literals: a plain case-insensitive substring test, plus the
  // separator-tolerant form for path-shaped ones.
  for (const { value, placeholder } of lits) {
    const lower = value.toLowerCase();
    const re = callerLiteralRe(value);
    leakRules.push({ label: `literal ${placeholder}`, re: { lastIndex: 0, test: (line) => line.toLowerCase().includes(lower) || new RegExp(re.source, 'i').test(line) } });
  }
  if (home && home.length > 1) leakRules.push({ label: 'home path', re: pathLiteralRe(home) });
  if (username) {
    const u = escapeRe(username);
    leakRules.push({ label: 'username', re: identityRe(username, userMode, 'i') ?? new RegExp(String.raw`${TOKEN_START}${u}@|(?:Users|home)${SEP}${u}${SEG_END}`, 'i') });
  }
  if (hostname) {
    leakRules.push({ label: 'hostname', re: identityRe(hostShort, hostMode, 'i') ?? new RegExp(String.raw`@${escapeRe(hostShort)}${NOT_WORD_AFTER}`, 'i') });
  }
  const genericRules = [
    ...GENERIC_HOME_RULES,
    ...TOKEN_RULES,
    reRule('email', EMAIL_RE, PLACEHOLDER.email),
    ...HAZARD_RULES.map(({ label, re }) => ({ label, detect: (line) => re.test(line) })),
  ];

  const scrubString = (s, counts) => replaceRules.reduce((acc, rule) => rule.apply(acc, counts), s);
  const hazardOf = (line) => HAZARD_RULES.find((r) => r.re.test(line));

  function scan(text) {
    const residualLeaks = [];
    const residualGenericHits = [];
    String(text).split('\n').forEach((line, i) => {
      for (const r of leakRules) {
        r.re.lastIndex = 0;
        if (r.re.test(line)) residualLeaks.push({ label: r.label, line: i + 1 });
      }
      for (const r of genericRules) if (r.detect(line)) residualGenericHits.push({ label: r.label, line: i + 1 });
      for (const m of line.matchAll(UUID_RE)) {
        const ctx = line.slice(Math.max(0, m.index - 120), m.index + m[0].length + 120);
        if (TEMP_DIR_HINT_RE.test(ctx)) residualGenericHits.push({ label: 'uuid next to a scratchpad/claude temp path', line: i + 1 });
      }
    });
    return { residualLeaks, residualGenericHits };
  }

  // Line filter shared by text and JSONL modes: drops a private-key block as one unit, from
  // BEGIN through END (an unterminated block drops everything after BEGIN), plus hazard
  // lines. `lineHandler` gets the lines that survive.
  function filterLines(text, lineHandler) {
    let inKey = false;
    let dropped = 0;
    let unterminatedKeyBlock = false;
    String(text).split('\n').forEach((line, i) => {
      if (inKey) {
        dropped += 1;
        if (KEY_END.test(line)) inKey = false;
        return;
      }
      const begin = KEY_BEGIN.exec(line);
      if (begin && !KEY_END.test(line.slice(begin.index))) {
        // Multi-line block. (A block wholly inside one line -- JSON-escaped -- is a hazard line.)
        inKey = true;
        dropped += 1;
        return;
      }
      if (lineHandler(line, i) === 'dropped') dropped += 1;
    });
    if (inKey) unterminatedKeyBlock = true;
    return { dropped, unterminatedKeyBlock };
  }

  function redactText(text) {
    const counts = {};
    const out = [];
    const { dropped, unterminatedKeyBlock } = filterLines(text, (line) => {
      if (hazardOf(line)) return 'dropped';
      out.push(scrubString(line, counts));
      return 'kept';
    });
    const redacted = out.join('\n');
    return { text: redacted, report: { mode: 'text', lines: out.length, droppedHazardLines: dropped, unterminatedKeyBlock, hazardProtocolFrames: [], replacements: counts, ...scan(redacted) } };
  }

  function walk(v, counts, key = null) {
    if (typeof v === 'string') {
      if (hazardOf(v) || KEY_END.test(v)) {
        counts[HAZARD_COUNT] = (counts[HAZARD_COUNT] ?? 0) + 1;
        return PLACEHOLDER.hazard;
      }
      if (key !== null && isSecretKeyName(key) && (HEADER_NAME_ONLY.test(key) ? v.length > 0 : secretValueLike(v))) {
        counts[SECRET_KEY_COUNT] = (counts[SECRET_KEY_COUNT] ?? 0) + 1;
        return PLACEHOLDER.secret;
      }
      return scrubString(v, counts);
    }
    // A [name, value] tuple (raw HTTP header lists): the name keys the value.
    if (Array.isArray(v) && v.length === 2 && typeof v[0] === 'string' && typeof v[1] === 'string' && isSecretKeyName(v[0])) {
      return [scrubString(v[0], counts), walk(v[1], counts, v[0])];
    }
    if (Array.isArray(v)) return v.map((x) => walk(x, counts, key));
    if (v && typeof v === 'object') {
      const o = {};
      for (const [k, val] of Object.entries(v)) o[scrubString(k, counts)] = walk(val, counts, k);
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
  // Records are redacted by value and by key (secret-named keys, header names); a record
  // that still carries a hazard afterwards is dropped, or -- if it is a protocol frame --
  // reported in hazardProtocolFrames. Non-JSON lines get the text-mode treatment,
  // including private-key blocks dropped BEGIN through END.
  function redactJsonl(text) {
    const counts = {};
    const hazardProtocolFrames = [];
    const out = [];
    const { dropped, unterminatedKeyBlock } = filterLines(text, (line, i) => {
      if (line.trim() === '') return 'kept';
      let rec;
      try {
        rec = JSON.parse(line);
      } catch {
        if (hazardOf(line)) return 'dropped';
        out.push(scrubString(line, counts));
        return 'kept';
      }
      const local = {};
      const walked = walk(rec, local);
      const changed = Object.keys(local).length > 0;
      const serialized = changed ? JSON.stringify(walked) : line;
      if (local[HAZARD_COUNT] || hazardOf(serialized)) {
        if (isProtocolFrame(rec)) {
          hazardProtocolFrames.push({ line: i + 1, method: rec.method ?? rec.payload?.method ?? null });
          return 'reported';
        }
        return 'dropped';
      }
      for (const [k, n] of Object.entries(local)) counts[k] = (counts[k] ?? 0) + n;
      out.push(serialized);
      return 'kept';
    });
    const redacted = out.length ? `${out.join('\n')}\n` : '';
    return { text: redacted, report: { mode: 'jsonl', lines: out.length, droppedHazardLines: dropped, unterminatedKeyBlock, hazardProtocolFrames, replacements: counts, ...scan(redacted) } };
  }

  return {
    redactText,
    redactValue,
    redactJsonl,
    withholdResiduals,
    scan,
    identity: { usernameMode: userMode, hostnameMode: hostMode },
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

// `VALUE=<PLACEHOLDER>` -> { value, placeholder }; the placeholder must look like <UPPER_SNAKE>.
export function parseLiteralSpec(spec) {
  const m = /^(.+)=(<[A-Z][A-Z0-9_]*>)$/.exec(String(spec));
  if (!m) throw new Error('a redaction literal is VALUE=<PLACEHOLDER>, e.g. 0b5e...=<INSTALLATION_ID>');
  return { value: m[1], placeholder: m[2] };
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
    else if (a === '--literal') opts.literals.push(parseLiteralSpec(argv[++i]));
    else positional.push(a);
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
