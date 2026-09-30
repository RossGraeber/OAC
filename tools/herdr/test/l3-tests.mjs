// L3a (#189) unit checks for lib/l3.mjs, run by the driver self-test
// (`node tools/herdr/run.mjs --self-test`). Unit only, in-process, on synthetic input: no
// herdr, no harness, no Beacon. Runs on Windows too.
//
// Probe values are generated in-process by makeProbeMarkers(). No check name and no failure
// detail ever carries a marker or fake-token value (details give ids and counts only), and no
// probe value appears in this file.

import { spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { inspect } from 'node:util';

import {
  L3_RECORD_VERSION, DELIVERY_PATHS, MARKER_SHAPE, TOKEN_SHAPE, makeProbeMarkers, markerRecords, augmentCaseTable, scanRuntimeLog, redactedExcerpt,
  harnessConfigTargets, hashConfig, sectionHashes, trailingCommentHeaders, compareSections, assertNoMarkerLeak, findMarkerLeaks, MarkerLeakError, sha256,
  L3_CLAUDE_CASE, L3_CODEX_CASE, EXCERPT_CHARS, neutralizePlaceholders, PLACEHOLDER_RE, sectionKey,
} from '../lib/l3.mjs';
import { createRedactor, reportIsClean } from '../lib/redact.mjs';
import { draftL3, leakGuard, parseNote, L3LeakAbort, L3ReportError, STEPS, RESULTS } from '../lib/l3-report.mjs';
import { presend } from '../gate-servers/g5-channel.mjs';
import { frameCase, HEADER_FIELDS } from '../gate-servers/g5-codex.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, '..', '..', '..');
const CASES = join(REPO, 'tools', 'herdr', 'gate-servers', 'g5-cases.json');
const LIB = join(REPO, 'tools', 'herdr', 'lib', 'l3.mjs');

// Excerpt placeholders read as a secret assignment to the scan; neutralize them first.
const clean = (redactor, text) => reportIsClean({ ...redactor.scan(neutralizePlaceholders(text)), hazardProtocolFrames: [] });

export function l3Unit(check) {
  // --- markers ---
  const markers = makeProbeMarkers();
  const values = markers.flatMap((m) => [m.marker, m.token]);
  check('l3: one marker and one fake token per delivery path', markers.length === 3 && markers.map((m) => m.id).join() === DELIVERY_PATHS.join());
  check('l3: markers and tokens have the right shape', markers.every((m) => MARKER_SHAPE.test(m.marker) && TOKEN_SHAPE.test(m.token)));
  check('l3: all six probe values are unique', new Set(values).size === 6);
  check('l3: a second draw shares no value with the first', !makeProbeMarkers().some((m) => values.includes(m.marker) || values.includes(m.token)));
  check('l3: recorded hashes are sha256 of the values', markers.every((m) => m.markerSha256 === sha256(m.marker) && m.tokenSha256 === sha256(m.token)));
  const serialized = [JSON.stringify(markers), inspect(markers), JSON.stringify(markerRecords(markers)), String(markers[0])].join('\n');
  check('l3: JSON and inspect of the marker objects carry ids and hashes only', findMarkerLeaks(serialized, markers).length === 0 && serialized.includes(markers[0].markerSha256), `leaks=${findMarkerLeaks(serialized, markers).length}`);
  // Review #193 finding 1: the run-manifest writer (run.mjs) walks values with
  // redactValue() (Object.entries, no toJSON); spread and Object.assign bypass toJSON too.
  const viaManifest = JSON.stringify(createRedactor().redactValue({ l3: { markers } }).value);
  check('l3: redactValue({ l3: { markers } }) (the manifest write path) carries no probe value', findMarkerLeaks(viaManifest, markers).length === 0 && viaManifest.includes(markers[0].markerSha256), `leaks=${findMarkerLeaks(viaManifest, markers).length}`);
  const bypasses = [JSON.stringify({ ...markers[0] }), JSON.stringify(Object.assign({}, markers[1])), JSON.stringify(Object.entries(markers[2])), JSON.stringify(Object.keys(markers[0]))].join('\n');
  check('l3: spread, Object.assign, entries and keys reach no probe value', findMarkerLeaks(bypasses, markers).length === 0 && !Object.keys(markers[0]).includes('marker') && !Object.keys(markers[0]).includes('token'), `leaks=${findMarkerLeaks(bypasses, markers).length}`);
  const repeating = () => Buffer.alloc(16, 7);
  let dupThrows = false;
  try {
    makeProbeMarkers({ rand: repeating });
  } catch {
    dupThrows = true;
  }
  check('l3: a repeating random source is refused', dupThrows);

  // --- case table ---
  const raw = readFileSync(CASES, 'utf8');
  const table = JSON.parse(raw);
  const before = JSON.stringify(table);
  const aug = augmentCaseTable(table, markers);
  check('l3: input case table is not mutated', JSON.stringify(table) === before);
  check('l3: committed g5-cases.json untouched and carries no probe value', readFileSync(CASES, 'utf8') === raw && findMarkerLeaks(raw, markers).length === 0);
  const byId = (list, id) => list.find((c) => c.id === id);
  const l3c = byId(aug.claude, L3_CLAUDE_CASE);
  const p = l3c ? presend(l3c) : null;
  check('l3: L3C passes g5-channel presend() (all five security keys, identifier-safe)', !!p && p.refuse === false && p.missingSecurityKeys.length === 0 && p.unsafeKeys.length === 0, JSON.stringify(p));
  check('l3: L3C carries the claude-channel probe', !!l3c && findMarkerLeaks(l3c.content, markers).map((l) => `${l.id}:${l.what}`).join() === 'claude-channel:marker,claude-channel:token');
  const l3x = byId(aug.codex, L3_CODEX_CASE);
  let framed = null;
  try {
    framed = frameCase(l3x, {});
  } catch {
    framed = null;
  }
  const x1 = byId(table.codex, 'X1');
  check('l3: L3X has X1\'s call/header shape', !!l3x && l3x.call === x1.call && HEADER_FIELDS.every((k) => Object.hasOwn(l3x.header, k)));
  check('l3: g5-codex frameCase accepts L3X and frames the codex-turn-start probe', !!framed && /^[0-9a-z]{26}$/.test(framed.D) && framed.text.startsWith(`--- oac-envelope ${framed.D} ---`) && findMarkerLeaks(framed.text, markers).every((l) => l.id === 'codex-turn-start') && findMarkerLeaks(framed.text, markers).length === 2);
  const x4 = byId(aug.codex, 'X4');
  const x4orig = byId(table.codex, 'X4');
  check('l3: X4 body replaced by the codex-queue-add probe', findMarkerLeaks(x4.body, markers).map((l) => `${l.id}:${l.what}`).join() === 'codex-queue-add:marker,codex-queue-add:token' && x4.body !== x4orig.body);
  check('l3: X4 setup text replaced, carries no probe value, still a long setup turn', x4.setupText !== x4orig.setupText && findMarkerLeaks(x4.setupText, markers).length === 0 && /30 distinct/.test(x4.setupText));
  let x4framed = null;
  try {
    x4framed = frameCase(x4, {});
  } catch {
    x4framed = null;
  }
  check('l3: X4 still frames', !!x4framed);
  check('l3: other cases unchanged', ['C1', 'C2', 'C3', 'C4', 'C4b', 'C5', 'C6'].every((id) => JSON.stringify(byId(aug.claude, id)) === JSON.stringify(byId(table.claude, id))) && ['X1', 'X2', 'X3', 'X5', 'X6'].every((id) => JSON.stringify(byId(aug.codex, id)) === JSON.stringify(byId(table.codex, id))));
  let twice = false;
  try {
    augmentCaseTable(aug, markers);
  } catch {
    twice = true;
  }
  check('l3: augmenting an already-augmented table is refused', twice);

  // --- scanner ---
  const [mc, mx, mq] = markers;
  // A prefix-only line: both prefixes followed by values that are NOT any generated marker.
  const other = makeProbeMarkers();
  const lines = [
    { event: { action: 'prompt.submitted' }, harness: { name: 'claude-code', collection_method: 'hook', executable_path: '/home/alice/bin/claude' }, session: { id: 's-claude-1', working_directory: '/home/alice/proj' }, prompt: { text: `before ${mc.marker} and ${mc.token}` } },
    { event: { action: 'mcp.tool_invoked' }, harness: { name: 'claude-code', collection_method: 'poll' }, session: { id: 's-claude-1' }, raw: { args: [JSON.stringify({ message: `reply ${mc.marker}` })] } },
    { event: { action: 'prompt.submitted' }, harness: { name: 'codex', collection_method: 'otlp' }, session: { id: 's-codex-1' }, prompt: { text: `L3 probe ${mx.marker}.` } },
    { event: { action: 'prompt.submitted' }, harness: { name: 'codex', collection_method: 'otlp' }, session: { id: 's-codex-1' }, prompt: { text: `prefix only ${other[0].marker} ${other[0].token}` } },
    { event: { action: 'agent.message' }, harness: { name: 'codex' }, session: { id: 's-codex-1' }, message: 'nothing here' },
  ].map((o) => JSON.stringify(o));
  const early = JSON.stringify({ event: { action: 'prompt.submitted' }, harness: { name: 'claude-code' }, session: { id: 's-old' }, prompt: { text: mq.marker } });
  const text = `${early}\n${lines.join('\n')}\nnot json but ${mq.token}\n`;
  const fromByte = Buffer.byteLength(`${early}\n`);
  const rotated = `${JSON.stringify({ event: { action: 'prompt.submitted' }, harness: { name: 'codex', collection_method: 'poll' }, session: { id: 's-codex-2' }, prompt: { text: `${mq.marker} ${mq.token}` } })}\n`;
  const scan = scanRuntimeLog([{ label: 'runtime.jsonl', text, fromByte }, { label: 'runtime-1.jsonl', text: rotated }], markers);
  const hs = scan.hits;
  check('l3 scan: a prefix-only line is not a hit', !hs.some((h) => h.file === 'runtime.jsonl' && h.line === 5));
  check('l3 scan: fromByte skips the line before it', !hs.some((h) => h.file === 'runtime.jsonl' && h.line === 1) && scan.files[0].linesSkipped === 1);
  check('l3 scan: lines with a hit counted (4 in runtime.jsonl, 1 in the rotated file)', scan.files[0].hitLines === 4 && scan.files[1].hitLines === 1, JSON.stringify(scan.files));
  const h1 = hs.find((h) => h.line === 2 && h.file === 'runtime.jsonl');
  check('l3 scan: marker and verbatim token attributed with their paths and event.action', !!h1 && h1.markerId === 'claude-channel' && h1.markerPresent && h1.tokenVerbatim && h1.markerPaths.join() === 'prompt.text' && h1.tokenPaths.join() === 'prompt.text' && h1.eventAction === 'prompt.submitted', JSON.stringify(h1 && { ...h1 }));
  check('l3 scan: harness and session identifiers recorded; path fields named, not valued', !!h1 && h1.harness.name === 'claude-code' && h1.harness.collection_method === 'hook' && h1.session.id === 's-claude-1' && h1.harnessOtherFields.includes('executable_path') && h1.sessionOtherFields.includes('working_directory') && !JSON.stringify(scan).includes('/home/alice'));
  const h2 = hs.find((h) => h.line === 3 && h.file === 'runtime.jsonl');
  check('l3 scan: a marker inside a JSON-string argument is found at its path, no token', !!h2 && h2.markerPaths.join() === 'raw.args[0]' && !h2.tokenVerbatim && h2.eventAction === 'mcp.tool_invoked');
  const hx = hs.find((h) => h.line === 4 && h.file === 'runtime.jsonl');
  check('l3 scan: codex-turn-start hit attributed per marker', !!hx && hx.markerId === 'codex-turn-start' && !hx.tokenVerbatim);
  const hn = hs.find((h) => h.line === 7 && h.file === 'runtime.jsonl');
  check('l3 scan: a non-JSON line with a token is a hit, unparsed, token only', !!hn && hn.markerId === 'codex-queue-add' && !hn.parsed && !hn.markerPresent && hn.tokenVerbatim && scan.files[0].unparsedLines === 1);
  const hr = hs.find((h) => h.file === 'runtime-1.jsonl');
  check('l3 scan: several files scanned, byte offsets per file', !!hr && hr.byteOffset === 0 && hr.line === 1 && hn.byteOffset === Buffer.byteLength(text.split('\n').slice(0, 6).join('\n')) + 1);
  const bm = scan.byMarker;
  check('l3 scan: per-marker summary', bm['claude-channel'].lines === 2 && bm['claude-channel'].tokenVerbatimLines === 1 && bm['claude-channel'].actions.join() === 'mcp.tool_invoked,prompt.submitted' && bm['codex-turn-start'].lines === 1 && bm['codex-queue-add'].lines === 2 && bm['codex-queue-add'].tokenVerbatimLines === 2, JSON.stringify(bm));
  const bs = scan.bySession;
  const g = (h, s) => bs.find((x) => x.harness === h && x.sessionId === s);
  check('l3 scan: hits grouped by (harness, session id)', bs.length === 4 && g('claude-code', 's-claude-1')?.lines === 2 && g('codex', 's-codex-1')?.lines === 1 && g('codex', 's-codex-2')?.tokenVerbatim === true && g(null, null)?.lines === 1, JSON.stringify(bs));
  check('l3 scan: the scan result carries no probe value', findMarkerLeaks(JSON.stringify(scan), markers).length === 0);
  const single = scanRuntimeLog(text, markers, { fromByte });
  check('l3 scan: a single string input with opts.fromByte', single.hits.length === 4 && single.files[0].label === 'runtime.jsonl');
  check('l3 scan: fromByte within the file raises no warning', scan.warnings.length === 0 && scan.files.every((x) => x.startBeyondEnd === false));
  const past = scanRuntimeLog([{ label: 'runtime.jsonl', text: rotated, fromByte: Buffer.byteLength(rotated) + 10000 }, { label: 'runtime-1.jsonl', text: rotated }], markers);
  check('l3 scan: fromByte past the end (rotated or truncated log) is flagged, not a silent clean negative', past.files[0].startBeyondEnd === true && past.files[0].linesScanned === 0 && past.warnings.length === 1 && past.warnings[0].startsWith('runtime.jsonl:') && past.files[1].startBeyondEnd === false && past.hits.length === 1);
  check('l3 scan: fromByte exactly at the end is not flagged', scanRuntimeLog(rotated, markers, { fromByte: Buffer.byteLength(rotated) }).warnings.length === 0);
  const keyed = scanRuntimeLog(JSON.stringify({ event: { action: 'x' }, raw: { [mc.marker]: 1 } }), markers);
  check('l3 scan: a probe value used as a key is reported with the value substituted', keyed.hits[0]?.markerPaths[0] === 'raw.<L3-MARKER:claude-channel><key>');

  // --- excerpts ---
  const synthetic = createRedactor({ home: '/home/alice', username: 'alice', hostname: 'buildbox-7' });
  const exLine = JSON.stringify({ endpoint: { hostname: 'buildbox-7' }, user: { name: 'alice' }, session: { working_directory: '/home/alice/proj' }, prompt: { text: `other sk-${'Z'.repeat(24)} from alice ${mc.marker} key ${mc.token}` } });
  const ex = redactedExcerpt(exLine, h1, markers, synthetic);
  check('l3 excerpt: at most ~160 characters', ex.length <= EXCERPT_CHARS + 40, String(ex.length));
  check('l3 excerpt: exported PLACEHOLDER_RE is not global (stateless .test)', !PLACEHOLDER_RE.global && PLACEHOLDER_RE.test(ex) && PLACEHOLDER_RE.test(ex) && neutralizePlaceholders(`${ex} ${ex}`).match(/<L3-/g) === null);
  check('l3 excerpt: marker and token become id placeholders', ex.includes('<L3-MARKER:claude-channel>') && ex.includes('<L3-FAKE-TOKEN:claude-channel>'));
  check('l3 excerpt: no probe value, home path, username or host; scans clean (synthetic identity)', findMarkerLeaks(ex, markers).length === 0 && !/alice|buildbox/.test(ex) && clean(synthetic, ex) && ex.includes('<SECRET>'));
  const here = createRedactor();
  const hereLine = JSON.stringify({ session: { working_directory: join(homedir(), 'src', 'proj') }, prompt: { text: `x ${mx.marker} ${mx.token}` } });
  const exHere = redactedExcerpt(hereLine, { markerId: 'codex-turn-start' }, markers, here);
  check('l3 excerpt: this machine\'s home path and username redacted; scans clean', findMarkerLeaks(exHere, markers).length === 0 && !exHere.includes(homedir()) && clean(here, exHere));
  const longLine = `${'a'.repeat(400)} ${mq.marker} ${'b'.repeat(400)}`;
  const exLong = redactedExcerpt(longLine, { markerId: 'codex-queue-add' }, markers, synthetic);
  check('l3 excerpt: long line cut around the hit', exLong.length <= EXCERPT_CHARS && exLong.includes('<L3-MARKER:codex-queue-add>'));
  const edge = `${mq.marker}${'c'.repeat(300)}`;
  const exEdge = redactedExcerpt(edge, { markerId: 'codex-turn-start' }, markers, synthetic);
  check('l3 excerpt: a value at a window edge is substituted before cutting (no partial value)', !exEdge.includes(mq.marker.slice(0, 12)) && clean(synthetic, exEdge));

  // --- leak assertion ---
  const child = spawnSync(process.execPath, ['-e', 'console.log("step ok " + process.env.L3_PLANTED)'], { env: { ...process.env, L3_PLANTED: mc.marker }, encoding: 'utf8', timeout: 20000 });
  let caught = null;
  try {
    assertNoMarkerLeak(child.stdout, markers, 'planted stdout');
  } catch (e) {
    caught = e;
  }
  check('l3 leak: a planted console.log of a marker is caught', caught instanceof MarkerLeakError && /claude-channel marker/.test(caught.message) && findMarkerLeaks(caught.message, markers).length === 0);
  check('l3 leak: clean output passes, prefix-only output passes', assertNoMarkerLeak('step ok', markers) === true && assertNoMarkerLeak(`${other[1].marker} ${other[1].token}`, markers) === true);

  // --- config targets, hashing, sections ---
  const fakeHome = join(tmpdir(), 'oac-l3-no-such-home');
  const t0 = harnessConfigTargets({}, { home: fakeHome });
  check('l3 config: the four fixed $HOME files, labelled ~/...', t0.targets.map((t) => t.label).join() === '~/.claude/settings.json,~/.claude.json,~/.codex/config.toml,~/.codex/hooks.json' && t0.targets.every((t) => t.path.startsWith(fakeHome)) && t0.findings.length === 0);
  const envDir = join(tmpdir(), 'oac-l3-env-dir');
  const t1 = harnessConfigTargets({ CLAUDE_CONFIG_DIR: envDir, CODEX_HOME: envDir }, { home: fakeHome });
  check('l3 config: CLAUDE_CONFIG_DIR / CODEX_HOME add env-dir targets and findings (names, not values)', t1.targets.length === 8 && t1.targets.slice(0, 4).every((t) => t.path.startsWith(fakeHome)) && t1.findings.length === 2 && !t1.findings.join().includes(envDir));

  const toml = ['model = "x"', '', '[mcp_servers.oac]', 'command = "oac"', '', '[otel]', 'log_user_prompt = false', '', '[profiles.a]', 'k = 1', ''].join('\n');
  const tomlOtelOnly = toml.replace('log_user_prompt = false', 'log_user_prompt = true\nexporter = "otlp"');
  const tomlOutside = toml.replace('command = "oac"', 'command = "other"');
  const ts = sectionHashes('codex-config', toml).sections;
  check('l3 sections: config.toml one hash per header, header text kept', Object.keys(ts).join() === '<root>,[mcp_servers.oac],[otel],[profiles.a]' && !JSON.stringify(ts).includes('oac"'));
  const cmp = (kind, a, b) => compareSections([{ label: 'f', kind, present: true, sha256: sha256(a), ...sectionHashes(kind, a) }], [{ label: 'f', kind, present: true, sha256: sha256(b), ...sectionHashes(kind, b) }]).files[0];
  const otelOnly = cmp('codex-config', toml, tomlOtelOnly);
  check('l3 sections: an [otel]-only change is reported as [otel] only', otelOnly.status === 'changed' && otelOnly.sections.changed.join() === '[otel]' && !otelOnly.sections.added.length);
  const outside = cmp('codex-config', toml, tomlOutside);
  check('l3 sections: a change outside [otel] is detected and named', outside.status === 'changed' && outside.sections.changed.join() === '[mcp_servers.oac]');
  const crlf = cmp('codex-config', toml, toml.replace(/\n/g, '\r\n'));
  check('l3 sections: CRLF re-encoding of config.toml leaves every section equal', crlf.sections.changed.length === 0 && crlf.formattingOnly === true);

  const settings = { env: { A: '1', B: '2' }, hooks: { PreToolUse: [{ matcher: '*' }] }, model: 'm' };
  const s1 = JSON.stringify(settings);
  const s2 = JSON.stringify({ model: 'm', hooks: settings.hooks, env: { B: '2', A: '1' } }, null, 4);
  const fmt = cmp('claude-settings', s1, s2);
  check('l3 sections: formatting-only JSON re-encoding ignored (sections equal, whole file differs)', fmt.status === 'changed' && fmt.formattingOnly && !fmt.sections.changed.length);
  const ss = sectionHashes('claude-settings', s1).sections;
  check('l3 sections: settings.json per top-level key plus per env and hooks key', Object.keys(ss).sort().join() === 'env.A,env.B,hooks.PreToolUse,key:env,key:hooks,key:model');
  const envChange = cmp('claude-settings', s1, JSON.stringify({ ...settings, env: { ...settings.env, OTEL_LOG_USER_PROMPTS: '1' } }));
  check('l3 sections: an added env key is reported by name', envChange.sections.added.join() === 'env.OTEL_LOG_USER_PROMPTS' && envChange.sections.changed.join() === 'key:env');
  const st = sectionHashes('claude-state', JSON.stringify({ oauthAccount: { emailAddress: 'a@example.com' }, numStartups: 3 })).sections;
  check('l3 sections: ~/.claude.json key names only, no value hash', Object.keys(st).join() === 'key:numStartups,key:oauthAccount' && Object.values(st).every((v) => v === null));
  check('l3 sections: hooks.json per top-level key', Object.keys(sectionHashes('codex-hooks', '{"hooks":{},"version":1}').sections).join() === 'key:hooks,key:version');
  const bad = sectionHashes('claude-settings', '{not json');
  check('l3 sections: unparseable JSON hashed whole and flagged', bad.parseError === true && Object.keys(bad.sections).join() === '<unparseable>');

  const tc = trailingCommentHeaders(['[otel]', 'x = 1', '[mcp_servers.oac] # oac', 'command = "oac"', '[a]', '[b] # note'].join('\n'));
  check('l3 trailing comment: headers found with line numbers; the one after [otel] flagged', tc.headers.length === 2 && tc.headers[0].line === 3 && tc.headers[0].afterOtel === true && tc.headers[1].line === 6 && tc.headers[1].afterOtel === false && tc.anyAfterOtel);
  const tc2 = trailingCommentHeaders(['[otel.exporter]', 'x = 1', '[m] #c'].join('\n'));
  const tc3 = trailingCommentHeaders(['[a] # c', '[otel]', '[b]', '[c] # c', 'arr = [1] # not a header'].join('\n'));
  check('l3 trailing comment: [otel.*] counts; a recognized header in between clears it; values are not headers', tc2.anyAfterOtel && !tc3.anyAfterOtel && tc3.headers.length === 2);
  const tc4 = trailingCommentHeaders(['[[otel]]', '[a] # c', '[otelx]', '[b] # c'].join('\n'));
  check('l3 trailing comment: only Beacon\'s own otel test counts ([otel] or [otel.*]; not [[otel]] or [otelx])', tc4.headers.length === 2 && !tc4.anyAfterOtel);

  // Review #193 finding 2: a non-bare header (Codex's [projects.'<abs path>']) must not put a
  // home path into a section key, and a baseline read back through manifest redaction must
  // compare unchanged against a fresh hash.
  const proj = join(homedir(), 'src', 'x');
  const projToml = ['[otel]', 'a = 1', `[projects.'${proj}']`, 'trust_level = "trusted"', `[projects."${join(homedir(), 'src', 'y')}"]`, 'trust_level = "trusted"', '[ mcp_servers . oac ]', 'command = "oac"', ''].join('\n');
  const psec = sectionHashes('codex-config', projToml).sections;
  const pkeys = Object.keys(psec);
  check('l3 sections: bare dotted headers kept verbatim, path headers keyed by leading segments plus a hash', pkeys.includes('[otel]') && pkeys.includes('[mcp_servers.oac]') && pkeys.filter((k) => /^\[projects\.<sha256:[0-9a-f]{16}>\]$/.test(k)).length === 2, pkeys.filter((k) => !k.includes(homedir())).join(' '));
  check('l3 sections: no section key carries the home path or username', !pkeys.some((k) => k.includes(homedir()) || k.toLowerCase().includes(homedir().split(/[\\/]/).pop().toLowerCase())));
  check('l3 sections: sectionKey is stable and distinguishes paths', sectionKey(`[projects.'${proj}']`) === sectionKey(`[ projects . '${proj}' ]`) && sectionKey(`[projects.'${proj}']`) !== sectionKey(`[projects.'${proj}2']`) && sectionKey('[[a.b]]') === '[[a.b]]');
  const redSections = createRedactor().redactValue({ sections: psec }).value.sections;
  const roundTrip = compareSections([{ label: 'c', kind: 'codex-config', present: true, sha256: 'x', sections: redSections }], [{ label: 'c', kind: 'codex-config', present: true, sha256: 'x', sections: psec }]).files[0];
  check('l3 sections: baseline through manifest redaction round-trips with no spurious section change', roundTrip.status === 'unchanged' && !roundTrip.sections.changed.length && !roundTrip.sections.added.length && !roundTrip.sections.removed.length, JSON.stringify(roundTrip.sections).replaceAll(homedir(), '~'));

  const dir = mkdtempSync(join(tmpdir(), 'oac-l3-unit-'));
  try {
    // Synthetic files with neutral names in a fresh temp dir; nothing here is a harness path.
    const a = join(dir, 'a.toml');
    const b = join(dir, 'b.json');
    writeFileSync(a, toml);
    writeFileSync(b, s1);
    const targets = [
      { label: 'A', kind: 'codex-config', path: a },
      { label: 'B', kind: 'claude-settings', path: b },
      { label: 'C', kind: 'codex-hooks', path: join(dir, 'missing.json') },
    ];
    const h0 = hashConfig(targets);
    check('l3 hash: whole-file sha256, sections, trailing-comment report; missing file recorded absent', h0[0].sha256 === sha256(toml) && !!h0[0].sections['[otel]'] && Array.isArray(h0[0].trailingComment.headers) && h0[1].sections['env.A'] && h0[2].present === false);
    check('l3 hash: result holds no config content', !JSON.stringify(h0).includes('log_user_prompt') && !JSON.stringify(h0).includes('"m"'));
    writeFileSync(a, tomlOutside);
    writeFileSync(b, s2);
    const c = compareSections(h0, hashConfig(targets));
    check('l3 hash: compare names the changed file and section; formatting-only JSON flagged', !c.unchanged && c.files[0].sections.changed.join() === '[mcp_servers.oac]' && c.files[1].formattingOnly && c.files[2].status === 'absent');
    check('l3 hash: an unchanged pair compares unchanged', compareSections(h0, h0).unchanged);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }

  // --- module hygiene ---
  const src = readFileSync(LIB, 'utf8');
  check('l3: lib/l3.mjs has no write, spawn or network API', !/\b(?:writeFile|appendFile|createWriteStream|copyFile|rename|unlink|rmSync|truncate|mkdir)\w*\s*\(|child_process|node:net|node:http|fetch\s*\(/.test(src));
  check('l3: record version is 1', L3_RECORD_VERSION === 1);

  l3ReportUnit(check);
}

// --- L3c (#191): lib/l3-report.mjs, on synthetic phase run manifests (L3a record schema) ------

const REPORT = join(REPO, 'tools', 'herdr', 'lib', 'l3-report.mjs');
const T0 = Date.parse('2026-09-30T10:00:00.000Z');
const at = (min) => new Date(T0 + min * 60000).toISOString();
// Canaries: raw log text and config content that must never reach the draft.
const LOG_CANARY = 'lighthouse-canary-text';
const CONFIG_CANARY = 'config-canary-value';

function l3Fixtures(markers) {
  const [mc, mx, mq] = markers;
  const settings0 = JSON.stringify({ env: { A: CONFIG_CANARY }, model: 'm' });
  const settings1 = JSON.stringify({ env: { A: CONFIG_CANARY, OTEL_LOG_USER_PROMPTS: '1' }, model: 'm' }, null, 2);
  const toml0 = ['[otel]', 'log_user_prompt = false', '', '[mcp_servers.oac]', `command = "${CONFIG_CANARY}"`, ''].join('\n');
  const toml1 = toml0.replace('log_user_prompt = false', 'log_user_prompt = true');
  const hashesOf = (settings, toml) => [
    { label: '~/.claude/settings.json', kind: 'claude-settings', present: true, sha256: sha256(settings), ...sectionHashes('claude-settings', settings) },
    { label: '~/.claude.json', kind: 'claude-state', present: false, sha256: null, sections: null },
    { label: '~/.codex/config.toml', kind: 'codex-config', present: true, sha256: sha256(toml), ...sectionHashes('codex-config', toml), trailingComment: trailingCommentHeaders(toml) },
    { label: '~/.codex/hooks.json', kind: 'codex-hooks', present: false, sha256: null, sections: null },
  ];
  const line = (action, harness, session, text) => JSON.stringify({ event: { action }, harness: { name: harness, collection_method: 'otlp' }, session: { id: session }, prompt: { text } });
  const summary = (lines) => {
    const s = scanRuntimeLog([{ label: 'runtime.jsonl', text: `${lines.join('\n')}\n` }], markers);
    return { files: s.files, byMarker: s.byMarker, bySession: s.bySession };
  };
  const hitLines = [
    line('prompt.submitted', 'claude-code', 's-claude-1', `${LOG_CANARY} ${mc.marker}`),
    line('mcp.tool_invoked', 'claude-code', 's-claude-1', `reply ${mc.marker}`),
    line('prompt.submitted', 'codex', 's-codex-1', `${LOG_CANARY} ${mx.marker}`),
    line('prompt.submitted', 'codex', 's-codex-1', `${LOG_CANARY} ${mq.marker}`),
  ];
  return {
    base: hashesOf(settings0, toml0),
    installed: hashesOf(settings1, toml1),
    scans: {
      hits: summary(hitLines),
      zero: summary([line('prompt.submitted', 'claude-code', 's-claude-1', `${LOG_CANARY} nothing`)]),
      token: summary([line('prompt.submitted', 'claude-code', 's-claude-1', `${mc.marker} ${mc.token}`), ...hitLines.slice(2)]),
      outside: summary([...hitLines, line('prompt.submitted', 'claude-code', 's-claude-9', `${mx.marker}`)]),
    },
  };
}

function l3Runs(markers, fx, o = {}) {
  const pins = { claude: { lastObserved: '2.1.284', differs: !!o.drift }, codex: { lastObserved: '0.158.0', differs: false } };
  const versions = { beacon: 'beacon version 1.3.29', claudeCli: '2.1.284 (Claude Code)', codex: { cli: 'codex-cli 0.158.0', daemon: { cliVersion: '0.158.0' }, wire: '0.158.0' }, pins };
  const record = (phase, hashes, extra = {}) => ({
    version: 1,
    phase,
    box: { start: at(0), budgetMs: 3600000, sourceRunId: phase === 'baseline' ? null : 'run-base' },
    versions,
    config: { hashes, compare: null, findings: [] },
    markers: phase === 'probe' ? markerRecords(markers) : [],
    scan: null,
    findings: [],
    ...extra,
  });
  const manifest = (runId, phase, start, end, rec, extra = {}) => ({
    schemaVersion: 1,
    runId,
    outcome: 'PASS',
    outcomeReason: null,
    scenario: { name: 'l3-beacon', file: 'tools/herdr/scenarios/l3-beacon.mjs', params: { phase, accept: 'human' } },
    driver: { entry: 'tools/herdr/run.mjs', commit: 'a'.repeat(40), toolsHerdrDirty: !!o.dirty, node: process.version },
    // Fields the report never prints, carrying this machine's home path.
    herdr: { pinnedTag: 'v0.9.1', observedVersionOutput: 'herdr 0.9.1', config: { path: join(homedir(), 'herdr-run', 'config.toml') } },
    scratch: { path: join(homedir(), 'scratch') },
    harnessVersions: { claude: '2.1.284 (Claude Code)', codex: 'codex-cli 0.158.0' },
    timebox: { budgetMs: 3600000, start, end, expired: false },
    commands: [],
    scenarioData: rec ? { l3: rec } : {},
    findings: [],
    ...extra,
  });
  const probeEnd = o.probeEnd ?? 30;
  return {
    baseline: { manifest: manifest('run-base', 'baseline', at(0), at(5), record('baseline', fx.base)) },
    probe: { manifest: manifest('run-probe', 'probe', at(10), at(probeEnd), record('probe', fx.installed, { scan: o.scan ?? fx.scans.hits })) },
    verify: { manifest: manifest('run-verify', 'verify', at(probeEnd + 5), o.verifyEnd ?? at(probeEnd + 10), record('verify', o.verifyHashes ?? fx.base)) },
    priors: [],
    notes: {},
  };
}

const lineFor = (draft, id) => draft.split('\n').find((l) => l.startsWith(`- **${id}:**`)) ?? '';
const tryDraft = (runs, opts) => {
  try {
    return { text: draftL3(runs, opts), err: null };
  } catch (err) {
    return { text: null, err };
  }
};

function l3ReportUnit(check) {
  const markers = makeProbeMarkers();
  const fx = l3Fixtures(markers);
  const values = markers.flatMap((m) => [m.marker, m.token]);
  const noValue = (s) => !values.some((v) => String(s ?? '').includes(v));
  const userName = homedir().split(/[\\/]/).pop();

  // All steps that can pass do; B1, B5, B6 are NOT RUN by the operator decisions.
  const all = tryDraft(l3Runs(markers, fx), { markers });
  const d = all.text ?? '';
  check('l3 report: all-PASS runs draft without error', !all.err, all.err?.message);
  check('l3 report: one line per step B0-B7, in order', STEPS.every((id) => lineFor(d, id)) && d.indexOf('- **B0:**') < d.indexOf('- **B7:**'));
  check('l3 report: B0, B2, B3, B4, B7 PASS', ['B0', 'B2', 'B3', 'B4', 'B7'].every((id) => lineFor(d, id).includes(':** PASS —')), ['B0', 'B2', 'B3', 'B4', 'B7'].map((id) => lineFor(d, id).slice(0, 40)).join(' | '));
  check('l3 report: B1 NOT RUN by the operator decision; B5 and B6 NOT RUN by default', lineFor(d, 'B1').includes('NOT RUN') && /operator decisions of 2026-09-30/.test(lineFor(d, 'B1')) && lineFor(d, 'B5').includes('NOT RUN') && lineFor(d, 'B6').includes('NOT RUN'));
  check('l3 report: B1 evidence names changed sections, attributed to the operator Beacon step', d.includes('`env.OTEL_LOG_USER_PROMPTS`') && d.includes('`[otel]`') && d.includes('attributed to: operator Beacon step'));
  check('l3 report: B3 names the tool-invocation capture', lineFor(d, 'B3').includes('mcp.tool_invoked'));
  check('l3 report: poll path NOT RUN (no sync --print field in record v1)', d.includes('`beacon endpoint claude sync --print`): NOT RUN') && d.includes('`beacon endpoint codex sync --print`): NOT RUN'));
  check('l3 report: header carries versions, pins, date, box and each phase\'s start and end', d.includes('- **Date:** 2026-09-30') && d.includes('`beacon version 1.3.29`') && d.includes('PINS.md last observed `2.1.284`') && d.includes(`ends ${at(60)}`) && d.includes(`\`${at(10)}\` to \`${at(30)}\``));
  check('l3 report: Driver lines as g1-report renders them, plus toolsHerdrDirty', d.includes('- **Driver (probe):** herdr (`herdr 0.9.1`, PINS.md `herdr (test tooling)` v0.9.1) via `tools/herdr/run.mjs`, scenario `tools/herdr/scenarios/l3-beacon.mjs`, driver commit') && d.includes('`driver.toolsHerdrDirty`: false'));
  check('l3 report: states herdr-driven, accept=human, not a gate result', /herdr-driven/.test(d) && d.includes('`accept=human`') && /not a gate result/.test(d) && /changes no verdict/.test(d));
  check('l3 report: operator attestation present and unticked, naming Beacon', d.includes('### Operator attestation') && (d.match(/^- \[ \] /gm) ?? []).length === 4 && !/^- \[x\]/im.test(d) && d.includes('**Beacon:**') && d.includes('**Attested by:** <operator>'));
  check('l3 report: no probe value, raw log text, config content, home path or username in the draft', noValue(d) && !d.includes(LOG_CANARY) && !d.includes(CONFIG_CANARY) && !d.includes(homedir()) && !(userName.length >= 4 && d.toLowerCase().includes(userName.toLowerCase())));
  check('l3 report: the draft scans clean (redactor, placeholders neutralized)', clean(createRedactor(), d));

  // Pin drift is a finding, not a stop.
  const drift = tryDraft(l3Runs(markers, fx, { drift: true }), { markers });
  check('l3 report: pin drift makes B0 a FINDING and is listed under Findings; the leg continues', !drift.err && lineFor(drift.text, 'B0').includes('FINDING') && /pin drift: Claude Code/.test(drift.text) && lineFor(drift.text, 'B2').includes('PASS'));

  // Probe NOT RUN: its steps are NOT RUN with the manifest's reason.
  const nr = l3Runs(markers, fx);
  nr.probe.manifest.outcome = 'NOT RUN';
  nr.probe.manifest.outcomeReason = 'command #7 (agent-wait) timed out';
  const nrd = tryDraft(nr, { markers });
  check('l3 report: probe NOT RUN makes B2-B4 NOT RUN with the outcome reason', !nrd.err && ['B2', 'B3', 'B4'].every((id) => lineFor(nrd.text, id).includes('NOT RUN') && lineFor(nrd.text, id).includes('command #7 (agent-wait) timed out')), nrd.err?.message);
  check('l3 report: B0 and B7 still evaluated when only the probe is NOT RUN', !nrd.err && lineFor(nrd.text, 'B0').includes('PASS') && lineFor(nrd.text, 'B7').includes('PASS'));

  // L3 box across phases: the probe ends 70 minutes after the box started.
  const late = tryDraft(l3Runs(markers, fx, { probeEnd: 70 }), { markers });
  check('l3 report: box expired between phases -> later steps NOT RUN (L3 box expired)', !late.err && ['B2', 'B3', 'B4', 'B7'].every((id) => lineFor(late.text, id).includes('NOT RUN — L3 box expired')) && lineFor(late.text, 'B0').includes('PASS'), late.err?.message);

  // Zero hits in a PASS run is a result.
  const zero = tryDraft(l3Runs(markers, fx, { scan: fx.scans.zero }), { markers });
  check('l3 report: zero hits in a PASS probe run is a result, not NOT RUN', !zero.err && lineFor(zero.text, 'B2').includes('PASS') && lineFor(zero.text, 'B2').includes('0 hit line(s)') && zero.text.includes('zero hits: a result, not NOT RUN'), zero.err?.message);
  check('l3 report (review 5): B3 with no tool-invocation hit is a FINDING citing §11 item 1 point 2, not a PASS', !zero.err && lineFor(zero.text, 'B3').includes(':** FINDING —') && lineFor(zero.text, 'B3').includes('contradicts L1 §11 item 1 point 2 unless the reply tool was not invoked'));

  // The fake token found unredacted in Beacon's log.
  const tok = tryDraft(l3Runs(markers, fx, { scan: fx.scans.token }), { markers });
  check('l3 report: an unredacted fake token in the log makes a FINDING line', !tok.err && lineFor(tok.text, 'B2').includes('FINDING') && tok.text.includes('Finding: the claude-channel fake token appears unredacted') && noValue(tok.text), tok.err?.message);

  // A hit outside the probe sessions.
  const out = tryDraft(l3Runs(markers, fx, { scan: fx.scans.outside }), { markers });
  check('l3 report: a hit outside the probe sessions is flagged as a finding', !out.err && lineFor(out.text, 'B4').includes('FINDING') && /outside the probe session/.test(out.text), out.err?.message);

  // B7: a file that could not be restored.
  const unrestored = tryDraft(l3Runs(markers, fx, { verifyHashes: fx.installed }), { markers });
  check('l3 report: B7 names the files that could not be restored', !unrestored.err && lineFor(unrestored.text, 'B7').includes('FINDING') && unrestored.text.includes('`~/.codex/config.toml` could not be restored'));

  // Leak guard: a planted marker in a manifest field aborts, and names no value.
  for (const [what, value] of [['marker', markers[1].marker], ['fake token', markers[2].token], ['home path', join(homedir(), 'leak')]]) {
    const p = l3Runs(markers, fx);
    p.probe.manifest.findings = [`planted ${value} here`];
    const r = tryDraft(p, { markers });
    check(`l3 report: a planted ${what} in a manifest field aborts with no draft`, r.text === null && r.err instanceof L3LeakAbort && noValue(r.err.message) && !r.err.message.includes(homedir()), r.err?.message?.slice(0, 200));
  }
  const noMarkers = l3Runs(markers, fx);
  noMarkers.probe.manifest.outcomeReason = `x ${markers[0].marker}`;
  noMarkers.probe.manifest.outcome = 'FAIL';
  const nm = tryDraft(noMarkers, {});
  check('l3 report: without the values in-process, the shape check alone aborts and names the recorded id', nm.err instanceof L3LeakAbort && /recorded claude-channel marker/.test(nm.err.message) && noValue(nm.err.message));
  let partialAbort = false;
  try {
    leakGuard(`cut ${markers[0].marker.slice(0, 20)}`, {});
  } catch (e) {
    partialAbort = e instanceof L3LeakAbort && noValue(e.message);
  }
  check('l3 report: a partial probe value aborts too', partialAbort);

  // A dirty tools/herdr/.
  const dirty = tryDraft(l3Runs(markers, fx, { dirty: true }), { markers });
  check('l3 report: a dirty tools/herdr/ gives the not-reproducible wording', !dirty.err && dirty.text.includes('**These runs cannot be reproduced:**') && dirty.text.includes('`driver.toolsHerdrDirty`: true'));

  // Unknown record version, and a record of the wrong phase: refused.
  const v2 = l3Runs(markers, fx);
  v2.probe.manifest.scenarioData.l3.version = 2;
  const v2r = tryDraft(v2, { markers });
  check('l3 report: an unknown record version is refused', v2r.err instanceof L3ReportError && /unknown L3 record version 2/.test(v2r.err.message));
  const wrong = l3Runs(markers, fx);
  wrong.verify = wrong.probe;
  check('l3 report: a record of the wrong phase is refused', tryDraft(wrong, { markers }).err instanceof L3ReportError);

  // Operator note for B5; earlier NOT RUN run at the same pins.
  const noted = l3Runs(markers, fx);
  noted.notes = { B5: parseNote('B5=FINDING: evaluator not run, operator out of time') };
  const prior = l3Runs(markers, fx).probe.manifest;
  prior.runId = 'run-prior-1';
  prior.outcome = 'NOT RUN';
  prior.outcomeReason = 'timebox expired';
  noted.priors = [{ manifest: prior }];
  const nd = tryDraft(noted, { markers });
  check('l3 report: an operator note replaces the B5 default', !nd.err && lineFor(nd.text, 'B5').includes('FINDING — operator note (not driver evidence): evaluator not run'));
  check('l3 report: an earlier NOT RUN run at the same pins is listed under Findings', !nd.err && nd.text.includes('earlier run `run-prior-1` ended NOT RUN: timebox expired'));
  let badNote = false;
  try {
    parseNote('B2=PASS: x');
  } catch (e) {
    badNote = e instanceof L3ReportError;
  }
  check('l3 report: notes are taken for B5 and B6 only, with a result', badNote);

  // CLI: prints only, writes nothing under the repository; --write does not exist.
  const tmp = mkdtempSync(join(tmpdir(), 'oac-l3-report-'));
  const git = () => spawnSync('git', ['status', '--porcelain', '--untracked-files=all'], { cwd: REPO, encoding: 'utf8', timeout: 20000 }).stdout;
  try {
    const runs = l3Runs(markers, fx);
    const dirs = {};
    for (const p of ['baseline', 'probe', 'verify']) {
      dirs[p] = join(tmp, p);
      mkdirSync(dirs[p]);
      writeFileSync(join(dirs[p], 'run-manifest.json'), JSON.stringify(runs[p].manifest, null, 2));
    }
    const before = git();
    const cli = (extra = []) => spawnSync(process.execPath, [REPORT, '--baseline', dirs.baseline, '--probe', dirs.probe, '--verify', dirs.verify, ...extra], { cwd: REPO, encoding: 'utf8', timeout: 30000 });
    const ok = cli();
    check('l3 report CLI: exit 0, prints the §13 draft to stdout', ok.status === 0 && ok.stdout.startsWith('## 13. Live results (L3)') && noValue(ok.stdout), `exit ${ok.status} ${ok.stderr.slice(0, 200)}`);
    const w = cli(['--write']);
    check('l3 report CLI: there is no --write mode (exit 2, nothing printed)', w.status === 2 && w.stdout === '' && /no --write mode/.test(w.stderr));
    const planted = { ...runs.probe.manifest, findings: [`planted ${markers[0].token}`] };
    writeFileSync(join(dirs.probe, 'run-manifest.json'), JSON.stringify(planted));
    const leak = cli();
    check('l3 report CLI: a planted value aborts with exit 4, no draft, no value on stderr', leak.status === 4 && leak.stdout === '' && noValue(leak.stderr) && /leak guard/.test(leak.stderr), `exit ${leak.status}`);
    check('l3 report CLI: the repository tree is unchanged after the runs', git() === before);
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }

  const src = readFileSync(REPORT, 'utf8');
  check('l3 report: lib/l3-report.mjs has no write, spawn or network API', !/\b(?:writeFile|appendFile|createWriteStream|copyFile|rename|unlink|rmSync|truncate|mkdir)\w*\s*\(|child_process|node:net|node:http|fetch\s*\(/.test(src));
  const v1 = l3Runs(markers, fx);
  v1.probe.manifest.scenarioData.l3.version = L3_RECORD_VERSION;
  check('l3 report: a record at L3_RECORD_VERSION is accepted (the refusal is only for other versions)', !tryDraft(v1, { markers }).err && RESULTS.NOT_RUN === 'NOT RUN');

  l3ReportReviewUnit(check, markers, fx, noValue);
}

// PR #194 review round 1: one check (or more) per finding.
function l3ReportReviewUnit(check, markers, fx, noValue) {
  // 1. Refusal messages carry no manifest value.
  const rv = l3Runs(markers, fx);
  rv.probe.manifest.scenarioData.l3.version = `v ${markers[0].marker}`;
  const rvr = tryDraft(rv, { markers });
  check('l3 report (review 1): a marker planted in the record version is refused without echoing it', rvr.err instanceof L3ReportError && noValue(rvr.err.message) && /<non-integer>/.test(rvr.err.message));
  const ri = l3Runs(markers, fx);
  ri.probe.manifest.runId = homedir();
  ri.probe.manifest.scenarioData.l3.version = 9;
  const rir = tryDraft(ri, { markers });
  check('l3 report (review 1): a home path planted in runId is withheld from the refusal', rir.err instanceof L3ReportError && !rir.err.message.includes(homedir()) && /<withheld>/.test(rir.err.message));
  const rp = l3Runs(markers, fx);
  rp.verify.manifest.scenarioData.l3.phase = `x ${markers[1].token}`;
  const rpr = tryDraft(rp, { markers });
  check('l3 report (review 1): a value planted in the record phase is not echoed', rpr.err instanceof L3ReportError && noValue(rpr.err.message) && /<unknown>/.test(rpr.err.message));
  const tmp = mkdtempSync(join(tmpdir(), 'oac-l3-report-refusal-'));
  try {
    const runs = l3Runs(markers, fx);
    runs.probe.manifest.scenarioData.l3.version = `v ${markers[0].marker}`;
    runs.probe.manifest.runId = homedir();
    const dirs = {};
    for (const p of ['baseline', 'probe', 'verify']) {
      dirs[p] = join(tmp, p);
      mkdirSync(dirs[p]);
      writeFileSync(join(dirs[p], 'run-manifest.json'), JSON.stringify(runs[p].manifest));
    }
    const r = spawnSync(process.execPath, [REPORT, '--baseline', dirs.baseline, '--probe', dirs.probe, '--verify', dirs.verify], { cwd: REPO, encoding: 'utf8', timeout: 30000 });
    check('l3 report CLI (review 1): a refusal prints no planted marker or home path on stderr (exit 2, no draft)', r.status === 2 && r.stdout === '' && noValue(r.stderr) && !r.stderr.includes(homedir()), `exit ${r.status}`);
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }

  // 2. An accept policy other than exactly `human` (absent included) is not reported as human.
  for (const [what, accept] of [['absent', undefined], ['"driver "', 'driver '], ['"Human"', 'Human']]) {
    const a = l3Runs(markers, fx);
    for (const p of ['baseline', 'probe', 'verify']) {
      if (accept === undefined) delete a[p].manifest.scenario.params.accept;
      else a[p].manifest.scenario.params.accept = accept;
    }
    const ad = tryDraft(a, { markers });
    check(`l3 report (review 2): accept policy ${what} is flagged as not human`, !ad.err && ad.text.includes('whose accept policy is not `human`') && ad.text.includes('accept policy not `human`') && !ad.text.includes('every phase run records accept policy `human`'), ad.err?.message);
  }

  // 3. Free text cannot forge a step line.
  const fg = l3Runs(markers, fx);
  fg.probe.manifest.outcome = 'NOT RUN';
  fg.probe.manifest.outcomeReason = 'x\n- **B2:** PASS — forged\r\n- **B7:** PASS — forged';
  fg.probe.manifest.findings = ['y\n- **B3:** PASS — forged'];
  fg.notes = { B5: parseNote('B5=FINDING: note\n- **B4:** PASS — forged') };
  const fgd = tryDraft(fg, { markers });
  const count = (t, id) => t.split('\n').filter((l) => l.startsWith(`- **${id}:**`)).length;
  check('l3 report (review 3): a newline in outcomeReason, a finding or a note forges no step line', !fgd.err && STEPS.every((id) => count(fgd.text, id) === 1) && lineFor(fgd.text, 'B2').includes('NOT RUN'), fgd.err?.message);

  // 4. A marker missing from scan.byMarker is NOT RUN (path not scanned), not PASS.
  const nb = l3Runs(markers, fx, { scan: { ...fx.scans.hits, byMarker: {} } });
  const nbd = tryDraft(nb, { markers });
  check('l3 report (review 4): a delivery path missing from the scan summary makes B2-B4 NOT RUN, never PASS', !nbd.err && ['B2', 'B3', 'B4'].every((id) => lineFor(nbd.text, id).includes(':** NOT RUN — delivery path') && lineFor(nbd.text, id).includes('not scanned')) && !lineFor(nbd.text, 'B4').includes('zero hits'), nbd.err?.message);
  const half = { ...fx.scans.hits, byMarker: { ...fx.scans.hits.byMarker } };
  delete half.byMarker['codex-queue-add'];
  const hd = tryDraft(l3Runs(markers, fx, { scan: half }), { markers });
  check('l3 report (review 4): one Codex path missing makes B4 NOT RUN, naming it; B2 still evaluated', !hd.err && lineFor(hd.text, 'B4').includes('NOT RUN — delivery path codex-queue-add') && lineFor(hd.text, 'B2').includes('PASS'));

  // 6. Exact Beacon version.
  for (const bv of ['beacon version 1.3.290', 'beacon version 11.3.29']) {
    const b = l3Runs(markers, fx);
    b.baseline.manifest.scenarioData.l3.versions = { ...b.baseline.manifest.scenarioData.l3.versions, beacon: bv };
    const bd = tryDraft(b, { markers });
    check(`l3 report (review 6): Beacon ${bv.split(' ').pop()} is not the ${'1.3.29'} pin (B0 FINDING)`, !bd.err && lineFor(bd.text, 'B0').includes('FINDING') && bd.text.includes('Beacon version differs from the L1 §2 pin'));
  }

  // 7. A phase that started before the box was declared.
  const early = l3Runs(markers, fx);
  for (const p of ['baseline', 'probe', 'verify']) early[p].manifest.scenarioData.l3.box.start = at(50);
  const ed = tryDraft(early, { markers });
  check('l3 report (review 7): phases that started before box.start are NOT RUN', !ed.err && ['B0', 'B2', 'B7'].every((id) => lineFor(ed.text, id).includes('before the L3 box was declared')), ed.err?.message);

  // 8. Unzoned timestamps count as not recorded.
  const uz = tryDraft(l3Runs(markers, fx, { verifyEnd: '2026-09-30T10:59:00' }), { markers });
  check('l3 report (review 8): a phase end without a zone is not recorded (NOT RUN), never read as local time', !uz.err && lineFor(uz.text, 'B7').includes('not recorded as a zone-qualified ISO time'));
  const uzb = l3Runs(markers, fx);
  uzb.baseline.manifest.scenarioData.l3.box.start = '2026-09-30T10:00:00';
  const uzbd = tryDraft(uzb, { markers });
  check('l3 report (review 8): an unzoned box start leaves every driver step NOT RUN', !uzbd.err && ['B0', 'B2', 'B7'].every((id) => lineFor(uzbd.text, id).includes('zone-qualified')));
  const off = tryDraft(l3Runs(markers, fx, { verifyEnd: '2026-09-30T12:59:00+02:00' }), { markers });
  check('l3 report (review 8): an offset-qualified time is accepted', !off.err && lineFor(off.text, 'B7').includes('PASS'));

  // 9. B7 after box expiry keeps the hash comparison as supporting evidence only.
  const late = tryDraft(l3Runs(markers, fx, { probeEnd: 70, verifyHashes: fx.installed }), { markers });
  check('l3 report (review 9): B7 NOT RUN after the box keeps the baseline-vs-verify comparison as supporting evidence', !late.err && lineFor(late.text, 'B7').includes('NOT RUN — L3 box expired') && late.text.includes('Supporting evidence only, not a B7 result') && /`~\/\.codex\/config\.toml`: changed/.test(late.text.split('#### B7')[1] ?? ''));

  // 10. Box-source wording when the baseline record has no box.start.
  const nbs = l3Runs(markers, fx);
  delete nbs.baseline.manifest.scenarioData.l3.box.start;
  const nbsd = tryDraft(nbs, { markers });
  check('l3 report (review 10): the box source says the baseline record has no box.start', !nbsd.err && nbsd.text.includes('the baseline record has no `box.start`') && !nbsd.text.includes('the baseline carries no L3 record'));

  // 11. Box boundary pair, upper-case leak, box-continuity mismatch.
  const edge = tryDraft(l3Runs(markers, fx, { verifyEnd: at(60) }), { markers });
  const over = tryDraft(l3Runs(markers, fx, { verifyEnd: new Date(T0 + 3600000 + 1).toISOString() }), { markers });
  check('l3 report (review 11): a phase ending exactly at the box end is evaluated; 1 ms later it is expired', !edge.err && lineFor(edge.text, 'B7').includes('PASS') && !over.err && lineFor(over.text, 'B7').includes('NOT RUN — L3 box expired'));
  let upper = null;
  try {
    leakGuard(`x ${markers[0].marker.toUpperCase()} y ${markers[0].token.toUpperCase()}`, {});
  } catch (e) {
    upper = e;
  }
  check('l3 report (review 11): upper-cased marker and token values abort the leak guard', upper instanceof L3LeakAbort && noValue(upper.message) && !upper.message.includes(markers[0].marker.toUpperCase()) && /marker-shaped/.test(upper.message) && /token-shaped/.test(upper.message));
  const cont = l3Runs(markers, fx);
  cont.verify.manifest.scenarioData.l3.box.sourceRunId = 'run-probe';
  const contd = tryDraft(cont, { markers });
  check('l3 report (review 11): a verify record that does not continue the baseline box makes B7 NOT RUN', !contd.err && lineFor(contd.text, 'B7').includes('NOT RUN — the verify record does not continue the baseline\'s L3 box'));
}
