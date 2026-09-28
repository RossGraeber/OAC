// Shared pieces of the scripted G4 re-run (Epic K, K8 #131): the verbatim Claude launch, the
// per-invocation Codex MCP registration and its validation, the reconstructed gate server,
// the human-run baseline, the facts read from a G4 server transcript, and the fixture
// sanitizer.
//
// NOT VERDICT-BEARING, and the server is a RECONSTRUCTION: the original G4 spike server was
// never committed (docs/planning/gates/G4-result.md), so tools/herdr/gate-servers/
// g4-server.mjs is rebuilt from G4-result.md and the committed fixture. A comparison against
// the human run therefore compares against a run of a DIFFERENT server binary that is
// believed, not proven, to behave the same.

import { readPinnedCriteria, assertCriteriaPin } from './gate-common.mjs';

// G4's Claude launch, verbatim (G4-result.md "Command transcript summary").
export const G4_LAUNCH = Object.freeze(['claude', '--dangerously-load-development-channels', 'server:g4spike', 'server:g4modern']);
export const G4_SERVER_FILES = Object.freeze(['g4-server.mjs']);
export const FIXTURE_DIR = 'docs/planning/gates/fixtures/g4-mcp-dual-era';
export const BASELINE_TRANSCRIPT = `${FIXTURE_DIR}/transcript-2026-09-26.jsonl`;
export const HERDR_RUNS_DIR = 'docs/planning/gates/herdr-runs';
export const PINS_PATH = 'docs/planning/PINS.md';
export const MANIFEST_PATH = 'docs/planning/gates/fixtures/MANIFEST.json';

export const MODERN = '2026-07-28';
export const LEGACY = '2025-11-25';
export const PV_KEY = 'io.modelcontextprotocol/protocolVersion';
// The public OAC MCP extension identifier the server puts its `_meta` provenance under.
export const OAC_EXT = 'io.github.rossgraeber/oac-session-channels';
// In captures the identifier is replaced by this placeholder BEFORE run.mjs's identity
// redaction: that redaction rewrites any substring of the operator's OS username, and the
// identifier's owner segment can contain one (G4-result.md's own redaction found exactly this
// string family). The sanitizer counts the replacements; the report reads the placeholder as
// the identifier.
export const OAC_EXT_PLACEHOLDER = '<OAC_EXTENSION_ID>';

export const G4_REFERENCE = '.claude/skills/oac-gates/references/G4-mcp-dual-era.md';
// sha256 of JSON.stringify(<the five parsed G4 criteria>) as committed when K8 was written.
// lib/g4-report.mjs is written against exactly these five, in this order.
export const G4_CRITERIA_SHA256 = 'f2bebc54d2fb04af969f9b8c1b396f68d359e3e7fbb7447baf5963abb291c6b1';
export const readG4Criteria = (repoRoot, { pin = G4_CRITERIA_SHA256 } = {}) => readPinnedCriteria(repoRoot, { path: G4_REFERENCE, count: 5, pin, owner: 'K8 (lib/g4-report.mjs, G4_CRITERIA_SHA256 in lib/g4.mjs)' });
export const assertG4Criteria = (criteria) => assertCriteriaPin(criteria, G4_CRITERIA_SHA256, 'lib/g4-report.mjs');

export const DEFAULT_PORTS = Object.freeze({ httpPort: 17448, modernHttpPort: 17450 });

// The project .mcp.json the Claude pane uses: the three registrations G4-result.md's `/mcp`
// paste lists (g4spike, g4modern, g4http), pointed at the staged server copy.
export function g4McpJson({ node, serverPath, httpPort, modernHttpPort }) {
  return {
    mcpServers: {
      g4spike: { command: node, args: [serverPath], env: { G4_HTTP_PORT: String(httpPort) } },
      g4modern: { command: node, args: [serverPath], env: { G4_STDIO_MODERN: '1', G4_HTTP_PORT: String(modernHttpPort) } },
      g4http: { type: 'http', url: `http://127.0.0.1:${httpPort}/mcp` },
    },
  };
}

// --- the Codex launch: per-invocation configuration only -----------------------------------
//
// The human run registered Codex's connection to the server in the operator's GLOBAL Codex
// config (G4-result.md, criterion 2). K8's acceptance forbids that: the scripted run passes
// the registration per invocation (`-c key=value`), never edits the operator's config, and
// never copies the Codex home. A project-scoped `.codex/config.toml` would also satisfy the
// acceptance, but the containment lint (oac-boundaries check 10) cannot tell a scratch
// project's file from the operator's, so it is not used.

