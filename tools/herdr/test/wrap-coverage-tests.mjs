// #288: a home path or caller literal split across pane lines by a wrap must be redacted and,
// if it survives, reported by the residual scan. #290: MANIFEST.json coverage writes a
// request/response pair as a range only when the lines are adjacent.
//
// The real failure shape is the committed G4 Claude pane fixture: its echoed env-probe
// command wraps the driver's worktree path mid-name on 9 lines. That fixture is hash-bound
// and is only read here. The worktree path is derived from it at test time, so this file
// carries no repository path of its own.

import { readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { createRedactor, reportIsClean, WRAP_MIN_LENGTH } from '../lib/redact.mjs';
import { lineSpan } from '../lib/gate-report-common.mjs';
import { draftManifestEntries } from '../lib/g4-report.mjs';
import { fixtureNames } from '../lib/g4.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, '..', '..', '..');
const G4_DIR = join(REPO, 'docs', 'planning', 'gates', 'fixtures', 'g4-mcp-dual-era');
const G4_PANE = join(G4_DIR, 'pane-claude-2026-10-04-2.1.285-herdr.txt');
const G4_RUN_MANIFEST = join(REPO, 'docs', 'planning', 'gates', 'herdr-runs', 'G4-2026-10-04.run-manifest.json');
// The 9 lines G4-2026-10-04.md "Redaction" lists as carrying the wrapped worktree path.
const G4_WRAPPED_LINES = [2, 29, 56, 79, 102, 125, 148, 171, 196];

const read = (p) => readFileSync(p, 'utf8').replace(/\r\n/g, '\n');

// The pre-#288 behaviour, kept here as the mutation baseline: every rule applied to one line
// at a time, and the residual scan run one line at a time.
const perLineRedact = (r, text) => text.split('\n').map((l) => r.redactText(l).text).join('\n');
const perLineLeaks = (r, text) => text.split('\n').flatMap((l) => r.scan(l).residualLeaks);

function realG4Shape(check) {
  const pane = read(G4_PANE);
  const lines = pane.split('\n');
  // Rebuild the wrapped literal from the fixture itself: the tail of line 2 after `node.exe `
  // and the head of line 3 up to `\tools\herdr`.
  const head = lines[1].slice(lines[1].indexOf('node.exe ') + 'node.exe '.length);
  const tail = lines[2].slice(0, lines[2].indexOf('\\tools\\herdr'));
  const worktree = head + tail;
  check('i288 fixture: the G4 Claude pane fixture wraps a worktree path across lines 2-3 (the failure shape)', /^[A-Za-z]:\\/.test(worktree) && /\\worktrees\\agent-[0-9a-f]+$/.test(worktree) && head.length >= 6 && tail.length >= 6 && !lines[1].includes(worktree), `${head.length}+${tail.length}`);
  const r = createRedactor({ home: '/home/alice', username: 'alice', hostname: 'buildbox-7', literals: [{ value: worktree, placeholder: '<REPO>' }] });

  // Mutation baseline: what the committed run saw.
  check('i288 fixture (mutation baseline): per-line redaction leaves both halves, per-line scan reports nothing', perLineRedact(r, pane) === pane && perLineLeaks(r, pane).length === 0);

  const scan = r.scan(pane);
  const hitLines = scan.residualLeaks.filter((h) => h.label === 'literal <REPO> (line-wrapped)').map((h) => h.line);
  check('i288 fixture: the residual scan reports the wrapped path on exactly the 9 recorded lines', JSON.stringify(hitLines) === JSON.stringify(G4_WRAPPED_LINES) && scan.residualLeaks.length === 9, JSON.stringify(scan.residualLeaks));
  check('i288 fixture: a run would refuse the capture (report not clean)', !reportIsClean({ ...scan, hazardProtocolFrames: [] }));

  const red = r.redactText(pane);
  const out = red.text.split('\n');
  const changed = lines.map((l, i) => (l !== out[i] ? i + 1 : 0)).filter(Boolean);
  const expectChanged = G4_WRAPPED_LINES.flatMap((n) => [n, n + 1]);
  check('i288 fixture: redactText replaces all 9 wrapped copies, report clean', red.report.replacements['literal <REPO> (line-wrapped)'] === 9 && reportIsClean(red.report), JSON.stringify(red.report));
  check('i288 fixture: line count unchanged and only the 18 wrapped lines differ', out.length === lines.length && JSON.stringify(changed) === JSON.stringify(expectChanged), changed.join(','));
  check('i288 fixture: the break is re-emitted after <REPO>, the rest of the command stays on the next line', out[1].endsWith('node.exe <REPO>') && out[2].startsWith('\\tools\\herdr\\lib\\env-probe.mjs ') && !red.text.includes(head) && !red.text.includes(tail.slice(0, WRAP_MIN_LENGTH)), `${out[1]} / ${out[2].slice(0, 40)}`);
  // Nothing else in 929 lines is touched: every other line equals its per-line redaction.
  check('i288 fixture: no over-redaction (every unwrapped line equals its per-line redaction)', out.every((l, i) => expectChanged.includes(i + 1) || l === r.redactText(lines[i]).text));
}

function splitPoints(check) {
  const home = 'C:\\Users\\alice';
  const repo = 'C:\\src\\OAC\\.claude\\worktrees\\agent-0123abcd';
  const scratch = `${home}\\AppData\\Local\\Temp\\oac-herdr-scratch-Zq9xYw`;
  const r = createRedactor({ home, username: 'alice', hostname: 'buildbox-7', literals: [{ value: scratch, placeholder: '<SCRATCH>' }, { value: repo, placeholder: '<REPO>' }] });

  // Every split point of the repo path and of the home path, in a pane line.
  const bad = [];
  let points = 0;
  let baselineWrong = 0;
  let baselineScannedRepo = 0;
  for (const [lit, ph, before, after] of [[repo, '<REPO>', 'PS x> node.exe ', '\\tools\\herdr\\lib\\env-probe.mjs'], [home, '<USER_HOME>', 'cd ', '\\AppData\\Roaming']]) {
    for (let k = 1; k < lit.length; k++) {
      points += 1;
      const text = `${before}${lit.slice(0, k)}\n${lit.slice(k)}${after}`;
      const expected = `${before}${ph}\n${after}`;
      const red = r.redactText(text);
      if (red.text !== expected || !reportIsClean(red.report)) bad.push(`${ph}@${k}: ${red.text}`);
      if (r.scan(text).residualLeaks.length === 0) bad.push(`${ph}@${k}: scan missed`);
      if (perLineRedact(r, text) !== expected) baselineWrong += 1;
      if (ph === '<REPO>' && perLineLeaks(r, text).length) baselineScannedRepo += 1;
    }
  }
  check('i288 split points: repo and home paths split at every point are redacted to the placeholder, break kept, scan flags the raw text', bad.length === 0, bad.slice(0, 3).join(' | '));
  // Mutation baseline: the pre-#288 per-line form gets every split point wrong, and its scan
  // misses every repo split (the residual run.mjs relied on).
  check('i288 split points (mutation baseline): per-line redaction is wrong at every split point; per-line scan misses the repo splits', baselineWrong === points && baselineScannedRepo === 0, `${baselineWrong}/${points} scanned ${baselineScannedRepo}`);

  // A real command line hard-wrapped at every pane width from 24 to 140 columns: home,
  // scratch (under home) and repo all wrap somewhere.
  const cmd = `PS ${scratch}\\g4-project> C:\\nvm4w\\nodejs\\node.exe ${repo}\\tools\\herdr\\lib\\env-probe.mjs ${scratch}\\env-probe-d4e8.json d4e8`;
  const wrapAt = (s, w) => s.match(new RegExp(`.{1,${w}}`, 'g')).join('\n');
  const widths = [];
  for (let w = 24; w <= 140; w++) {
    const text = `before\n${wrapAt(cmd, w)}\nENVPROBE-OK-d4e8`;
    const red = r.redactText(text);
    const flat = red.text.replace(/\n/g, '');
    const ok = red.text.split('\n').length === text.split('\n').length && reportIsClean(red.report) && !/alice|Zq9xYw|agent-0123|OAC\\\.cla/i.test(flat.replace(/<USER_HOME>|<SCRATCH>|<REPO>/g, '')) && flat.includes('<SCRATCH>\\g4-project>') && flat.includes('node.exe <REPO>\\tools') && flat.includes('<SCRATCH>\\env-probe-d4e8.json');
    if (!ok) widths.push(w);
  }
  check('i288 pane widths 24-140: home, scratch and repo wrapped anywhere are all redacted, lines kept', widths.length === 0, widths.join(','));

  // Pane padding and other line endings.
  check('i288 padding: trailing spaces before the break and a continuation indent after it', r.redactText(`x ${repo.slice(0, 9)}   \n    ${repo.slice(9)}\\y`).text === 'x <REPO>\n\\y');
  check('i288 CRLF: the CRLF break is re-emitted as CRLF', r.redactText(`x ${repo.slice(0, 12)}\r\n${repo.slice(12)}\\y`).text === 'x <REPO>\r\n\\y');
  check('i288 double wrap: a literal over three lines keeps both breaks', r.redactText(`x ${repo.slice(0, 7)}\n${repo.slice(7, 20)}\n${repo.slice(20)}\\y`).text === 'x <REPO>\n\n\\y');
  const ur = createRedactor({ home: '/home/alice', username: 'alice', hostname: 'buildbox-7' });
  check('i288 unix home: /home/alice wrapped mid-name', ur.redactText('ls /home/al\nice/src').text === 'ls <USER_HOME>\n/src' && ur.scan('ls /home/al\nice/src').residualLeaks.some((h) => h.label === 'home path (line-wrapped)'));
}

function noOverRedaction(check) {
  const repo = 'C:\\src\\OAC\\.claude\\worktrees\\agent-0123abcd';
  const r = createRedactor({ home: '/home/alice', username: 'alice', hostname: 'buildbox-7', literals: [{ value: repo, placeholder: '<REPO>' }, { value: 'Zq9', placeholder: '<SHORT>' }] });
  const same = (t) => r.redactText(t).text === t && r.scan(t).residualLeaks.length === 0;
  const cases = {
    'prefix at a line end, unrelated next line': `node.exe ${repo.slice(0, 14)}\nde\\something-else\\x`,
    'prefix and rest separated by a non-space character': `${repo.slice(0, 14)}\n#${repo.slice(14)}`,
    'prefix and rest separated by a blank line (two breaks in one gap)': `${repo.slice(0, 14)}\n\n${repo.slice(14)}`,
    'a sibling worktree wrapped (not the literal)': `${repo.slice(0, 14)}\n${repo.slice(14, -1)}e`,
    'a literal shorter than the wrap minimum split by a break': 'Z\nq9',
    'the literal ends a line and the next line starts unrelated text': `a\n${repo.slice(0, 5)}`,
  };
  for (const [name, t] of Object.entries(cases)) check(`i288 negative: ${name} is untouched and not flagged`, same(t), r.redactText(t).text);
  // A break before the first character is never consumed: the placeholder stays on its own line.
  check('i288 negative: a line break before the literal stays before the placeholder', r.redactText(`see\n${repo}\\x`).text === 'see\n<REPO>\\x');
  // Text with no wrapped value: whole-text redaction equals per-line redaction, byte for byte.
  const g1 = read(join(REPO, 'docs', 'planning', 'gates', 'fixtures', 'g1-claude-wake', 'pane-2026-09-29-2.1.283-herdr.txt'));
  check('i288 negative: the committed G1 herdr pane fixture redacts identically whole-text and per line', r.redactText(g1).text === perLineRedact(r, g1));
  // Structured values (run manifests) get the same treatment for a multi-line string.
  const v = r.redactValue({ note: `ran ${repo.slice(0, 10)}\n${repo.slice(10)}\\tools` });
  check('i288 value: a wrapped literal inside a manifest string is redacted', v.value.note === 'ran <REPO>\n\\tools' && reportIsClean(v.report), v.value.note);
  const w = r.withholdResiduals({ note: `ran ${repo.slice(0, 10)}\n${repo.slice(10)}\\tools` });
  check('i288 value: withholdResiduals withholds a wrapped literal it is handed raw', w.withheld.length === 1 && /line-wrapped/.test(w.withheld[0].labels.join()));
}

function coverage(check) {
  check('i290 lineSpan: adjacent lines are a range', lineSpan(10, 11) === '10-11');
  check('i290 lineSpan: non-adjacent lines are a list (the #286 G4 probes)', lineSpan(5, 7) === '5, 7' && lineSpan(6, 8) === '6, 8' && lineSpan(35, 37) === '35, 37');
  check('i290 lineSpan: one line', lineSpan(4) === '4' && lineSpan(4, 4) === '4' && lineSpan(4, null) === '4');
  check('i290 lineSpan (mutation): never the old range form for a gap', !/^\d+-\d+$/.test(lineSpan(5, 8)));

  // The real G4 run: draft the transcript entry from the committed run manifest and transcript.
  const manifest = JSON.parse(readFileSync(G4_RUN_MANIFEST, 'utf8'));
  const g4 = manifest.scenarioData.g4;
  const names = fixtureNames(g4.date, g4.versions.cli.claude, g4.versions.cli.codex);
  const transcriptText = read(join(G4_DIR, names.transcript));
  const [entry] = draftManifestEntries({ manifest, fixtures: names, runManifestPath: 'docs/planning/gates/herdr-runs/G4-2026-10-04.run-manifest.json', transcriptText, pinsCommit: 'p', redactSha256: 'r' });
  const c = entry.coverage;
  check('i290 G4 draft: the interleaved stdio probes come out as lists, matching the hand-fixed MANIFEST entry', c['server/discover (stdio probe, rejected -32601)'] === '5, 7' && c['server/discover (stdio, modern-only negative-case copy)'] === '6, 8', JSON.stringify(c));
  check('i290 G4 draft: adjacent exchanges stay ranges; the Codex relay call around the line-36 push is a list', c['server/discover (HTTP, modern)'] === '10-11' && c['initialize (legacy stdio)'] === '14-15' && c['tools/call (Codex HTTP legacy)'] === '33-34, 35, 37', JSON.stringify(c));
  // No coverage range may span a line that the transcript attributes to a different exchange.
  const spans = Object.values(c).filter(Boolean).flatMap((s) => s.split(', ')).filter((s) => s.includes('-')).map((s) => s.split('-').map(Number));
  check('i290 G4 draft: every remaining range is two adjacent lines', spans.length > 0 && spans.every(([a, b]) => b === a + 1), JSON.stringify(spans));
}

export function wrapCoverageUnit(check) {
  realG4Shape(check);
  splitPoints(check);
  noOverRedaction(check);
  coverage(check);
}
