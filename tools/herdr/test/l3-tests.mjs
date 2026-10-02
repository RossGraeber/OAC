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
import { fileURLToPath, pathToFileURL } from 'node:url';
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
import l3Scenario, {
  BEACON_ALLOWLIST, BEACON_VERSION, L3_BOX_MS, beaconArgv, parseBeaconVersion, parseBeaconStatus, boxState, claudeProjectSlug, sessionEntryShapes, manifestSafePlaceholders,
  defaultBeaconLog, beaconManagedIsLocal, BEACON_MANAGED_LOCAL_VALUES, DEFAULT_REPLY_PROMPT, DEFAULT_THREAD_MARKER, loadBaseline, hitCounts, BEACON_BIN_NAME, STATUS_SIDE_EFFECTS, streamScan, probeSessionIds,
} from '../scenarios/l3-beacon.mjs';
import { assertNoSpoof } from '../lib/g5.mjs';
import { CI_SCENARIOS } from '../ci.mjs';
import { installFakeClaudeCli } from './g1-tests.mjs';
import { fakeCodexEnv, stopFakeCodexDaemon } from './g2-tests.mjs';
import { parseClaudeVersions } from '../lib/pins.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, '..', '..', '..');
// The fake Claude reports PINS.md's last tested version unless a case sets another (#216).
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
  check('l3 report: poll path NOT RUN for a record without the sync --print field (pre-#195 record)', d.includes('`beacon endpoint claude sync --print`) at B2: NOT RUN (this record carries no poll-path result') && d.includes('`beacon endpoint codex sync --print`) at B4: NOT RUN') && d.includes('### Not recorded by the probe record') && /`sync --print` poll-path counts/.test(d));
  check('l3 report: header carries versions, pins, date, box and each phase\'s start and end', d.includes('- **Date:** 2026-09-30') && d.includes('`beacon version 1.3.29`') && d.includes('PINS.md last observed `2.1.284`') && d.includes(`ends ${at(60)}`) && d.includes(`\`${at(10)}\` to \`${at(30)}\``));
  check('l3 report: Driver lines as g1-report renders them, plus toolsHerdrDirty', d.includes('- **Driver (probe):** herdr (`herdr 0.9.1`, PINS.md `herdr (test tooling)` v0.9.1) via `tools/herdr/run.mjs`, scenario `tools/herdr/scenarios/l3-beacon.mjs`, driver commit') && d.includes('`driver.toolsHerdrDirty`: false'));
  check('l3 report: states herdr-driven, accept=human per phase, not a gate result', /herdr-driven/.test(d) && d.includes('probe `accept=human` (the driver sent no dialog key)') && d.includes('accepted by me, a human at the keyboard') && /not a gate result/.test(d) && /changes no verdict/.test(d));
  check('l3 report: operator attestation present and unticked, naming Beacon', d.includes('### Operator attestation') && (d.match(/^- \[ \] /gm) ?? []).length === 4 && !/^- \[x\]/im.test(d) && d.includes('**Beacon:**') && d.includes('**Attested by:** <operator>'));
  check('l3 report: no probe value, raw log text, config content, home path or username in the draft', noValue(d) && !d.includes(LOG_CANARY) && !d.includes(CONFIG_CANARY) && !d.includes(homedir()) && !(userName.length >= 4 && d.toLowerCase().includes(userName.toLowerCase())));
  check('l3 report: the draft scans clean (redactor, placeholders neutralized)', clean(createRedactor(), d));

  // Pin drift is a finding, not a stop.
  const drift = tryDraft(l3Runs(markers, fx, { drift: true }), { markers });
  check('l3 report: pin drift makes B0 a FINDING and is listed under Findings; the leg continues', !drift.err && lineFor(drift.text, 'B0').includes('FINDING') && /VERSION WARNING: Claude Code installed version differs from PINS.md/.test(drift.text) && lineFor(drift.text, 'B2').includes('PASS'));

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

  // 2. An accept policy other than exactly `human` or exactly `driver` (absent included), or a
  // `human` run holding a dialog-accept command, is never reported as a human accept (#196).
  for (const [what, accept, cmd] of [['absent', undefined], ['"driver "', 'driver '], ['"Human"', 'Human'], ['human with a dialog-accept command', 'human', true]]) {
    const a = l3Runs(markers, fx);
    for (const p of ['baseline', 'probe', 'verify']) {
      if (accept === undefined) delete a[p].manifest.scenario.params.accept;
      else a[p].manifest.scenario.params.accept = accept;
      if (cmd) a[p].manifest.commands.push({ seq: 7, role: 'dialog-accept', argv: ['herdr', 'agent', 'send-keys', 'l3claude', 'enter'] });
    }
    const ad = tryDraft(a, { markers });
    check(`l3 report (review 2): accept policy ${what} is reported as origin not established, never as human`, !ad.err && ad.text.includes('accept origin NOT established') && ad.text.includes('accept origin not established in the baseline, probe, verify run(s)') && !ad.text.includes('accepted by me, a human') && !ad.text.includes('`accept=human` (the driver sent no dialog key)'), ad.err?.message);
  }
  // #196: a driver-accepted probe is rendered as the driver's, with its keys, everywhere.
  {
    const a = l3Runs(markers, fx);
    for (const p of ['baseline', 'probe', 'verify']) a[p].manifest.scenario.params.accept = 'driver';
    a.probe.manifest.scenarioData.l3.acceptPolicy = 'driver';
    a.probe.manifest.scenarioData.l3.dialogs = [
      { agent: 'claude', kind: 'workspace-trust', readSeq: 11, acceptOrigin: 'driver', acceptSeq: 14, acceptKeys: [{ key: 'down', seq: 12 }, { key: 'enter', seq: 14 }] },
      { agent: 'claude', kind: 'dev-channels', readSeq: 20, acceptOrigin: 'driver', acceptSeq: 21, acceptKeys: [{ key: 'enter', seq: 21 }] },
    ];
    const dd = tryDraft(a, { markers });
    const t = dd.text ?? '';
    check('l3 report #196: accept=driver runs are stated as driver accepts (header, finding, attestation), never as a human\'s', !dd.err && t.includes('probe `accept=driver` (accepted by the driver, #196)') && t.includes('accepted by the DRIVER, not by a human: claude workspace-trust (read #11; accepted by the DRIVER (herdr dialog-accept: down #12, enter #14))') && /- \[ \] \*\*Consent dialog:\*\* accepted by the DRIVER \(`accept=driver`, #196\), not by me/.test(t) && !t.includes('accepted by me, a human'), dd.err?.message ?? t.split('\n').filter((l) => /DRIVER|accept/.test(l)).join(' || '));
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

  // 7. Box declaration vs. phase starts. The baseline opens the box: run.mjs records its
  // timebox.start before setup, so box.start lands slightly after it.
  const boxAt = (runs, iso) => {
    for (const p of ['baseline', 'probe', 'verify']) runs[p].manifest.scenarioData.l3.box.start = iso;
    return runs;
  };
  const late2s = tryDraft(boxAt(l3Runs(markers, fx), new Date(T0 + 2000).toISOString()), { markers });
  check('l3 report (review 7): a box declared 2 s after the baseline run started leaves B0 evaluated (PASS), and later phases too', !late2s.err && lineFor(late2s.text, 'B0').includes(':** PASS —') && lineFor(late2s.text, 'B2').includes(':** PASS —') && lineFor(late2s.text, 'B7').includes(':** PASS —'), late2s.err?.message);
  const pre = tryDraft(boxAt(l3Runs(markers, fx), at(12)), { markers });
  check('l3 report (review 7): a probe that started before box.start makes B2-B4 NOT RUN', !pre.err && ['B2', 'B3', 'B4'].every((id) => lineFor(pre.text, id).includes('the probe run started') && lineFor(pre.text, id).includes('before the L3 box was declared')), pre.err?.message);
  const afterBase = tryDraft(boxAt(l3Runs(markers, fx), at(6)), { markers });
  check('l3 report (review 7): a box declared after the baseline run ended makes B0 NOT RUN', !afterBase.err && lineFor(afterBase.text, 'B0').includes(':** NOT RUN — the L3 box was declared') && lineFor(afterBase.text, 'B0').includes('after the baseline run ended'), afterBase.err?.message);

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

export async function l3ScenarioUnit(check) {
  // --- #195 re-review: probe sessions unknown vs empty; B3 tool action from the whole run ---
  check('l3b probeSessionIds: no session file found (or not read) and no thread id record null (unknown), never an empty list', probeSessionIds({ read: true, sessionIds: [] }, null).claude === null && probeSessionIds({ read: true }, null).claude === null && probeSessionIds({ read: false }, 't').claude === null && probeSessionIds(null, null).codex === null && JSON.stringify(probeSessionIds({ read: true, sessionIds: ['b', 'a'] }, 't')) === JSON.stringify({ ...probeSessionIds({ read: true, sessionIds: ['b', 'a'] }, 't'), claude: ['a', 'b'], codex: ['t'] }));
  {
    const mk = makeProbeMarkers();
    const fx = l3Fixtures(mk);
    const withRecord = (extra) => {
      const runs = l3Runs(mk, fx);
      Object.assign(runs.probe.manifest.scenarioData.l3, extra);
      return runs;
    };
    const counts = { 'claude-channel': { lines: 2, markerLines: 2, tokenLines: 0, byAction: { 'prompt.submitted': 1, 'mcp.tool_invoked': 1 }, byPath: {}, byCollectionMethod: {}, byHarness: {} } };
    // The B3 delta closed on a late B2 line: it lacks the tool line; the whole-run scan has it.
    const steps = { B3: { replyToolCalls: 1, replyArgsCarryMarker: true, log: { delta: { counts: { 'claude-channel': { lines: 1, byAction: { 'prompt.submitted': 1 } } } } } } };
    const scan = { ...fx.scans.hits, counts };
    const late = draftL3(withRecord({ scan, steps, probeSessions: { claude: ['s-claude-1'], codex: ['s-codex-1'] } }));
    check('l3 report (#195 re-review): a B3 delta without the tool line, while the whole-run scan has it, gives no contradiction finding', !/contradicts L1 §11 item 1 point 2/.test(late) && /- \*\*B3:\*\* PASS/.test(late) && /within the B3 snapshot: none/.test(late), late.split('\n').filter((l) => /B3/.test(l)).join(' || '));
    const empty = draftL3(withRecord({ scan, steps, probeSessions: { claude: [], codex: [] } }));
    const unknown = draftL3(withRecord({ scan, steps, probeSessions: { claude: null, codex: null } }));
    const other = draftL3(withRecord({ scan, steps, probeSessions: { claude: ['s-claude-other'], codex: ['s-codex-1'] } }));
    check('l3 report (#195 re-review): an empty or null probe-session list is unknown, not "every hit outside"; a recorded list that misses the hit session still flags it', !/not a recorded probe session/.test(empty) && !/not a recorded probe session/.test(unknown) && /session `s-claude-1`, which is not a recorded probe session/.test(other));
  }

  // --- streamScan: no cap, keeps needle lines only; a timeout is reported, never thrown ---
  const needle = ['needle', 'l3b', 'unit'].join('-');
  const emit = `const l='x'.repeat(200)+'\\n';const c=l.repeat(5000);let n=0;function w(){while(n<12){n++;if(!process.stdout.write(c)){process.stdout.once('drain',w);return;}}process.stdout.write('last ${needle} line\\n');}w();`;
  const big = await streamScan(process.execPath, ['-e', emit], { deadlineMs: 30000, env: process.env, needles: [needle] });
  check('l3b streamScan: 12 MB of output streamed past any cap, the needle line (last) kept, nothing else kept', big.exitCode === 0 && !big.timedOut && big.bytes > 12 * 1000 * 1000 && big.lines === 60001 && big.kept.length === 1 && big.kept[0].includes(needle), JSON.stringify({ ...big, kept: big.kept.length }));
  const slow = await streamScan(process.execPath, ['-e', 'setTimeout(()=>{},60000)'], { deadlineMs: 500, env: process.env, needles: [needle] });
  check('l3b streamScan: a hanging child is killed at its deadline and reported timedOut', slow.timedOut === true && slow.kept.length === 0);
  check('l3b streamScan: refuses to run without a deadline', await (async () => {
    try {
      await streamScan(process.execPath, ['-v'], {});
      return false;
    } catch {
      return true;
    }
  })());

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
  check('l3b beacon: the scenario starts Beacon only via beaconCommand (the executable itself plus an allowlisted argv), never under an interpreter', !/spawn(?:Sync)?\s*\(\s*['"`]beacon/.test(code) && !/(?:runBounded|streamScan|spawn)\(\s*(?:beaconBin|process\.execPath,\s*\[beaconBin)/.test(code) && (code.match(/= beaconCommand\(key\)/g) ?? []).length === 2 && !code.includes('.test(beaconBin)'));
  check('l3b beacon: --param beaconBin must be named beacon or beacon.exe', ['beacon', 'beacon.exe', 'BEACON.EXE'].every((n) => BEACON_BIN_NAME.test(n)) && ['beacon.mjs', 'beacon.js', 'beacon.cmd', 'beacon.bat', 'node', 'fake-beacon.mjs', 'beacon.exe.mjs'].every((n) => !BEACON_BIN_NAME.test(n)));
  check('l3b beacon: the status side effects are recorded as audited, not as UNVERIFIED', /loopback probes to 4317\/4318\/13133/.test(STATUS_SIDE_EFFECTS) && /--version/.test(STATUS_SIDE_EFFECTS) && !src.includes('UNVERIFIED: that `beacon version`'));
  check('l3b: the scenario header says LIVE STATUS: UNVERIFIED and carries the three phase commands', /LIVE STATUS: UNVERIFIED/.test(src) && ['phase=baseline', 'phase=probe', 'phase=verify'].every((p) => src.includes(`--param ${p}`)));
  check('l3b: l3-beacon is not a CI scenario', !CI_SCENARIOS.includes('l3-beacon') && JSON.stringify(CI_SCENARIOS) === JSON.stringify(['smoke', 'g1-claude-wake']));
  check('l3b: scenario defaults: the G5 launch, a 60-minute box, accept=driver (#196), readonly Beacon CLI', l3Scenario.name === 'l3-beacon' && JSON.stringify(l3Scenario.harnesses) === '["claude","codex"]' && l3Scenario.defaults.timeboxMs === L3_BOX_MS && L3_BOX_MS === 3600000 && l3Scenario.defaults.params.accept === 'driver' && l3Scenario.defaults.params.beaconCli === 'readonly' && l3Scenario.defaults.launch.join(' ') === 'claude --dangerously-load-development-channels server:g5spike');
  check('l3b: no harness-config write in the scenario (JS write API on a config path)', !/(?:writeFile|appendFile|copyFile|rename|rm|unlink)\w*\([^)]*(?:settings\.json|config\.toml|hooks\.json|\.claude\.json)/.test(code));

  // --- parsers ---
  check('l3b parse: beacon version', parseBeaconVersion('beacon version 1.3.29\n') === BEACON_VERSION && parseBeaconVersion('beacon version v1.3.29 (commit abc)') === '1.3.29' && parseBeaconVersion('beacon version 1.3.290') === '1.3.290' && parseBeaconVersion('1.3.29') === null && parseBeaconVersion('') === null);
  const st = parseBeaconStatus('Beacon Endpoint Agent 1.3.29\nConfig: x\nRuntime log: C:\\ProgramData\\Beacon\\Endpoint\\logs\\runtime.jsonl\nService: loaded=true running=false (Access is denied)\nBeacon Managed: not connected\n');
  check('l3b parse: endpoint status log path, Managed and Service lines', st.logPath === 'C:\\ProgramData\\Beacon\\Endpoint\\logs\\runtime.jsonl' && st.managed === 'not connected' && /running=false/.test(st.service) && st.agentVersion === '1.3.29' && parseBeaconStatus('').logPath === null);
  // #209: the real v1.3.29 Local-mode line (endpoint_connect.go@v1.3.29 L291), CRLF as on Windows.
  const real = parseBeaconStatus('Beacon Endpoint Agent 1.3.29\r\nLast event: present\r\nBeacon Managed: not connected (run `beacon endpoint connect`)\r\nInventory heartbeat: x\r\n');
  check('l3b parse #209: the real 1.3.29 Managed line is parsed whole and is Local mode', real.managed === 'not connected (run `beacon endpoint connect`)' && beaconManagedIsLocal(real.managed), JSON.stringify(real.managed));
  check('l3b parse #209: only the exact allowlisted Local-mode values (bare; L291 hint) are Local mode', BEACON_MANAGED_LOCAL_VALUES.length === 2 && ['not connected', 'not connected (run `beacon endpoint connect`)'].every(beaconManagedIsLocal));
  check('l3b parse #209: connect incomplete (status.go L64, L71) is NOT Local mode (a forwarder may run; connect was run)', ['not connected (connect incomplete: this device was approved but connect did not finish; run `beacon endpoint connect` again)', 'not connected (connect incomplete: this device was approved but connect did not finish; the partial forwarder was stopped; run `beacon endpoint connect` again)'].every((v) => !beaconManagedIsLocal(v)));
  check('l3b parse #209: connected, a missing line, enrollment-read errors (status.go L62), re-connect incomplete (L78), disconnected-with-credentials (L101) and unrecognised wording are NOT Local mode (fail safe; exact match only)', [null, undefined, '', 'connected to Acme as device d1; forwarder loaded=true running=true; credential valid', 'connected', 'not connected yet', 'not connected; forwarding', 'not connected (x) and forwarding', 'not connected (x) and (y)', 'not connected (x)', 'not connected (open C:\\ProgramData\\Beacon\\Endpoint\\enrollment.json: Access is denied.)', 'not connected (enrollment record C:\\ProgramData\\Beacon\\Endpoint\\enrollment.json is not valid JSON: unexpected end of JSON input)', 'not connected (re-connect incomplete: the server rotated device d1\'s key but connect did not finish, so the forwarder cannot upload; run `beacon endpoint connect` again)', 'not connected (disconnected; credentials for device d1 kept, run `beacon endpoint connect` to reuse them)', 'not connected (run `beacon endpoint connect`) ', ' not connected', 'Not Connected', 'disconnected'].every((v) => !beaconManagedIsLocal(v)) && parseBeaconStatus('Beacon Endpoint Agent 1.3.29\n').managed === null);
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
  const xm = markers.find((m) => m.id === 'codex-turn-start');
  const logText = [
    JSON.stringify({ event: { action: 'prompt.submitted' }, harness: { name: 'claude_code', collection_method: 'hook' }, session: { id: 's1' }, prompt: { text: `a ${cm.marker} ${cm.token}` } }),
    JSON.stringify({ event: { action: 'mcp.tool_invoked' }, harness: { name: 'claude_code', collection_method: 'hook' }, session: { id: 's1' }, tool: { arguments: { message: cm.marker } } }),
    JSON.stringify({ event: { action: 'prompt.submitted' }, harness: { name: 'codex', collection_method: 'otlp' }, session: { id: 't1' }, prompt: { text: xm.marker } }),
  ].join('\n');
  const hc = hitCounts(scanRuntimeLog(logText, markers).hits, markers);
  check('l3b counts: per path, lines / marker lines / token lines and counts per action, JSON path, collection method and harness; value-free', hc['claude-channel'].lines === 2 && hc['claude-channel'].tokenLines === 1 && hc['claude-channel'].byAction['mcp.tool_invoked'] === 1 && hc['claude-channel'].byAction['prompt.submitted'] === 1 && hc['claude-channel'].byPath['tool.arguments.message'] === 1 && hc['codex-turn-start'].byCollectionMethod.otlp === 1 && hc['codex-turn-start'].byHarness.codex === 1 && hc['codex-queue-add'].lines === 0 && findMarkerLeaks(JSON.stringify(hc), markers).length === 0, JSON.stringify(hc));
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
const FAST = ['--param', 'settleMs=300', '--param', 'pollMs=200', '--param', 'listPollMs=400', '--param', 'wireTimeoutMs=10000', '--param', 'turnTimeoutMs=30000', '--param', 'startupTimeoutMs=20000', '--param', 'handshakeTimeoutMs=20000', '--param', 'attachTimeoutMs=15000', '--param', 'readyTimeoutMs=15000', '--param', 'beaconSettleMs=2500', '--param', 'beaconTimeoutMs=20000'];
const SYNTHETIC = {
  'claude-settings': '{\n  "env": { "A": "1" },\n  "hooks": {}\n}\n',
  'claude-state': '{ "numStartups": 1 }\n',
  'codex-config': 'model = "m"\n\n[otel]\nexporter = "none"\n',
  'codex-hooks': '{}\n',
};
const gateServerHashes = () => Object.fromEntries(readdirSync(GATE_SERVERS).sort().map((n) => [n, sha256(readFileSync(join(GATE_SERVERS, n)))]));

// One machine: a fake $HOME with synthetic harness config at Beacon's fixed paths, the harness
// doubles and fake-beacon on PATH, and a fake Beacon runtime log.
// accept: the scenario's accept policy for every phase. `driver` (the default since #196): the
// fakes never accept their own dialogs, so the driver must. `human`: the fakes self-accept after
// 1 s (standing in for the operator) and the driver sends nothing. claudeDialogs: fake-claude's
// FAKE_CLAUDE_DIALOG (default: the dev-channels dialog alone); codexDialog: fake-codex's
// FAKE_CODEX_DIALOG (default: its trust dialog, #199).
function l3World(h, { claudeCli = parseClaudeVersions(readFileSync(join(REPO, 'docs', 'planning', 'PINS.md'), 'utf8')).lastTested, codexVersion, trace = false, harnessWritesLog = true, syncHistoryBytes = 0, accept = 'driver', claudeDialogs = null, codexDialog = null } = {}) {
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
    // The fake Codex shows its trust dialog (the text seen live on 0.159.2, #199): under
    // accept=driver the driver accepts it (enter), under accept=human the fake self-accepts.
    ...(accept === 'human' ? { FAKE_CLAUDE_SELF_ACCEPT_MS: '1000', FAKE_CODEX_SELF_ACCEPT_MS: '1000' } : {}),
    ...(codexDialog ? { FAKE_CODEX_DIALOG: codexDialog } : {}),
    ...(claudeDialogs ? { FAKE_CLAUDE_DIALOG: claudeDialogs } : {}),
    FAKE_CLAUDE_SESSION_FILE: '1',
    FAKE_CODEX_LONG_MS: '4500',
    ...(syncHistoryBytes ? { FAKE_BEACON_SYNC_HISTORY_BYTES: String(syncHistoryBytes) } : {}),
    ...(claudeCli ? { FAKE_CLAUDE_CLI_VERSION: claudeCli, FAKE_CLAUDE_VERSION: claudeCli } : {}),
    ...(harnessWritesLog ? { FAKE_BEACON_LOG: log } : {}),
  };
  let n = 0;
  const drive = (phase, args = [], extraEnv = {}, { nodeArgs = [], invariantOpts } = {}) => {
    const tag = `${phase}-${++n}`;
    const bp = { ...b, state: join(b.base, `herdr-state-${tag}`) };
    const out = join(b.base, `out-${tag}`);
    const res = spawnSync(process.execPath, [...nodeArgs, h.RUN, '--scenario', 'l3-beacon', '--herdr-bin', h.FAKE, '--out', out, '--param', `phase=${phase}`, '--param', `beaconBin=${beaconBin}`, '--param', `beaconLog=${log}`, '--param', `accept=${accept}`, ...FAST, ...args], {
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
    h.invariants(`l3 ${tag}`, bp, r, invariantOpts);
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
    // #196: Claude Code's three dialogs in the live order, each with its live preselection; the
    // driver (the default policy) accepts them. #199: likewise Codex's trust dialog, here with
    // its optional Note block, as in the live probe run.
    const w = l3World(h, { trace: true, syncHistoryBytes: 9 * 1024 * 1024 + 4096, claudeDialogs: 'workspace-trust,mcp-server-approval,dev-channels', codexDialog: 'trust-note' });
    try {
      const base = w.drive('baseline');
      const bm = base.manifest;
      const b3 = bm?.scenarioData?.l3;
      check('l3 baseline: PASS; the box declared; Beacon 1.3.29 read through the allowlist; no harness launched', base.status === 0 && bm.outcome === 'PASS' && !!b3.box.start && b3.box.budgetMs === L3_BOX_MS && b3.versions.beacon === '1.3.29' && agentStarts(base) === 0, `${base.status} ${bm?.outcomeReason}`);
      check('l3 baseline: the four fixed-$HOME files and the env-dir equivalents hashed; env names recorded, never values; trailing-comment check recorded', b3.config.hashes.filter((x) => x.present).length >= 6 && b3.config.envSet.CLAUDE_CONFIG_DIR === true && b3.config.envSet.CODEX_HOME === true && bm.findings.some((f) => /CLAUDE_CONFIG_DIR is set/.test(f)) && Array.isArray(b3.config.trailingComment) && !base.manifestText.includes('exporter = '), JSON.stringify(b3.config.hashes.map((x) => [x.label, x.present])));
      check('l3 baseline: status --system recorded (log path, the real 1.3.29 Managed line); no "hosted forwarding" finding (#209)', b3.beacon.status.managed === 'not connected (run `beacon endpoint connect`)' && !!b3.beacon.status.logPath && !bm.findings.some((f) => /Beacon Managed/.test(f)), JSON.stringify([b3.beacon.status.managed, bm.findings]));

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
        check('l3 probe B2: the channel probe reached the (fake) Beacon log; the fake token appears verbatim; trust, MCP and dev-channels dialogs accepted by the DRIVER (#196: down,enter / up,up,enter / enter)', bm2['claude-channel'].lines >= 1 && bm2['claude-channel'].tokenVerbatimLines >= 1 && l3.steps.B2.dialogs.map((d) => `${d.kind}:${d.acceptOrigin}:${d.acceptKeys.join(',')}`).join(' ') === 'workspace-trust:driver:down,enter mcp-server-approval:driver:up,up,enter dev-channels:driver:enter', JSON.stringify({ bm2, dialogs: l3.steps.B2.dialogs }));
        const cx = l3.dialogs.filter((d) => d.agent === 'codex');
        check('l3 probe #199: the Codex trust dialog (with its Note block) was read, then accepted by the DRIVER with `enter` alone, straight after a read', cx.length === 1 && cx[0].kind === 'workspace-trust' && cx[0].acceptOrigin === 'driver' && JSON.stringify(cx[0].acceptKeys?.map((k) => k.key)) === '["enter"]' && cx[0].inputBetweenReadAndAccept === 0 && pm.commands.find((x) => x.seq === cx[0].acceptSeq - 1)?.argv.includes('read'), JSON.stringify(l3.dialogs));
        check('l3 probe B3: the reply tool was called with the probe marker (in-process check); B3 counted separately from B2', l3.steps.B3.replyToolCalls === 1 && l3.steps.B3.replyArgsCarryMarker === true && l3.steps.B3.log.delta.byMarker['claude-channel'].actions.includes('mcp.tool_invoked') && !l3.steps.B2.log.cumulative.byMarker['claude-channel'].actions.includes('mcp.tool_invoked'), JSON.stringify(l3.steps.B3.log.delta.byMarker['claude-channel']));
        check('l3 probe B4: turn/start and thread/queue/add both completed and both reached the (fake) log', l3.steps.B4.turnStart.status === 'completed' && l3.steps.B4.queueAdd.status === 'completed' && l3.steps.B4.turnStart.recordedByteIdentical && l3.scan.byMarker['codex-turn-start'].lines >= 1 && l3.scan.byMarker['codex-queue-add'].lines >= 1);
        check('l3 probe: poll-path counts from sync --print for B2 and B4, streamed past 9 MB of older history printed first (the probe session last)', l3.beacon.sync.B2.status === 'recorded' && l3.beacon.sync.B2.streamed === true && l3.beacon.sync.B2.bytes > 9 * 1024 * 1024 && l3.beacon.sync.B2.lines > 15000 && l3.beacon.sync.B2.byMarker['claude-channel'].lines >= 1 && l3.beacon.sync.B2.byMarker['claude-channel'].collectionMethods.includes('poll') && l3.beacon.sync.B4.byMarker['codex-turn-start'].lines >= 1 && l3.beacon.sync.B4.bytes > 9 * 1024 * 1024 && l3.beacon.sync.B4.truncated === false, JSON.stringify({ ...l3.beacon.sync.B2, bySession: undefined, byMarker: undefined, counts: undefined }));
        check('l3 probe: whole-run scan with redacted excerpts in the manifest-safe placeholder form', l3.scan.hitCount >= 4 && l3.scan.excerpts.length >= 4 && l3.scan.excerpts.every((e) => /\{L3-(?:MARKER|FAKE-TOKEN) [a-z-]+\}/.test(e.text) && !/<L3-/.test(e.text)) && l3.scan.bySession.every((s) => s.sessionId !== 'before-the-probe'), JSON.stringify(l3.scan.excerpts.map((e) => e.text.slice(0, 60))));
        check('l3 probe: the session file described by entry type and flags only', l3.sessionFile.read === true && l3.sessionFile.dirsFound === 1 && l3.sessionFile.entries.some((e) => e.type === 'attachment' && e.isMeta === true && e.attachmentType === 'channel_message') && l3.sessionFile.entries.every((e) => !('content' in e)), JSON.stringify(l3.sessionFile));
        check('l3 probe: B5 and B6 NOT RUN; the daemon state is a finding (not running before)', l3.steps.B5.status === 'NOT RUN' && l3.steps.B6.status === 'NOT RUN' && l3.daemon.alreadyRunning === false && pm.findings.some((f) => /daemon state: not running before/.test(f)) && l3.daemon.leftRunning === true && pm.findings.some((f) => /codex app-server daemon stop. before the B7/.test(f)));
        check('l3 probe: the status side effects are recorded', /no writes/.test(l3.beacon.status.sideEffects) && /4317/.test(l3.beacon.status.sideEffects));
        check('l3 probe: no VERSION WARNING at the PINS.md last tested versions', !pm.findings.some((f) => /VERSION WARNING|pin drift/.test(f)) && l3.versions.pins.claude.differs === false && l3.versions.pins.codex.differs === false, JSON.stringify(pm.findings));
        check('l3 probe: captures written clean, none fixture-shaped', pm.captures.length === 4 && pm.captures.every((c) => c.written && /^l3-/.test(c.file) && !/-herdr\./.test(c.file)), JSON.stringify(pm.captures.map((c) => [c.file, c.written])));
        check('l3 probe (L3c contract): accept policy, per-action/per-path counts, poll-path counts, probe session ids recorded', l3.acceptPolicy === 'driver' && l3.scan.counts['claude-channel'].byAction['mcp.tool_invoked'] === 1 && l3.scan.counts['claude-channel'].byAction['prompt.submitted'] >= 1 && l3.scan.counts['claude-channel'].tokenLines >= 1 && Object.keys(l3.scan.counts['codex-queue-add'].byPath).includes('prompt.text') && l3.beacon.sync.B4.counts['codex-queue-add'].markerLines >= 1 && l3.beacon.sync.B2.harness === 'claude' && l3.steps.B3.log.delta.counts['claude-channel'].byAction['mcp.tool_invoked'] === 1 && l3.probeSessions.claude.length === 1 && l3.probeSessions.codex[0] === l3.thread.id && l3.scan.bySession.filter((s) => s.markerIds.length).every((s) => [...l3.probeSessions.claude, ...l3.probeSessions.codex].includes(s.sessionId)), JSON.stringify({ counts: l3.scan.counts, probeSessions: l3.probeSessions }));
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

      // L3c on the scenario's own records (this lifecycle run's three manifests).
      const reportRuns = { baseline: { manifest: base.manifest }, probe: { manifest: pm }, verify: { manifest: ver.manifest }, priors: [], notes: {} };
      const dr = tryDraft(reportRuns, { markers: markers.every((m) => m.marker && m.token) ? markers : null });
      const d = dr.text ?? '';
      check('l3 report on scenario records: a draft is produced and passes the leak guard with the real probe values', !!dr.text && dr.err === null && findMarkerLeaks(d, markers.filter((m) => m.marker)).length === 0, dr.err?.message);
      check('l3 report on scenario records: no spurious "versions differ between phases" finding (null daemon/wire in the baseline and null Beacon in verify are not compared)', !/differ between phases/.test(d));
      const findingLines = d.split('### Findings')[1]?.split('###')[0].split('\n').filter((l) => l.startsWith('- ')).map((l) => l.replace(/^- (baseline|probe|verify) (?:run|record): /, '$1: ')) ?? [];
      check('l3 report on scenario records: a finding written to both the manifest and the record is listed once', findingLines.length > 0 && new Set(findingLines).size === findingLines.length, findingLines.join(' | '));
      check('l3 report on scenario records: poll-path counts rendered for B2, B3 and B4 (not NOT RUN)', /sync --print`\) at B2: \d+ event line\(s\) read, scanned as streamed/.test(d) && /sync --print`\) at B3: \d+ event line/.test(d) && /sync --print`\) at B4: \d+ event line/.test(d) && !/sync --print`\) at B\d: NOT RUN/.test(d));
      check('l3 report on scenario records: per-action counts, the B2/B3 split, the reply-tool record, session-file shapes, excerpts, envSet all rendered; nothing listed as not recorded', /hit lines per action: [^\n]*`mcp\.tool_invoked` 1/.test(d) && /B2 log snapshot \(from the probe start, taken before B3\)/.test(d) && /B3 log snapshot \(lines added since the B2 snapshot\): claude-channel 1 line\(s\), per action `mcp\.tool_invoked` 1/.test(d) && /Reply tool on the channel-server wire: 1 call\(s\); its arguments carried the marker: yes/.test(d) && /Claude session file \(entry types and flags only/.test(d) && /type `attachment`, isMeta `true`, attachment type `channel_message`/.test(d) && /### Redacted excerpts/.test(d) && /\{L3-MARKER claude-channel\}/.test(d) && /Environment \(whether set only\): CLAUDE_CONFIG_DIR set, CODEX_HOME set/.test(d) && /nothing: the probe record carries every field/.test(d) && !/outside the probe session/.test(d), d.split('\n').filter((l) => /Not recorded|nothing:|outside|per action|B3 log|Reply tool/.test(l)).join(' || '));
      const cli = spawnSync(process.execPath, [REPORT, '--baseline', base.out, '--probe', probe.out, '--verify', ver.out], { encoding: 'utf8', timeout: 30000 });
      check('l3 report CLI on scenario run dirs: exit 0, draft printed, no probe value in its output', cli.status === 0 && /## 13\. Live results \(L3\)/.test(cli.stdout) && findMarkerLeaks(cli.stdout + cli.stderr, markers.filter((m) => m.marker)).length === 0, cli.stderr);

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
      check('l3 trace: no credential file and no OS credential-store tool touched by the driver', driver.every((t) => !t.path || !CREDENTIAL_NAMES.includes(basename(t.path))) && spawned.every((f) => [process.execPath, 'git', 'codex', 'claude', 'ps', w.beaconBin, join(w.b.base, 'bin', 'claude'), join(w.b.base, 'bin', 'codex')].includes(f)), JSON.stringify(spawned));
      check('l3 trace: no harness config file copied or written anywhere by the driver', driver.every((t) => !(t.kind === 'write' || /^(?:copyFile|cp)/.test(t.op ?? '')) || !['settings.json', 'config.toml', 'hooks.json', '.claude.json'].includes(basename(t.path ?? ''))) && driver.filter((t) => /^(?:copyFile|cp)/.test(t.op ?? '')).every((t) => !underHome(t)));
      check('l3 trace: beacon ran only with allowlisted argv', driver.filter((t) => t.kind === 'spawn' && t.file === w.beaconBin).length >= 5 && driver.filter((t) => t.kind === 'spawn' && t.file === w.beaconBin).every((t) => Object.values(BEACON_ALLOWLIST).some((x) => x.join(' ') === (t.args ?? []).join(' '))));
    } catch (err) {
      check('l3 happy path: assertions ran', false, err.stack);
    } finally {
      w.cleanup();
    }
  }

  // --- #202: scratch removal fails persistently (the shared Codex daemon's handle on the
  // thread's project directory, seen live on Windows; here test/fake-rm-eperm.mjs makes every
  // removal attempt throw EPERM). The manifest is still written with the full L3 record, the
  // outcome is the run's, and the leftover is recorded, redacted, with the daemon named. ---
  if (h.FAKE_RM) {
    const w = l3World(h);
    let left = null;
    try {
      const base = w.drive('baseline');
      const p = w.drive('probe', ['--param', `baselineRun=${base.out}`], { FAKE_RM_EPERM: 'always' }, { nodeArgs: ['--import', pathToFileURL(h.FAKE_RM).href], invariantOpts: { scratchLeft: true } });
      left = /^scratch left behind \(delete by hand\): (.+)$/m.exec(p.stdout)?.[1]?.trim() ?? null;
      const m = p.manifest;
      const l3 = m?.scenarioData?.l3;
      check('l3 scratch EPERM #202: the probe manifest is written and its outcome is the run\'s (PASS, exit 0)', p.status === 0 && m?.outcome === 'PASS', `${p.status} ${m?.outcome} ${m?.outcomeReason} ${p.stderr}`);
      check('l3 scratch EPERM #202: the full L3 record survives (steps B1-B6, B4 both paths, thread, daemon, scan, markers, captures)', !!l3 && ['B1', 'B2', 'B3', 'B4', 'B5', 'B6'].every((k) => !!l3.steps?.[k]) && l3.steps.B4.turnStart?.status === 'completed' && l3.steps.B4.queueAdd?.status === 'completed' && !!l3.thread?.id && l3.daemon?.leftRunning === true && l3.markers.length === 3 && typeof l3.scan?.hitCount === 'number' && m.captures.length === 4 && m.captures.every((c) => c.written), JSON.stringify({ steps: Object.keys(l3?.steps ?? {}), captures: m?.captures?.map((c) => [c.file, c.written]) }));
      check('l3 scratch EPERM #202: teardown.clean=false, teardown.leftover redacted, scratch not removed after the bounded attempts', m?.teardown?.clean === false && m.teardown.leftover === '<SCRATCH>' && m.scratch.removed === false && m.scratch.removal.errors.every((e) => e.code === 'EPERM') && !p.manifestText.includes('oac-herdr-scratch-'), JSON.stringify({ teardown: m?.teardown, scratch: m?.scratch }));
      check('l3 scratch EPERM #202: the finding names the leftover and the daemon, released on `codex app-server daemon stop`', m?.scratch?.holders?.some((x) => /Codex app-server daemon/.test(x)) && m.findings.some((f) => /left behind at <SCRATCH>/.test(f) && /codex app-server daemon stop/.test(f)), JSON.stringify(m?.findings));
      const pidFile = join(w.b.env.CODEX_HOME, 'app-server-control', 'fake-daemon.pid');
      let daemonAlive = false;
      try {
        process.kill(Number(readFileSync(pidFile, 'utf8')), 0);
        daemonAlive = true;
      } catch {
        /* no pid file, or gone */
      }
      check('l3 scratch EPERM #202: the driver left the (fake) daemon running; it never stops it', daemonAlive);
    } catch (err) {
      check('l3 scratch EPERM #202: assertions ran', false, err.stack);
    } finally {
      if (left && left.startsWith(tmpdir()) && basename(left).startsWith('oac-herdr-scratch-')) rmSync(left, { recursive: true, force: true });
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

  // --- harness versions off PINS.md (#216: VERSION WARNING) and a pre-existing daemon: findings, not stops ---
  {
    const w = l3World(h, { claudeCli: '2.1.999', codexVersion: '0.999.0' });
    try {
      const base = w.drive('baseline');
      spawnSync('codex', ['app-server', 'daemon', 'start'], { env: { ...process.env, ...w.b.env, ...w.env }, encoding: 'utf8', timeout: 20000 });
      // A slow full-history sync: each sync --print stalls 6 s before any output; the sync
      // timeout is 1.5 s.
      const p = w.drive('probe', ['--param', `baselineRun=${base.out}`, '--param', 'readSessionFile=false', '--param', 'beaconSyncTimeoutMs=1500'], { FAKE_BEACON_SYNC_SLEEP_MS: '6000' });
      const m = p.manifest;
      const l3 = m?.scenarioData?.l3;
      check('l3 version drift (#216): PASS, with a VERSION WARNING per harness (CLI, daemon fields, wire); PINS.md is not edited', p.status === 0 && m.outcome === 'PASS' && m.findings.some((f) => /^VERSION WARNING \(L3\): `claude --version` reports v2\.1\.999/.test(f)) && m.findings.some((f) => /^VERSION WARNING \(L3\): `codex --version` reports 0\.999\.0/.test(f)) && m.findings.some((f) => /^VERSION WARNING \(L3\): `codex app-server daemon version` cliVersion reports 0\.999\.0/.test(f)) && m.findings.some((f) => /^VERSION WARNING \(L3\): the Codex wire initialize userAgent reports 0\.999\.0/.test(f)) && l3.versions.pins.claude.differs && l3.versions.pins.codex.differs && /^\d+\.\d+\.\d+$/.test(l3.versions.pins.claude.minimum ?? ''), `${p.status} ${m?.outcomeReason} ${JSON.stringify(m?.findings)}`);
      check('l3 pre-existing daemon: recorded as a finding, not a stop', l3?.daemon?.alreadyRunning === true && m.findings.some((f) => /ALREADY RUNNING/.test(f)));
      check('l3 readSessionFile=false: the session file is not read', l3?.sessionFile?.read === false);
      const dt = tryDraft({ baseline: { manifest: base.manifest }, probe: { manifest: m }, verify: null, priors: [], notes: {} }).text ?? '';
      check('l3 report on a timed-out sync: each poll path rendered NOT RUN with its reason; B4 still evaluated', /sync --print`\) at B2: NOT RUN \(timed out/.test(dt) && /sync --print`\) at B4: NOT RUN \(timed out/.test(dt) && /- \*\*B4:\*\* (?:PASS|FINDING)/.test(dt), dt.split('\n').filter((l) => /Poll path|\*\*B4/.test(l)).join(' || '));
      check('l3 sync timeout: each timed-out sync --print is NOT RUN for its poll path only; the probe still PASSes through B3 and B4', ['B2', 'B3', 'B4'].every((k) => l3?.beacon?.sync?.[k]?.status === 'NOT RUN' && /timed out/.test(l3.beacon.sync[k].reason)) && l3.steps.B4.queueAdd.status === 'completed' && l3.steps.B3.replyToolCalls === 1 && m.findings.filter((f) => /poll path is recorded NOT RUN/.test(f)).length === 3, JSON.stringify(l3?.beacon?.sync));
    } catch (err) {
      check('l3 pin drift: assertions ran', false, err.stack);
    } finally {
      w.cleanup();
    }
  }

  // --- #209: a connected Managed line is still a finding (end to end through the fake Beacon) ---
  {
    const w = l3World(h);
    try {
      const base = w.drive('baseline', [], { FAKE_BEACON_MANAGED: 'connected to Acme as device d1; forwarder loaded=true running=true; credential valid' });
      const bm = base.manifest;
      check('l3 baseline #209: a "connected" Managed line is a "hosted forwarding may be on" finding; the baseline still PASSes', base.status === 0 && bm?.outcome === 'PASS' && /^connected to Acme/.test(bm.scenarioData.l3.beacon.status.managed) && bm.findings.some((f) => /Beacon Managed: connected to Acme/.test(f) && /hosted forwarding may be on/.test(f)), `${base.status} ${JSON.stringify(bm?.findings)}`);
    } catch (err) {
      check('l3 baseline #209: assertions ran', false, err.stack);
    } finally {
      w.cleanup();
    }
  }

  // --- zero hits is a result, not a failure; beaconCli=off runs no Beacon command ---
  {
    const w = l3World(h, { harnessWritesLog: false, accept: 'human' });
    try {
      const base = w.drive('baseline');
      const p = w.drive('probe', ['--param', `baselineRun=${base.out}`]);
      const l3 = p.manifest?.scenarioData?.l3;
      check('l3 accept=human (still available): the driver sent no dialog key; the dialog is recorded as accepted outside the driver', p.manifest?.scenarioData?.l3?.acceptPolicy === 'human' && !(p.manifest?.commands ?? []).some((c) => c.role === 'dialog-accept') && (l3?.steps?.B2?.dialogs ?? []).length >= 1 && l3.steps.B2.dialogs.every((d) => d.acceptOrigin === 'human'), JSON.stringify(l3?.steps?.B2?.dialogs));
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

  // --- #204: Codex's startup hook review blocks the session start; the thread marker waits ---
  {
    const w = l3World(h);
    try {
      const base = w.drive('baseline');
      // The live #204 shape: the startup-draft composer (no session yet), then the hook review.
      const p = w.drive('probe', ['--param', `baselineRun=${base.out}`, '--param', 'readSessionFile=false'], { FAKE_CODEX_STARTUP_MS: '1500', FAKE_CODEX_HOOKS_REVIEW: '1' });
      const m = p.manifest;
      const l3 = m?.scenarioData?.l3;
      const d = (l3?.dialogs ?? []).find((x) => x.kind === 'hooks-review');
      check('l3 #204 hook review: NOT RUN (exit 3) naming Codex\'s startup hook review; the thread marker never typed; no key sent to the review', p.status === 3 && /codex dialog \d+ \(hooks-review\): Codex's startup hook review is on screen/.test(m.outcomeReason) && d?.acceptOrigin === 'none (driver refused)' && !p.prompts.some((x) => x.target === 'l3codex') && !m.commands.some((x) => x.seq > d.readSeq && ['operator-input', 'dialog-accept'].includes(x.role)) && !l3.thread, `${p.status} ${m?.outcomeReason} ${JSON.stringify(d)}`);
      const q = w.drive('probe', ['--param', `baselineRun=${base.out}`, '--param', 'readSessionFile=false'], { FAKE_CODEX_STARTUP_MS: '2500' });
      const r3 = q.manifest?.scenarioData?.l3;
      const tmSeq = q.manifest?.commands.find((x) => x.role === 'operator-input' && x.argv.includes('prompt') && x.argv.includes('l3codex'))?.seq;
      check('l3 #204 startup draft: PASS; the thread marker typed once, after the ready read (new loaded thread + idle composer)', q.status === 0 && r3?.codexReady?.newThreads >= 1 && r3.codexReady.observations.some((o) => /startup draft/.test(o.why ?? '')) && tmSeq > r3.codexReady.readSeq && q.prompts.filter((x) => x.target === 'l3codex').length === 1 && !!r3.thread?.id, `${q.status} ${q.manifest?.outcomeReason} ${JSON.stringify(r3?.codexReady)}`);
    } catch (err) {
      check('l3 #204: assertions ran', false, err.stack);
    } finally {
      w.cleanup();
    }
  }

  // --- #205 review: accept=human and the hook review: answered by the operator, recorded ---
  {
    const w = l3World(h, { accept: 'human' });
    try {
      const base = w.drive('baseline');
      // The fake self-answers the review with esc ("Continue without trusting") after 1 s,
      // standing in for the operator.
      const p = w.drive('probe', ['--param', `baselineRun=${base.out}`, '--param', 'readSessionFile=false'], { FAKE_CODEX_HOOKS_REVIEW: '1' });
      const m = p.manifest;
      const l3 = m?.scenarioData?.l3;
      const d = (l3?.dialogs ?? []).find((x) => x.kind === 'hooks-review');
      check('l3 #205 human hook review: PASS; the review answered outside the driver (no key sent); a finding says Beacon\'s SessionStart hook may not have run', p.status === 0 && d?.acceptOrigin === 'human' && !m.commands.some((x) => x.role === 'dialog-accept') && m.findings.some((f) => /hook review was on screen and answered by the operator/.test(f) && /Continue without trusting/.test(f) && /SessionStart hook/.test(f)) && !!l3.thread?.id, `${p.status} ${m?.outcomeReason} ${JSON.stringify(m?.findings)}`);
    } catch (err) {
      check('l3 #205 human hook review: assertions ran', false, err.stack);
    } finally {
      w.cleanup();
    }
  }

  const gsAfter = gateServerHashes();
  check('l3: the committed tools/herdr/gate-servers/ files are byte-unchanged after every L3 run', JSON.stringify(gsBefore) === JSON.stringify(gsAfter));
}