export const defaultCodexLaunch = (httpPort) => ['codex', '-c', `mcp_servers.g4http.url="http://127.0.0.1:${httpPort}/mcp"`];
const ALLOWED_OVERRIDE = /^(?:mcp_servers\.[A-Za-z0-9_-]+\.(?:url|enabled|startup_timeout_sec|tool_timeout_sec)|features\.[A-Za-z0-9_]+)$/;

// -> { ok, why, overrides: [{ key, value }] }
export function validateCodexLaunch(argv) {
  if (!Array.isArray(argv) || argv[0] !== 'codex') return { ok: false, why: 'the Codex launch must start with the herdr agent kind `codex`', overrides: [] };
  const overrides = [];
  for (let i = 1; i < argv.length; i++) {
    const a = argv[i];
    if (a !== '-c' && a !== '--config') return { ok: false, why: `argument ${JSON.stringify(a)} is not a per-invocation \`-c key=value\` override`, overrides };
    const kv = argv[++i];
    const eq = typeof kv === 'string' ? kv.indexOf('=') : -1;
    if (eq < 1) return { ok: false, why: `\`${a}\` needs a key=value`, overrides };
    const key = kv.slice(0, eq).trim();
    if (!ALLOWED_OVERRIDE.test(key)) return { ok: false, why: `override key ${JSON.stringify(key)} is not an MCP-server url/enabled/timeout key or a feature flag; nothing else may be overridden`, overrides };
    overrides.push({ key, value: kv.slice(eq + 1) });
  }
  if (!overrides.some((o) => /^mcp_servers\.[^.]+\.url$/.test(o.key))) return { ok: false, why: 'no per-invocation MCP server url (`-c mcp_servers.<name>.url="..."`)', overrides };
  return { ok: true, why: null, overrides };
}

// Env handed to the Claude pane: only non-credential names, never a harness home.
export function validatePaneEnv(env) {
  for (const k of Object.keys(env ?? {})) {
    if (!/^[A-Z][A-Z0-9_]*$/.test(k)) return `env name ${JSON.stringify(k)} is not an upper-case variable name`;
    if (/KEY|TOKEN|SECRET|PASSW|AUTH|CREDENTIAL|COOKIE|SESSION/.test(k)) return `env name ${k} looks credential-bearing; refused`;
    if (/^(?:HOME|USERPROFILE|CLAUDE_CONFIG_DIR|CODEX_HOME|PATH|NODE_OPTIONS)$/.test(k)) return `env name ${k} would redirect a harness home or the toolchain; refused`;
  }
  return null;
}

export const DEFAULT_PROMPTS = Object.freeze({
  claudeEchoPrompt: 'Call the g4_echo tool from the g4http MCP server with the text "hello from claude over http", and show me the full result including _meta.',
  codexToolsPrompt: 'Call the g4_echo tool with the text "hello from codex through herdr" and print its result. Then call the g4_relay_to_claude tool with the text "codex relay during modern session" and print its result.',
  claudeEchoAfterPrompt: 'Call the g4_echo tool from the g4http MCP server again, with the text "modern still healthy after codex traffic", and show me the full result.',
});

// No operator prompt may carry a channel notification or a wake: those come only from the
// server (its wake trigger, or the relay tool Codex calls).
export function assertNotInjected(label, text) {
  if (/notifications\/claude\/channel|G4 wake test|relayed from http|oac_message_id|<channel\b/i.test(String(text))) {
    throw new Error(`${label} looks like it carries a channel notification; notifications come only from the G4 server (its wake trigger or its relay tool)`);
  }
}

// --- fixture names --------------------------------------------------------------------------

const V = /^\d+\.\d+\.\d+$/;
export function fixtureNames(date, claude, codex) {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(date)) throw new Error(`bad fixture date ${JSON.stringify(date)}`);
  if (!V.test(claude) || !V.test(codex)) throw new Error(`bad harness versions ${JSON.stringify({ claude, codex })}`);
  return { transcript: `transcript-${date}-claude-${claude}-codex-${codex}-herdr.jsonl`, paneClaude: `pane-claude-${date}-${claude}-herdr.txt`, paneCodex: `pane-codex-${date}-${codex}-herdr.txt` };
}
export function unverifiedNames(date) {
  return { transcript: `unverified-transcript-${date}-herdr.jsonl`, paneClaude: `unverified-pane-claude-${date}-herdr.txt`, paneCodex: `unverified-pane-codex-${date}-herdr.txt` };
}

