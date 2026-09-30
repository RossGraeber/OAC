// L3a (#189) unit checks for lib/l3.mjs, run by the driver self-test
// (`node tools/herdr/run.mjs --self-test`). Unit only, in-process, on synthetic input: no
// herdr, no harness, no Beacon. Runs on Windows too.
//
// Probe values are generated in-process by makeProbeMarkers(). No check name and no failure
// detail ever carries a marker or fake-token value (details give ids and counts only), and no
// probe value appears in this file.

import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
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
}
