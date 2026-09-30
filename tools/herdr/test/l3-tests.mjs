// L3a (#189) unit checks for lib/l3.mjs, run by the driver self-test
// (`node tools/herdr/run.mjs --self-test`). Unit only, in-process, on synthetic input: no
// herdr, no harness, no Beacon. Runs on Windows too.
//
// Probe values are generated in-process by makeProbeMarkers(). No check name and no failure
// detail ever carries a marker or fake-token value (details give ids and counts only), and no
// probe value appears in this file.
//
// L3b (#190) adds l3ScenarioUnit (unit checks of scenarios/l3-beacon.mjs's pure helpers, its
// Beacon allowlist and its source; runs on Windows too) and l3Cases (lifecycle: the three phases
// end to end through run.mjs against the fake herdr, fake Claude Code, fake Codex and
// test/fake-beacon.mjs -- TEST DOUBLES; POSIX only, like every lifecycle case). The lifecycle's
// probe values are generated inside the scenario; the test recovers them only from the fake
// Beacon log the doubles wrote, to prove none reached a manifest, a capture or the driver's output.

import { spawnSync } from 'node:child_process';
import { chmodSync, copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { basename, dirname, isAbsolute, join, relative, resolve } from 'node:path';
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
import l3Scenario, {
  BEACON_ALLOWLIST, BEACON_VERSION, L3_BOX_MS, beaconArgv, parseBeaconVersion, parseBeaconStatus, boxState, claudeProjectSlug, sessionEntryShapes, manifestSafePlaceholders,
  defaultBeaconLog, DEFAULT_REPLY_PROMPT, DEFAULT_THREAD_MARKER, loadBaseline,
} from '../scenarios/l3-beacon.mjs';
import { assertNoSpoof } from '../lib/g5.mjs';
import { CI_SCENARIOS } from '../ci.mjs';
import { installFakeClaudeCli } from './g1-tests.mjs';
import { fakeCodexEnv, stopFakeCodexDaemon } from './g2-tests.mjs';

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

// --- L3b (#190): scenarios/l3-beacon.mjs ---------------------------------------------------

const SCENARIO = join(REPO, 'tools', 'herdr', 'scenarios', 'l3-beacon.mjs');
const GATE_SERVERS = join(REPO, 'tools', 'herdr', 'gate-servers');
const FAKE_BEACON = join(HERE, 'fake-beacon.mjs');
const throwsLike = (fn, re) => {
  try {
    fn();
    return false;
  } catch (e) {
    return !re || re.test(e.message);
  }
};
// Credential file names, built so this file never spells them (oac-boundaries check 10).
const CREDENTIAL_NAMES = [['auth', 'json'].join('.'), ['.credentials', 'json'].join('.')];

export function l3ScenarioUnit(check) {
  // --- the Beacon allowlist ---
  const allow = Object.values(BEACON_ALLOWLIST).map((a) => a.join(' ')).sort();
  check('l3b beacon: the allowlist is exactly the four read-only commands of the 2026-09-30 decision', JSON.stringify(allow) === JSON.stringify(['endpoint claude sync --print', 'endpoint codex sync --print', 'endpoint status --system', 'version']), allow.join(' | '));
  check('l3b beacon: beaconArgv refuses anything off the allowlist, before a process starts', ['install', 'uninstall', 'connect', 'memory', 'hooks', 'endpoint install', '', undefined, '__proto__', 'toString'].every((k) => throwsLike(() => beaconArgv(k), /not on the read-only allowlist/)));
  const a = beaconArgv('claudeSync');
  a.push('--watch');
  check('l3b beacon: beaconArgv hands out a copy; the allowlist is frozen', beaconArgv('claudeSync').join(' ') === 'endpoint claude sync --print' && Object.isFrozen(BEACON_ALLOWLIST) && Object.values(BEACON_ALLOWLIST).every(Object.isFrozen));
  const src = readFileSync(SCENARIO, 'utf8');
  const code = src.split('\n').filter((l) => !/^\s*\/\//.test(l)).join('\n');
  check('l3b beacon: the scenario code names no install/uninstall/connect/memory/hooks/repair subcommand as an argument', !/['"`](?:install|uninstall|connect|memory|hooks|user-config|repair[\w-]*|login|token)['"`]/.test(code) && !/\bintegration\W+install\b/i.test(src));
  check('l3b beacon: \'endpoint\' appears as an argv element only in the allowlist (and the default log path)', (code.match(/['"`]endpoint['"`],\s*['"`](?:status|claude|codex)['"`]/g) ?? []).length === 3 && (code.match(/['"`]endpoint['"`]/g) ?? []).length === 4);
  check('l3b beacon: the scenario never spawns beacon except through the allowlist runner', !/spawn(?:Sync)?\s*\(\s*['"`]beacon/.test(code) && (code.match(/runBounded\(file, args/g) ?? []).length === 1);
  check('l3b: the scenario header says LIVE STATUS: UNVERIFIED and carries the three phase commands', /LIVE STATUS: UNVERIFIED/.test(src) && ['phase=baseline', 'phase=probe', 'phase=verify'].every((p) => src.includes(`--param ${p}`)));
  check('l3b: l3-beacon is not a CI scenario', !CI_SCENARIOS.includes('l3-beacon') && JSON.stringify(CI_SCENARIOS) === JSON.stringify(['smoke', 'g1-claude-wake']));
  check('l3b: scenario defaults: the G5 launch, a 60-minute box, accept=human, readonly Beacon CLI', l3Scenario.name === 'l3-beacon' && JSON.stringify(l3Scenario.harnesses) === '["claude","codex"]' && l3Scenario.defaults.timeboxMs === L3_BOX_MS && L3_BOX_MS === 3600000 && l3Scenario.defaults.params.accept === 'human' && l3Scenario.defaults.params.beaconCli === 'readonly' && l3Scenario.defaults.launch.join(' ') === 'claude --dangerously-load-development-channels server:g5spike');
  check('l3b: no harness-config write in the scenario (JS write API on a config path)', !/(?:writeFile|appendFile|copyFile|rename|rm|unlink)\w*\([^)]*(?:settings\.json|config\.toml|hooks\.json|\.claude\.json)/.test(code));

  // --- parsers ---
  check('l3b parse: beacon version', parseBeaconVersion('beacon version 1.3.29\n') === BEACON_VERSION && parseBeaconVersion('beacon version v1.3.29 (commit abc)') === '1.3.29' && parseBeaconVersion('beacon version 1.3.290') === '1.3.290' && parseBeaconVersion('1.3.29') === null && parseBeaconVersion('') === null);
  const st = parseBeaconStatus('Beacon Endpoint Agent 1.3.29\nConfig: x\nRuntime log: C:\\ProgramData\\Beacon\\Endpoint\\logs\\runtime.jsonl\nService: loaded=true running=false (Access is denied)\nBeacon Managed: not connected\n');
  check('l3b parse: endpoint status log path, Managed and Service lines', st.logPath === 'C:\\ProgramData\\Beacon\\Endpoint\\logs\\runtime.jsonl' && st.managed === 'not connected' && /running=false/.test(st.service) && st.agentVersion === '1.3.29' && parseBeaconStatus('').logPath === null);
  check('l3b: default Beacon log per L1 §12 (Windows system path, else the per-user one)', defaultBeaconLog('win32') === 'C:\\ProgramData\\Beacon\\Endpoint\\logs\\runtime.jsonl' && defaultBeaconLog('linux', '/home/u') === join('/home/u', '.beacon', 'endpoint', 'logs', 'runtime.jsonl'));

  // --- the box ---
  const t0 = Date.parse('2026-09-30T10:00:00.000Z');
  const live = boxState('2026-09-30T10:00:00.000Z', t0 + 59 * 60000);
  const dead = boxState('2026-09-30T10:00:00.000Z', t0 + 60 * 60000);
  check('l3b box: 60 minutes from the baseline\'s start, then expired; never extended', live.valid && !live.expired && live.remainingMs === 60000 && dead.expired && dead.remainingMs === 0 && boxState('not a time').valid === false && boxState(undefined).valid === false);

  // --- session-file shapes: types and flags only ---
  const markers = makeProbeMarkers();
  const cm = markers.find((m) => m.id === 'claude-channel');
  const session = [
    JSON.stringify({ type: 'user', message: { role: 'user', content: 'hello' } }),
    JSON.stringify({ type: 'attachment', isMeta: true, attachment: { type: 'channel_message', content: `x ${cm.marker} y ${cm.token}` } }),
    JSON.stringify({ type: cm.marker, isMeta: 'no', attachment: { [cm.marker]: 1, type: 'a b' } }),
    `not json ${cm.marker}`,
  ].join('\n');
  const shapes = sessionEntryShapes(session, markers);
  check('l3b session file: only entries holding the probe values are described, by type/isMeta/attachment type and key names', shapes.lines === 4 && shapes.entries.length === 3 && shapes.entries[0].type === 'attachment' && shapes.entries[0].isMeta === true && shapes.entries[0].attachmentType === 'channel_message' && shapes.entries[0].attachmentKeys.join() === 'content,type' && shapes.entries[0].tokenPresent === true && shapes.entries[2].parsed === false, JSON.stringify(shapes.entries.map((e) => ({ ...e, attachmentKeys: e.attachmentKeys?.length }))));
  check('l3b session file: a probe value in a type or key never reaches the record', findMarkerLeaks(JSON.stringify(shapes), markers).length === 0 && shapes.entries[1].type === '<non-enum>' && shapes.entries[1].attachmentType === '<non-enum>' && shapes.entries[1].isMeta === null);
  check('l3b: the Claude project slug replaces every non-alphanumeric character', claudeProjectSlug('C:\\sources\\OAC') === 'C--sources-OAC' && claudeProjectSlug('/tmp/oac-herdr-scratch-x/l3-project') === '-tmp-oac-herdr-scratch-x-l3-project');

  // --- excerpts in the manifest ---
  const safe = manifestSafePlaceholders('a <L3-MARKER:claude-channel> b <L3-FAKE-TOKEN:codex-queue-add> c');
  const r = createRedactor();
  check('l3b excerpts: placeholders stored in a form manifest redaction leaves alone', safe === 'a {L3-MARKER claude-channel} b {L3-FAKE-TOKEN codex-queue-add} c' && r.redactValue({ safe }).value.safe === safe && reportIsClean({ ...r.scan(safe), hazardProtocolFrames: [] }));

  // --- operator texts ---
  const table = JSON.parse(readFileSync(CASES, 'utf8'));
  check('l3b: the default reply prompt and thread marker carry no probe value, prefix, spoofing body or frame marker', [DEFAULT_REPLY_PROMPT, DEFAULT_THREAD_MARKER].every((t) => !throwsLike(() => assertNoSpoof('t', t, table)) && !/L3-PROBE-|sk-l3fake-/i.test(t) && findMarkerLeaks(t, markers).length === 0) && /reply tool/i.test(DEFAULT_REPLY_PROMPT));

  // --- baseline loading ---
  const dir = mkdtempSync(join(tmpdir(), 'oac-l3b-unit-'));
  try {
    const put = (m) => writeFileSync(join(dir, 'run-manifest.json'), JSON.stringify(m));
    const good = { runId: 'r1', outcome: 'PASS', scenario: { name: 'l3-beacon' }, scenarioData: { l3: { version: 1, phase: 'baseline', box: { start: '2026-09-30T10:00:00.000Z', budgetMs: L3_BOX_MS }, config: { hashes: [] }, versions: {} } } };
    put(good);
    const ok = loadBaseline(dir);
    put({ ...good, outcome: 'NOT RUN', outcomeReason: 'x' });
    const notRun = throwsLike(() => loadBaseline(dir), /no B0 to compare against/);
    put({ ...good, scenarioData: { l3: { ...good.scenarioData.l3, phase: 'probe' } } });
    const wrongPhase = throwsLike(() => loadBaseline(dir), /not an l3-beacon baseline/);
    put({ ...good, scenarioData: { l3: { ...good.scenarioData.l3, version: 2 } } });
    const wrongVersion = throwsLike(() => loadBaseline(dir), /version 2/);
    check('l3b baseline: a PASS baseline loads; a NOT RUN one, another phase or an unknown record version is refused', ok.runId === 'r1' && notRun && wrongPhase && wrongVersion && throwsLike(() => loadBaseline(''), /baselineRun/));
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }

  // --- fake-beacon answers the allowlist only ---
  const fb = mkdtempSync(join(tmpdir(), 'oac-l3b-fakebeacon-'));
  try {
    const calls = join(fb, 'calls.jsonl');
    const run = (...args) => spawnSync(process.execPath, [FAKE_BEACON, ...args], { env: { ...process.env, FAKE_BEACON_CALLS: calls }, encoding: 'utf8', timeout: 15000 });
    const okV = run('version');
    const bad = [run('endpoint', 'install'), run('endpoint', 'claude', 'sync'), run('memory', 'list'), run('version', '--check')];
    const log = readFileSync(calls, 'utf8').split('\n').filter(Boolean).map((l) => JSON.parse(l));
    check('l3b fake-beacon: answers `beacon version`; refuses (exit 2) and logs anything off the allowlist', okV.status === 0 && parseBeaconVersion(okV.stdout) === '1.3.29' && bad.every((b) => b.status === 2) && log.length === 5 && log.filter((x) => !x.allowed).length === 4);
  } finally {
    rmSync(fb, { recursive: true, force: true });
  }
}

// --- lifecycle --------------------------------------------------------------------------------

const within = (p, root) => {
  const rel = relative(root, p);
  return rel === '' || (!rel.startsWith('..') && !isAbsolute(rel));
};
const FAST = ['--param', 'settleMs=300', '--param', 'pollMs=200', '--param', 'listPollMs=400', '--param', 'wireTimeoutMs=10000', '--param', 'turnTimeoutMs=30000', '--param', 'startupTimeoutMs=20000', '--param', 'handshakeTimeoutMs=20000', '--param', 'attachTimeoutMs=15000', '--param', 'beaconSettleMs=2500', '--param', 'beaconTimeoutMs=20000'];
const SYNTHETIC = {
  'claude-settings': '{\n  "env": { "A": "1" },\n  "hooks": {}\n}\n',
  'claude-state': '{ "numStartups": 1 }\n',
  'codex-config': 'model = "m"\n\n[otel]\nexporter = "none"\n',
  'codex-hooks': '{}\n',
};
const gateServerHashes = () => Object.fromEntries(readdirSync(GATE_SERVERS).sort().map((n) => [n, sha256(readFileSync(join(GATE_SERVERS, n)))]));

// One machine: a fake $HOME with synthetic harness config at Beacon's fixed paths, the harness
// doubles and fake-beacon on PATH, and a fake Beacon runtime log.
function l3World(h, { claudeCli, codexVersion, trace = false, harnessWritesLog = true } = {}) {
  const b = h.makeBase();
  const home = join(b.base, 'home');
  mkdirSync(home);
  // Test setup only: harness-config paths from lib/l3.mjs for the fake home, guarded to the
  // temp dir (the same pattern, and the same reason, as selftest.mjs makeBase()).
  const { targets } = harnessConfigTargets({}, { home });
  const writeConfig = (kind, content) => {
    for (const t of targets.filter((x) => x.kind === kind)) {
      if (!within(t.path, b.base)) throw new Error(`l3 lifecycle refused to write outside its temp dir: ${t.label}`);
      mkdirSync(dirname(t.path), { recursive: true });
      writeFileSync(t.path, content);
    }
  };
  for (const [kind, content] of Object.entries(SYNTHETIC)) writeConfig(kind, content);
  const bin = installFakeClaudeCli(b.base);
  const codexEnv = fakeCodexEnv(b.base, { trace, ...(codexVersion ? { FAKE_CODEX_VERSION: codexVersion } : {}) });
  const beaconBin = join(bin, 'beacon');
  copyFileSync(FAKE_BEACON, beaconBin);
  chmodSync(beaconBin, 0o755);
  const logDir = join(b.base, 'beacon-logs');
  mkdirSync(logDir);
  const log = join(logDir, 'runtime.jsonl');
  writeFileSync(log, `${JSON.stringify({ event: { action: 'session.started' }, harness: { name: 'claude_code', collection_method: 'hook' }, session: { id: 'before-the-probe' } })}\n`);
  const calls = join(b.base, 'beacon-calls.jsonl');
  const env = {
    ...codexEnv,
    HOME: home,
    FAKE_BEACON_CALLS: calls,
    FAKE_CLAUDE_SELF_ACCEPT_MS: '1000',
    FAKE_CODEX_SELF_ACCEPT_MS: '1000',
    FAKE_CLAUDE_SESSION_FILE: '1',
    FAKE_CODEX_LONG_MS: '4500',
    ...(claudeCli ? { FAKE_CLAUDE_CLI_VERSION: claudeCli, FAKE_CLAUDE_VERSION: claudeCli } : {}),
    ...(harnessWritesLog ? { FAKE_BEACON_LOG: log } : {}),
  };
  let n = 0;
  const drive = (phase, args = [], extraEnv = {}) => {
    const tag = `${phase}-${++n}`;
    const bp = { ...b, state: join(b.base, `herdr-state-${tag}`) };
    const out = join(b.base, `out-${tag}`);
    const res = spawnSync(process.execPath, [h.RUN, '--scenario', 'l3-beacon', '--herdr-bin', h.FAKE, '--out', out, '--param', `phase=${phase}`, '--param', `beaconBin=${beaconBin}`, '--param', `beaconLog=${log}`, ...FAST, ...args], {
      env: h.driverEnv(bp, 'fake-claude,fake-codex', { ...env, ...extraEnv }),
      encoding: 'utf8',
      timeout: 240000,
    });
    const read = (p) => (existsSync(p) ? readFileSync(p, 'utf8') : '');
    const lines = (p) => read(p).split('\n').filter(Boolean).map((l) => JSON.parse(l));
    const manifestText = read(join(out, 'run-manifest.json'));
    const r = {
      status: res.status,
      stdout: res.stdout,
      stderr: res.stderr,
      out,
      manifestText,
      manifest: manifestText ? JSON.parse(manifestText) : null,
      calls: lines(join(bp.state, 'calls.log')),
      prompts: lines(join(bp.state, 'prompts.log')),
      outFiles: () => (existsSync(out) ? readdirSync(out).map((f) => ({ name: f, text: read(join(out, f)) })) : []),
    };
    h.invariants(`l3 ${tag}`, bp, r);
    return r;
  };
  const beaconCalls = () => (existsSync(calls) ? readFileSync(calls, 'utf8').split('\n').filter(Boolean).map((l) => JSON.parse(l)) : []);
  const cleanup = () => {
    stopFakeCodexDaemon(b.env.CODEX_HOME);
    rmSync(b.base, { recursive: true, force: true });
  };
  return { b, home, log, beaconBin, env, drive, writeConfig, beaconCalls, cleanup };
}

// The probe values, recovered only from the fake Beacon log the doubles wrote, matched to the
// run's recorded hashes. Never printed.
function recoverMarkers(logText, records) {
  const found = [...new Set([...String(logText).matchAll(/L3-PROBE-[0-9a-z]{26}|sk-l3fake-[0-9a-z]{26}/g)].map((x) => x[0]))];
  return records.map((rec) => ({
    id: rec.id,
    marker: found.find((v) => sha256(v) === rec.markerSha256) ?? null,
    token: found.find((v) => sha256(v) === rec.tokenSha256) ?? null,
  }));
}
function leaksIn(runs, markers) {
  const usable = markers.filter((m) => m.marker && m.token);
  const where = [];
  for (const r of runs) {
    for (const [what, text] of [['stdout', r.stdout], ['stderr', r.stderr], ['run-manifest.json', r.manifestText], ...r.outFiles().map((f) => [f.name, f.text])]) {
      const leaks = findMarkerLeaks(text, usable);
      if (leaks.length) where.push(`${what}: ${leaks.map((l) => `${l.id} ${l.what}`).join(', ')}`);
    }
    for (const p of r.prompts) if (findMarkerLeaks(p.text, usable).length || /L3-PROBE-|sk-l3fake-/.test(p.text)) where.push('a typed prompt');
    for (const c of r.manifest?.commands ?? []) if (findMarkerLeaks(JSON.stringify(c.argv), usable).length) where.push(`herdr command #${c.seq}`);
  }
  return where;
}

export async function l3Cases(check, h) {
  const gsBefore = gateServerHashes();
  const agentStarts = (r) => r.calls.filter((c) => c.argv.includes('agent') && c.argv.includes('start')).length;

  // --- happy path, all three phases, traced ---
  {
    const w = l3World(h, { trace: true });
    try {
      const base = w.drive('baseline');
      const bm = base.manifest;
      const b3 = bm?.scenarioData?.l3;
      check('l3 baseline: PASS; the box declared; Beacon 1.3.29 read through the allowlist; no harness launched', base.status === 0 && bm.outcome === 'PASS' && !!b3.box.start && b3.box.budgetMs === L3_BOX_MS && b3.versions.beacon === '1.3.29' && agentStarts(base) === 0, `${base.status} ${bm?.outcomeReason}`);
      check('l3 baseline: the four fixed-$HOME files and the env-dir equivalents hashed; env names recorded, never values; trailing-comment check recorded', b3.config.hashes.filter((x) => x.present).length >= 6 && b3.config.envSet.CLAUDE_CONFIG_DIR === true && b3.config.envSet.CODEX_HOME === true && bm.findings.some((f) => /CLAUDE_CONFIG_DIR is set/.test(f)) && Array.isArray(b3.config.trailingComment) && !base.manifestText.includes('exporter = '), JSON.stringify(b3.config.hashes.map((x) => [x.label, x.present])));
      check('l3 baseline: status --system recorded (log path, Managed not connected)', b3.beacon.status.managed === 'not connected' && !!b3.beacon.status.logPath);

      // The operator's Beacon step between B0 and the probe: [otel] rewritten in config.toml.
      w.writeConfig('codex-config', 'model = "m"\n\n[otel]\nexporter = { otlp-http = { endpoint = "http://127.0.0.1:4318" } }\n');
      const probe = w.drive('probe', ['--param', `baselineRun=${base.out}`]);
      const pm = probe.manifest;
      const l3 = pm?.scenarioData?.l3;
      check('l3 probe: PASS (exit 0)', probe.status === 0 && pm.outcome === 'PASS', `${probe.status} ${pm?.outcome} ${pm?.outcomeReason}`);
      if (pm?.outcome === 'PASS') {
        check('l3 probe B1: NOT RUN by the operator decision, with the baseline-to-probe diff naming config.toml [otel], attributed to the operator\'s Beacon step', l3.steps.B1.status === 'NOT RUN' && /2026-09-30/.test(l3.steps.B1.reason) && l3.config.compare.files.some((f) => f.label === '~/.codex/config.toml' && f.sections.changed.includes('[otel]')) && pm.findings.some((f) => /attributed to the operator's Beacon step/.test(f)));
        check('l3 probe: three probe markers recorded as ids and hashes only', l3.markers.length === 3 && l3.markers.every((m) => /^[0-9a-f]{64}$/.test(m.markerSha256) && /^[0-9a-f]{64}$/.test(m.tokenSha256) && Object.keys(m).length === 3));
        check('l3 probe: only the staged case table was augmented; the committed one is unchanged', l3.staging.casesAugmentedSha256 !== l3.staging.casesStagedSha256 && l3.staging.committedCasesUnchanged === true && l3.staging.files.every((f) => /^tools\/herdr\/gate-servers\//.test(f.path)));
        const bm2 = l3.steps.B2.log.cumulative.byMarker;
        check('l3 probe B2: the channel probe reached the (fake) Beacon log; the fake token appears verbatim; the dialog was accepted by the human (fake self-accept)', bm2['claude-channel'].lines >= 1 && bm2['claude-channel'].tokenVerbatimLines >= 1 && l3.steps.B2.dialogs.length >= 1 && l3.steps.B2.dialogs.every((d) => d.acceptOrigin === 'human'), JSON.stringify({ bm2, dialogs: l3.steps.B2.dialogs }));
        check('l3 probe B3: the reply tool was called with the probe marker (in-process check); B3 counted separately from B2', l3.steps.B3.replyToolCalls === 1 && l3.steps.B3.replyArgsCarryMarker === true && l3.steps.B3.log.delta.byMarker['claude-channel'].actions.includes('mcp.tool_invoked') && !l3.steps.B2.log.cumulative.byMarker['claude-channel'].actions.includes('mcp.tool_invoked'), JSON.stringify(l3.steps.B3.log.delta.byMarker['claude-channel']));
        check('l3 probe B4: turn/start and thread/queue/add both completed and both reached the (fake) log', l3.steps.B4.turnStart.status === 'completed' && l3.steps.B4.queueAdd.status === 'completed' && l3.steps.B4.turnStart.recordedByteIdentical && l3.scan.byMarker['codex-turn-start'].lines >= 1 && l3.scan.byMarker['codex-queue-add'].lines >= 1);
        check('l3 probe: poll-path counts from sync --print for B2 and B4', l3.beacon.sync.B2.byMarker['claude-channel'].lines >= 1 && l3.beacon.sync.B2.byMarker['claude-channel'].collectionMethods.includes('poll') && l3.beacon.sync.B4.byMarker['codex-turn-start'].lines >= 1 && l3.beacon.sync.B4.truncated === false);
        check('l3 probe: whole-run scan with redacted excerpts in the manifest-safe placeholder form', l3.scan.hitCount >= 4 && l3.scan.excerpts.length >= 4 && l3.scan.excerpts.every((e) => /\{L3-(?:MARKER|FAKE-TOKEN) [a-z-]+\}/.test(e.text) && !/<L3-/.test(e.text)) && l3.scan.bySession.every((s) => s.sessionId !== 'before-the-probe'), JSON.stringify(l3.scan.excerpts.map((e) => e.text.slice(0, 60))));
        check('l3 probe: the session file described by entry type and flags only', l3.sessionFile.read === true && l3.sessionFile.dirsFound === 1 && l3.sessionFile.entries.some((e) => e.type === 'attachment' && e.isMeta === true && e.attachmentType === 'channel_message') && l3.sessionFile.entries.every((e) => !('content' in e)), JSON.stringify(l3.sessionFile));
        check('l3 probe: B5 and B6 NOT RUN; the daemon state is a finding (not running before)', l3.steps.B5.status === 'NOT RUN' && l3.steps.B6.status === 'NOT RUN' && l3.daemon.alreadyRunning === false && pm.findings.some((f) => /daemon state: not running before/.test(f)));
        check('l3 probe: no pin drift at the PINS.md versions', !pm.findings.some((f) => /pin drift/.test(f)) && l3.versions.pins.claude.differs === false && l3.versions.pins.codex.differs === false, JSON.stringify(pm.findings));
        check('l3 probe: captures written clean, none fixture-shaped', pm.captures.length === 4 && pm.captures.every((c) => c.written && /^l3-/.test(c.file) && !/-herdr\./.test(c.file)), JSON.stringify(pm.captures.map((c) => [c.file, c.written])));
        check('l3 probe: typed text was only the reply prompt and the thread marker', probe.prompts.map((p) => p.text).sort().join('|') === [DEFAULT_REPLY_PROMPT, DEFAULT_THREAD_MARKER].sort().join('|'));
      }

      // The operator's B7 restore, then the verify phase.
      w.writeConfig('codex-config', SYNTHETIC['codex-config']);
      const ver = w.drive('verify', ['--param', `baselineRun=${base.out}`]);
      const v3 = ver.manifest?.scenarioData?.l3;
      check('l3 verify: PASS; restored files compare unchanged with B0; no harness, no Beacon command', ver.status === 0 && v3.steps.B7.notRestored.length === 0 && v3.config.compare.unchanged && agentStarts(ver) === 0 && v3.beacon.calls.length === 0, `${ver.status} ${ver.manifest?.outcomeReason}`);

      // Beacon allowlist, from the fake's own call log.
      const calls = w.beaconCalls();
      const argvs = [...new Set(calls.map((c) => c.argv.join(' ')))].sort();
      check('l3 beacon: every Beacon invocation across the three phases was on the allowlist; all four were used', calls.every((c) => c.allowed) && JSON.stringify(argvs) === JSON.stringify(['endpoint claude sync --print', 'endpoint codex sync --print', 'endpoint status --system', 'version']), argvs.join(' | '));
      check('l3 herdr: the hook-writing command family was never called', [base, probe, ver].every((r) => r.calls.every((c) => !c.argv.includes('integration'))));

      // Marker leak: values recovered from the fake log only.
      const markers = l3 ? recoverMarkers(readFileSync(w.log, 'utf8'), l3.markers) : [];
      check('l3 leak: all six probe values recovered from the fake Beacon log (by hash)', markers.length === 3 && markers.every((m) => m.marker && m.token));
      const leaks = leaksIn([base, probe, ver], markers);
      check('l3 leak: no probe value in any run manifest, capture, driver stdout/stderr, typed prompt or herdr argv', markers.length === 3 && leaks.length === 0, leaks.join('; '));

      // File-access trace.
      const trace = readFileSync(join(w.b.base, 'fs-trace.jsonl'), 'utf8').split('\n').filter(Boolean).map((l) => JSON.parse(l));
      const RUN_JS = resolve(h.RUN);
      const driver = trace.filter((t) => t.script && resolve(t.script) === RUN_JS);
      const homes = [join(w.home, '.claude'), join(w.home, '.codex'), join(w.home, '.claude.json'), w.b.env.CLAUDE_CONFIG_DIR, w.b.env.CODEX_HOME];
      const underHome = (t) => t.path && homes.some((x) => within(t.path, x));
      const hashed = new Set(harnessConfigTargets(w.b.env, { home: w.home }).targets.map((t) => t.path));
      const sessionDir = join(w.b.env.CLAUDE_CONFIG_DIR, 'projects');
      const allowedRead = (t) => hashed.has(t.path) || (within(t.path, sessionDir) && /[\\/]projects[\\/][^\\/]*l3-project(?:[\\/][^\\/]+\.jsonl)?$/.test(t.path));
      check('l3 trace: the tracer saw the driver in all three phases', new Set(driver.map((t) => t.pid)).size >= 3 && driver.length > 100, String(driver.length));
      check('l3 trace: the driver wrote nothing under either harness home', driver.filter(underHome).every((t) => t.kind !== 'write'), JSON.stringify([...new Set(driver.filter(underHome).filter((t) => t.kind === 'write').map((t) => `${t.op} ${relative(w.b.base, t.path)}`))]));
      check('l3 trace: the driver read nothing under the harness homes but the hashed config files and the probe project\'s session file', driver.filter(underHome).every((t) => t.kind === 'fs' && allowedRead(t)), JSON.stringify([...new Set(driver.filter(underHome).filter((t) => !allowedRead(t)).map((t) => `${t.op} ${relative(w.b.base, t.path)}`))]));
      check('l3 trace: the driver read the probe session file (readSessionFile=true)', driver.some((t) => t.op === 'readFileSync' && within(t.path, sessionDir) && t.path.endsWith('.jsonl')));
      const spawned = [...new Set(driver.filter((t) => t.kind === 'spawn').map((t) => t.file))];
      check('l3 trace: no credential file and no OS credential-store tool touched by the driver', driver.every((t) => !t.path || !CREDENTIAL_NAMES.includes(basename(t.path))) && spawned.every((f) => [process.execPath, 'git', 'codex', 'claude', 'ps', w.beaconBin].includes(f)), JSON.stringify(spawned));
      check('l3 trace: no harness config file copied or written anywhere by the driver', driver.every((t) => !(t.kind === 'write' || /^(?:copyFile|cp)/.test(t.op ?? '')) || !['settings.json', 'config.toml', 'hooks.json', '.claude.json'].includes(basename(t.path ?? ''))) && driver.filter((t) => /^(?:copyFile|cp)/.test(t.op ?? '')).every((t) => !underHome(t)));
      check('l3 trace: beacon ran only with allowlisted argv', driver.filter((t) => t.kind === 'spawn' && t.file === w.beaconBin).length >= 5 && driver.filter((t) => t.kind === 'spawn' && t.file === w.beaconBin).every((t) => Object.values(BEACON_ALLOWLIST).some((x) => x.join(' ') === (t.args ?? []).join(' '))));
    } catch (err) {
      check('l3 happy path: assertions ran', false, err.stack);
    } finally {
      w.cleanup();
    }
  }

  // --- expired box -> NOT RUN, nothing launched, no Beacon command ---
  {
    const w = l3World(h);
    try {
      const base = w.drive('baseline');
      const mp = join(base.out, 'run-manifest.json');
      const m = JSON.parse(readFileSync(mp, 'utf8'));
      m.scenarioData.l3.box.start = new Date(Date.now() - 2 * L3_BOX_MS).toISOString();
      writeFileSync(mp, JSON.stringify(m));
      const before = w.beaconCalls().length;
      const p = w.drive('probe', ['--param', `baselineRun=${base.out}`]);
      check('l3 expired box: NOT RUN (exit 3) before any harness or Beacon command', p.status === 3 && /has expired/.test(p.manifest?.outcomeReason) && agentStarts(p) === 0 && w.beaconCalls().length === before, `${p.status} ${p.manifest?.outcomeReason}`);
    } catch (err) {
      check('l3 expired box: assertions ran', false, err.stack);
    } finally {
      w.cleanup();
    }
  }

  // --- Beacon version mismatch -> NOT RUN (baseline, and a probe after a good baseline) ---
  {
    const w = l3World(h);
    try {
      const bad = w.drive('baseline', [], { FAKE_BEACON_VERSION: '1.3.30' });
      check('l3 Beacon version: a baseline against 1.3.30 is NOT RUN', bad.status === 3 && /1\.3\.30/.test(bad.manifest?.outcomeReason) && /1\.3\.29/.test(bad.manifest?.outcomeReason), bad.manifest?.outcomeReason);
      const base = w.drive('baseline');
      const p = w.drive('probe', ['--param', `baselineRun=${base.out}`], { FAKE_BEACON_VERSION: '1.3.30' });
      check('l3 Beacon version: a probe against 1.3.30 is NOT RUN before anything launches', p.status === 3 && /1\.3\.30/.test(p.manifest?.outcomeReason) && agentStarts(p) === 0 && !p.manifest.scenarioData.l3.markers.length, p.manifest?.outcomeReason);
    } catch (err) {
      check('l3 Beacon version: assertions ran', false, err.stack);
    } finally {
      w.cleanup();
    }
  }

  // --- harness versions off PINS.md and a pre-existing daemon: findings, not stops ---
  {
    const w = l3World(h, { claudeCli: '2.1.999', codexVersion: '0.999.0' });
    try {
      const base = w.drive('baseline');
      spawnSync('codex', ['app-server', 'daemon', 'start'], { env: { ...process.env, ...w.b.env, ...w.env }, encoding: 'utf8', timeout: 20000 });
      const p = w.drive('probe', ['--param', `baselineRun=${base.out}`, '--param', 'readSessionFile=false']);
      const m = p.manifest;
      const l3 = m?.scenarioData?.l3;
      check('l3 pin drift: PASS, with a pin-drift finding per harness; PINS.md is not edited', p.status === 0 && m.outcome === 'PASS' && m.findings.some((f) => /pin drift.*`claude --version` reports v2\.1\.999/.test(f)) && m.findings.some((f) => /pin drift.*`codex --version` reports v0\.999\.0/.test(f)) && l3.versions.pins.claude.differs && l3.versions.pins.codex.differs, `${p.status} ${m?.outcomeReason} ${JSON.stringify(m?.findings)}`);
      check('l3 pre-existing daemon: recorded as a finding, not a stop', l3?.daemon?.alreadyRunning === true && m.findings.some((f) => /ALREADY RUNNING/.test(f)));
      check('l3 readSessionFile=false: the session file is not read', l3?.sessionFile?.read === false);
    } catch (err) {
      check('l3 pin drift: assertions ran', false, err.stack);
    } finally {
      w.cleanup();
    }
  }

  // --- zero hits is a result, not a failure; beaconCli=off runs no Beacon command ---
  {
    const w = l3World(h, { harnessWritesLog: false });
    try {
      const base = w.drive('baseline');
      const p = w.drive('probe', ['--param', `baselineRun=${base.out}`]);
      const l3 = p.manifest?.scenarioData?.l3;
      check('l3 zero hits: PASS with zero hits on every path, recorded, not NOT RUN', p.status === 0 && p.manifest.outcome === 'PASS' && Object.values(l3.scan.byMarker).every((x) => x.lines === 0) && l3.scan.hitCount === 0 && l3.steps.B2.log.hitSeenAfterMs === null, `${p.status} ${p.manifest?.outcomeReason}`);
      const before = w.beaconCalls().length;
      const off = w.drive('baseline', ['--param', 'beaconCli=off', '--param', 'beaconVersion=1.3.29', '--param', 'beaconBin=']);
      const o3 = off.manifest?.scenarioData?.l3;
      check('l3 beaconCli=off: PASS, no Beacon command, version and log recorded as operator-supplied', off.status === 0 && w.beaconCalls().length === before && /operator-supplied/.test(o3.versions.beaconSource) && /operator-supplied/.test(o3.beacon.log.source), `${off.status} ${off.manifest?.outcomeReason}`);
    } catch (err) {
      check('l3 zero hits: assertions ran', false, err.stack);
    } finally {
      w.cleanup();
    }
  }

  const gsAfter = gateServerHashes();
  check('l3: the committed tools/herdr/gate-servers/ files are byte-unchanged after every L3 run', JSON.stringify(gsBefore) === JSON.stringify(gsAfter));
}