// --- the server transcript -------------------------------------------------------------------

export function parseG4Transcript(text, { completeLinesOnly = false } = {}) {
  let s = String(text ?? '');
  if (completeLinesOnly) s = s.slice(0, s.lastIndexOf('\n') + 1);
  const out = [];
  s.split('\n').forEach((raw, i) => {
    if (!raw.trim()) return;
    let e;
    try {
      e = JSON.parse(raw);
    } catch {
      throw new Error(`G4 transcript line ${i + 1} is not JSON`);
    }
    out.push({ ...e, line: i + 1 });
  });
  return out;
}

// The public identifier, or its placeholder, as the `_meta` key of a tools/call result.
export const provenanceOf = (result) => result?._meta?.[OAC_EXT] ?? result?._meta?.[OAC_EXT_PLACEHOLDER] ?? null;
const uaVersion = (ua, name) => {
  const m = new RegExp(`^${name}/v?(\\d+\\.\\d+\\.\\d+)(?=$|[\\s(])`).exec(String(ua ?? ''));
  return m ? m[1] : null;
};

// Everything the scenario and the report read from a G4 server transcript.
export function g4Facts(entries) {
  const f = {
    instances: [],
    listening: [],
    listenErrors: [],
    armed: [],
    stdioDiscover: [],
    stdioInitialize: [],
    stdioToolsList: [],
    httpDiscover: [],
    httpToolsList: [],
    httpInitialize: [],
    toolCalls: [],
    pushes: [],
    requests: [],
    getOrDelete: [],
    errors: [],
    firstT: entries[0]?.t ?? null,
    lastT: entries.at(-1)?.t ?? null,
  };
  const pending = [];
  for (const e of entries) {
    const p = e.payload;
    if (e.direction === 'spike' && p?.note === 'g4 server started') f.instances.push({ pid: e.pid, stdioModern: p.stdio_modern === true, line: e.line, t: e.t });
    else if (e.direction === 'spike' && /^listening /.test(p?.note ?? '')) f.listening.push({ pid: e.pid, url: p.note.slice('listening '.length), line: e.line });
    else if (e.direction === 'spike' && /^armed/.test(p?.note ?? '')) f.armed.push({ pid: e.pid, era: e.era, line: e.line });
    else if (e.direction === 'listen-error') f.listenErrors.push({ pid: e.pid, line: e.line, error: String(p) });
    if (e.direction === 'client->server') {
      const http = e.surface === 'http';
      const body = http ? p?.body : p;
      if (http && (p?.method === 'GET' || p?.method === 'DELETE')) {
        f.getOrDelete.push({ pid: e.pid, line: e.line, method: p.method, note: e.note ?? null, userAgent: p.headers?.['user-agent'] ?? null });
        continue;
      }
      if (http && p?.url && p.url !== '/mcp') {
        f.getOrDelete.push({ pid: e.pid, line: e.line, method: p.method, url: p.url, note: e.note ?? null });
        continue;
      }
      if (!body || typeof body !== 'object' || !body.method) continue;
      const req = {
        pid: e.pid,
        surface: e.surface,
        era: e.era,
        line: e.line,
        t: e.t,
        method: body.method,
        id: body.id,
        isRequest: body.id !== undefined,
        params: body.params ?? null,
        carriesPV: body.params?._meta?.[PV_KEY] ?? null,
        headers: http ? p.headers ?? {} : null,
        userAgent: http ? p.headers?.['user-agent'] ?? null : null,
        sessionId: http ? p.headers?.['mcp-session-id'] ?? null : null,
        response: null,
      };
      f.requests.push(req);
      if (req.isRequest) pending.push(req);
      continue;
    }
    if (e.direction === 'server->client' && p && typeof p === 'object') {
      if (p.method === 'notifications/claude/channel') {
        f.pushes.push({ pid: e.pid, era: e.era, line: e.line, t: e.t, content: p.params?.content ?? null, meta: p.params?.meta ?? {} });
        continue;
      }
      if (p.id === undefined) continue;
      const i = pending.findIndex((r) => r.pid === e.pid && r.surface === e.surface && JSON.stringify(r.id) === JSON.stringify(p.id));
      if (i === -1) continue;
      const req = pending.splice(i, 1)[0];
      req.response = { line: e.line, t: e.t, result: p.result ?? null, error: p.error ?? null, status: e.status ?? null, headers: e.headers ?? null };
      if (p.error) f.errors.push({ pid: e.pid, line: e.line, method: req.method, error: p.error });
    }
  }
  for (const r of f.requests.filter((x) => x.isRequest)) {
    const base = { pid: r.pid, surface: r.surface, era: r.era, reqLine: r.line, resLine: r.response?.line ?? null, t: r.t, error: r.response?.error ?? null };
    if (r.method === 'server/discover') (r.surface === 'http' ? f.httpDiscover : f.stdioDiscover).push({ ...base, supportedVersions: r.response?.result?.supportedVersions ?? null, channelCap: !!r.response?.result?.capabilities?.experimental?.['claude/channel'], resultType: r.response?.result?.resultType ?? null });
    else if (r.method === 'initialize' && r.surface === 'stdio') f.stdioInitialize.push({ ...base, requested: r.params?.protocolVersion ?? null, negotiated: r.response?.result?.protocolVersion ?? null, channelCap: !!r.response?.result?.capabilities?.experimental?.['claude/channel'], clientInfo: r.params?.clientInfo ?? null });
    else if (r.method === 'initialize') f.httpInitialize.push({ ...base, requested: r.params?.protocolVersion ?? null, negotiated: r.response?.result?.protocolVersion ?? null, sessionId: r.response?.headers?.['Mcp-Session-Id'] ?? null, userAgent: r.userAgent, clientInfo: r.params?.clientInfo ?? null, codexVersion: uaVersion(r.userAgent, 'codex-mcp-client') });
    else if (r.method === 'tools/list') (r.surface === 'http' ? f.httpToolsList : f.stdioToolsList).push({ ...base, tools: (r.response?.result?.tools ?? []).map((t) => t.name), cacheHints: r.response?.result?.ttlMs !== undefined && r.response?.result?.cacheScope !== undefined });
    else if (r.method === 'tools/call') {
      f.toolCalls.push({
        ...base,
        name: r.params?.name ?? null,
        text: r.params?.arguments?.text ?? null,
        userAgent: r.userAgent,
        sessionId: r.sessionId,
        carriesPV: r.carriesPV,
        protocolHeader: r.headers?.['mcp-protocol-version'] ?? null,
        resultType: r.response?.result?.resultType ?? null,
        resultText: (r.response?.result?.content ?? []).map((c) => c.text ?? '').join(''),
        provenance: provenanceOf(r.response?.result),
        claudeVersion: uaVersion(r.userAgent, 'claude-code'),
        codexVersion: uaVersion(r.userAgent, 'codex-mcp-client'),
      });
    }
  }
  return f;
}

// The modern-path requests of a transcript (criterion 3): an HTTP request carrying the
// `mcp-protocol-version: 2026-07-28` header or classified modern by the server, and a stdio
// request the server classified modern (a probe, or any request to the modern-only copy).
export function modernRequests(f) {
  return f.requests.filter((r) => r.isRequest && (r.era === 'modern' || r.headers?.['mcp-protocol-version'] === MODERN));
}

// The legacy channel copy (Claude's g4spike): the non-modern instance that owns the HTTP port.
export function roles(f) {
  const legacy = f.instances.filter((i) => !i.stdioModern && f.listening.some((l) => l.pid === i.pid));
  const modern = f.instances.filter((i) => i.stdioModern);
  return { legacyPid: legacy.length === 1 ? legacy[0].pid : null, modernPid: modern.length === 1 ? modern[0].pid : null, legacyCandidates: legacy.map((i) => i.pid), modernCandidates: modern.map((i) => i.pid) };
}

// --- fixture sanitizer ----------------------------------------------------------------------

export function sanitizeG4Transcript(text) {
  const s = String(text ?? '');
  const parts = s.split(OAC_EXT);
  return { text: parts.join(OAC_EXT_PLACEHOLDER), report: { extensionIdReplaced: parts.length - 1 } };
}
