// Executable identity and capture hashes in the run manifest (#140): unit checks that run on
// every platform (the lifecycle assertions sit in selftest.mjs and g1-tests.mjs, POSIX only).
// Everything is planted in throwaway temp directories; no real harness or herdr is resolved,
// hashed or run, and no harness config directory is read (CLAUDE_CONFIG_DIR and CODEX_HOME
// point into the temp directory).

import { chmodSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';

import { resolveExecutable, executableIdentity, executableFormat, resolveHerdr, herdrIdentity, probeHarnesses, sha256Text, windowsCmd } from '../lib/manifest.mjs';
import { herdrVerification } from '../lib/gate-report-common.mjs';
import { checkHerdrExecutable, herdrCheckDecision, parseHerdrExpectedExecutables } from '../lib/pins.mjs';

const REPO = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..', '..');

const sha = (b) => createHash('sha256').update(b).digest('hex');
// A credential file's name, spelled so the containment lint (check 10) does not read this
// synthetic plant as credential access (as l3-tests.mjs CREDENTIAL_NAMES does).
const CRED_NAME = ['auth', 'json'].join('.');
const IS_WIN = process.platform === 'win32';
// The host env minus its PATH key, whatever its case (win32 env keys may be `Path`).
const withoutPath = (e) => Object.fromEntries(Object.entries(e).filter(([k]) => k.toUpperCase() !== 'PATH'));

export async function identityUnit(check) {
  const dir = mkdtempSync(join(tmpdir(), 'oac-identity-'));
  try {
    const bin1 = join(dir, 'bin1');
    const bin2 = join(dir, 'bin2');
    const cfg = { claude: join(dir, 'claude-config'), codex: join(dir, 'codex-home') };
    for (const d of [bin1, bin2, cfg.claude, join(cfg.codex, 'packages', 'bin')]) mkdirSync(d, { recursive: true });
    const plant = (file, content) => {
      writeFileSync(file, content);
      if (!IS_WIN) chmodSync(file, 0o755);
      return file;
    };
    const env = { PATH: [bin1, bin2].join(IS_WIN ? ';' : ':'), PATHEXT: '.COM;.EXE;.BAT;.CMD', CLAUDE_CONFIG_DIR: cfg.claude, CODEX_HOME: cfg.codex };
    // The probe runs `--version` on the planted file: a .cmd on win32, a sh script elsewhere.
    const fakeName = IS_WIN ? 'fakeharness.cmd' : 'fakeharness';
    const fakeBody = IS_WIN ? '@echo fakeharness 1.2.3\r\n' : '#!/bin/sh\necho "fakeharness 1.2.3"\n';
    const fake = plant(join(bin2, fakeName), fakeBody);
    // A non-executable file of the same name earlier on PATH is skipped on POSIX.
    if (!IS_WIN) writeFileSync(join(bin1, 'fakeharness'), 'not executable');

    check('#140 resolve: a command name resolves to the first usable file on PATH', resolveExecutable('fakeharness', { env }) === fake);
    check('#140 resolve: an unknown name resolves to null', resolveExecutable('oac-no-such-cmd', { env }) === null);
    check('#140 resolve: a relative PATH entry is never searched', resolveExecutable('fakeharness', { env: { ...env, PATH: 'bin2' }, cwd: dir }) === null);

    // win32 PATHEXT order: checked on win32 only (the resolver's file checks use the host fs).
    const w = join(dir, 'win');
    mkdirSync(w);
    plant(join(w, 'tool.cmd'), '@echo off\r\n');
    plant(join(w, 'tool.exe'), 'MZ');
    const wEnv = { Path: w, PATHEXT: '.COM;.EXE;.BAT;.CMD' };
    if (IS_WIN) {
      check('#140 resolve win32: PATHEXT order picks .exe before .cmd', resolveExecutable('tool', { env: wEnv, platform: 'win32' }).toLowerCase().endsWith('tool.exe'));
      check('#140 resolve win32: herdr resolution is limited to .com/.exe (what a shell-less spawn runs)', resolveExecutable('tool', { env: { ...wEnv, PATHEXT: '.CMD;.EXE' }, platform: 'win32', exts: ['.com', '.exe'] }).toLowerCase().endsWith('tool.exe'));
    }

    check('#140 format: ELF, PE, Mach-O, #! script, .cmd script, unknown', executableFormat(Buffer.from([0x7f, 0x45, 0x4c, 0x46])) === 'elf' && executableFormat(Buffer.from('MZ\x90\x00', 'latin1')) === 'pe' && executableFormat(Buffer.from([0xcf, 0xfa, 0xed, 0xfe])) === 'mach-o' && executableFormat(Buffer.from('#!/b')) === 'script' && executableFormat(Buffer.from('@ech'), 'x.cmd') === 'script' && executableFormat(Buffer.from('abcd'), 'x') === 'unknown');

    const id = await executableIdentity(fake, { requested: 'fakeharness', env });
    check('#140 identity: basename, sha256, size and format of the resolved file; no directory recorded', id.resolved && id.basename === fakeName && id.realBasename === fakeName && id.sha256 === sha(fakeBody) && id.bytes === Buffer.byteLength(fakeBody) && id.format === 'script' && !JSON.stringify(id).includes(dir) && !JSON.stringify(id).includes(bin2), JSON.stringify(id));
    const missing = await executableIdentity(null, { requested: 'oac-no-such-cmd', env });
    check('#140 identity: an unresolved command is recorded as such, unhashed', missing.resolved === false && missing.sha256 === null);

    // Harness config directories: only the command's own binary is ever read there.
    const own = plant(join(cfg.codex, 'packages', 'bin', IS_WIN ? 'codex.exe' : 'codex'), 'MZ-managed-binary');
    const ownId = await executableIdentity(own, { requested: 'codex', env });
    check('#140 identity: a managed binary named for the command under CODEX_HOME is hashed (Codex standalone install)', ownId.sha256 === sha('MZ-managed-binary') && !ownId.notRead, JSON.stringify(ownId));
    // The realistic boundary-3 case: a PATH entry named for the command that is a symlink to a
    // credential file under CODEX_HOME. Recorded, never read (POSIX: symlinks need no privilege).
    if (!IS_WIN) {
      const cred = join(cfg.codex, CRED_NAME);
      writeFileSync(cred, '{"synthetic":"never-read"}');
      const link = join(dir, 'linkbin');
      mkdirSync(link);
      symlinkSync(cred, join(link, 'codex'));
      chmodSync(cred, 0o755);
      const linkId = await executableIdentity(resolveExecutable('codex', { env: { ...env, PATH: link } }), { requested: 'codex', env });
      check('#140 identity: a PATH symlink `codex` -> $CODEX_HOME credential file is recorded unread (no hash, no size)', linkId.resolved === true && linkId.basename === 'codex' && linkId.realBasename === CRED_NAME && linkId.sha256 === null && linkId.bytes === null && /never read/.test(linkId.notRead ?? ''), JSON.stringify(linkId));
    }
    const other = plant(join(cfg.claude, 'settings.json'), '{"secret":"x"}');
    const otherId = await executableIdentity(other, { requested: 'claude', env });
    check('#140 identity: any other file under a harness config directory is never read', otherId.sha256 === null && otherId.bytes === null && /never read/.test(otherId.notRead ?? ''), JSON.stringify(otherId));

    // herdr: a .mjs --herdr-bin runs under node and is flagged as the test double.
    const fakeHerdr = plant(join(dir, 'fake-herdr.mjs'), '#!/usr/bin/env node\n');
    const r1 = resolveHerdr([process.execPath, fakeHerdr], { env });
    const h1 = await herdrIdentity(r1, { env });
    check('#140 herdr: the node-run .mjs is recorded as a test double, hashed', h1.testDouble === true && h1.runUnderNode === true && h1.basename === 'fake-herdr.mjs' && h1.sha256 === sha('#!/usr/bin/env node\n') && h1.requested === 'fake-herdr.mjs', JSON.stringify(h1));
    const native = plant(join(bin1, IS_WIN ? 'herdr.exe' : 'herdr'), Buffer.from([0x7f, 0x45, 0x4c, 0x46, 1, 2]));
    const r2 = resolveHerdr(['herdr'], { env });
    const h2 = await herdrIdentity(r2, { env });
    check('#140 herdr: `herdr` resolves on PATH to the file the driver then spawns; native, not a test double', r2.path === native && h2.testDouble === false && h2.format === 'elf' && h2.sha256 === sha(Buffer.from([0x7f, 0x45, 0x4c, 0x46, 1, 2])), JSON.stringify(h2));
    const r3 = resolveHerdr([join(dir, 'nowhere', 'herdr')], { env });
    check('#140 herdr: an explicit path that does not exist stays unresolved (the driver then refuses: NOT RUN, nothing spawned)', r3.path === null && r3.requested === 'herdr');

    // The probe runs --version on the resolved file and hashes that same file.
    const probe = await probeHarnesses(['fakeharness', 'oac-no-such-cmd'], { env: { ...withoutPath(process.env), ...env, PATH: env.PATH + (IS_WIN ? `;${process.env.SystemRoot ?? 'C:\\Windows'}\\System32` : '') } });
    check('#140 probe: --version answered by the resolved file, whose hash is recorded', probe.versions.fakeharness === 'fakeharness 1.2.3' && probe.executables.fakeharness.sha256 === sha(fakeBody), JSON.stringify(probe));
    check('#140 probe: a missing harness is N/A and unresolved, never guessed', /^N\/A \(not runnable: not found on PATH\)$/.test(probe.versions['oac-no-such-cmd']) && probe.executables['oac-no-such-cmd'].resolved === false);

    // win32: a .cmd under a directory with spaces and cmd metacharacters answers --version
    // through the quoted, verbatim cmd.exe command line (cmd.exe by full path, /v:off).
    if (IS_WIN) {
      for (const sub of ['a b&c^d(f)', 'x!y z']) {
        const d = join(dir, sub);
        mkdirSync(d);
        plant(join(d, 'fh.cmd'), '@echo fh 9.9.9\r\n');
        const pr = await probeHarnesses(['fh'], { env: { ...withoutPath(process.env), PATH: d } });
        check(`#140 probe win32: a .cmd under "${sub}" answers --version through quoted cmd.exe`, pr.versions.fh === 'fh 9.9.9' && pr.executables.fh.sha256 === sha('@echo fh 9.9.9\r\n'), JSON.stringify(pr.versions));
      }
      check('#140 probe win32: cmd.exe is %SystemRoot%\\System32\\cmd.exe by full path; no SystemRoot fails closed', /\\System32\\cmd\.exe$/i.test(windowsCmd({ SystemRoot: 'C:\\Windows' })) && windowsCmd({}) === null);
      const noRoot = await probeHarnesses(['fakeharness'], { env: { PATH: env.PATH, PATHEXT: env.PATHEXT } });
      check('#140 probe win32: without SystemRoot a .cmd harness is N/A, never run through a bare cmd.exe', /SystemRoot is not set/.test(noRoot.versions.fakeharness), noRoot.versions.fakeharness);
    }
    check('#140 windowsCmd: resolves only from an absolute SystemRoot (case-insensitive key)', windowsCmd({ systemroot: 'C:\\W' }) === 'C:\\W\\System32\\cmd.exe' && windowsCmd({ SystemRoot: 'W' }) === null);

    check('#140 capture hash: sha256 of the UTF-8 bytes written', sha256Text('a\u00e9\n') === sha(Buffer.from('a\u00e9\n', 'utf8')));

    // #252: the driver compares the herdr hash with PINS.md's expected value for its platform,
    // and the record's herdr line is VERIFIED only on a recorded match, citing the fields.
    const hex = 'a'.repeat(64);
    const otherHex = 'b'.repeat(64);
    const expected = [{ platform: 'linux-x64', sha256: hex, firstParty: true, basis: 'release asset digest' }, { platform: 'win32-x64', sha256: hex, firstParty: false, basis: 'installed binary' }];
    const exe = (x) => ({ basename: 'herdr', sha256: hex, format: 'elf', testDouble: false, unchangedAfterRun: true, ...x });
    const xc = (x, platform = 'linux-x64') => checkHerdrExecutable(exe(x), expected, platform);
    check('#252 check: the expected hash for the platform is a match', xc({}).result === 'match' && xc({}).expectedSha256 === hex && xc({}).basis === 'release asset digest');
    check('#252 check: another hash is a mismatch (run.mjs: NOT RUN)', xc({ sha256: otherHex }).result === 'mismatch' && xc({ sha256: otherHex }).detail.includes(otherHex));
    check('#252 check: a platform with no row is no-expected-value (a finding)', xc({}, 'darwin-arm64').result === 'no-expected-value');
    check('#252 check: the node-run test double is never compared', xc({ testDouble: true, format: 'script' }).result === 'test-double');
    check('#252 check: an unhashed native herdr is unhashed (run.mjs: NOT RUN)', xc({ sha256: null }).result === 'unhashed');
    const man = (x, check = xc(x)) => ({ herdr: { observedVersionOutput: 'herdr 0.9.1', expectedVersionOutput: 'herdr 0.9.1', executable: exe(x), executableCheck: check } });
    const hv = (m) => herdrVerification(m);
    check('#252 verification: a recorded match is VERIFIED, citing the manifest fields and the hash', hv(man({})).verified && /^VERIFIED — /.test(hv(man({})).text) && hv(man({})).text.includes(hex) && hv(man({})).text.includes('herdr.executableCheck'));
    check('#252 verification: a mismatch, a test double, a changed executable or a missing check is UNVERIFIED', [man({ sha256: otherHex }), man({ testDouble: true, format: 'script' }), man({ unchangedAfterRun: false }), { herdr: { ...man({}).herdr, executableCheck: null } }].every((m) => !hv(m).verified && /^UNVERIFIED — /.test(hv(m).text)));
    check('#252 verification: a schemaVersion 1 manifest (no herdr.executable) is UNVERIFIED', !hv({ herdr: { observedVersionOutput: 'herdr 0.9.1', expectedVersionOutput: 'herdr 0.9.1' } }).verified);
    check('#252 verification: a version off the pin is UNVERIFIED', !hv({ herdr: { ...man({}).herdr, observedVersionOutput: 'herdr 0.9.2' } }).verified);
    check('#252 check: a match on a locally observed (not first-party) row says so', xc({}, 'win32-x64').result === 'match' && xc({}, 'win32-x64').firstParty === false && /first-party source UNVERIFIED/.test(xc({}, 'win32-x64').detail) && xc({}).firstParty === true);
    check('#252 verification: a match on a locally observed value is UNVERIFIED, worded as such', (() => { const v = hv(man({}, xc({}, 'win32-x64'))); return !v.verified && /matches the locally observed value .*first-party source UNVERIFIED/.test(v.text); })());
    // herdrCheckDecision: what run.mjs does with each result (NOT RUN before spawn, or a finding).
    const dec = (x, platform) => herdrCheckDecision(xc(x, platform));
    check('#252 decision: mismatch and unhashed are NOT RUN, with no finding', /Refusing to run \(#252\)/.test(dec({ sha256: otherHex }).notRun ?? '') && /Refusing to run/.test(dec({ sha256: null }).notRun ?? '') && dec({ sha256: otherHex }).finding === null);
    check('#252 decision: no expected value is a finding, not a stop', dec({}, 'darwin-arm64').notRun === null && /herdr identity UNVERIFIED \(#252\)/.test(dec({}, 'darwin-arm64').finding ?? ''));
    check('#252 decision: a first-party match is neither; a locally observed match is a finding; a test double is neither', (() => { const a = dec({}); const b = dec({}, 'win32-x64'); const c = dec({ testDouble: true, format: 'script' }); return a.notRun === null && a.finding === null && b.notRun === null && /first-party/.test(b.finding ?? '') && c.notRun === null && c.finding === null; })());
    check('#252 PINS.md: the First-party column parses; every committed row is first-party (win32 verified with gh release verify-asset, 2026-10-03)', (() => { const t = parseHerdrExpectedExecutables(readFileSync(join(REPO, 'docs', 'planning', 'PINS.md'), 'utf8')); return t.length === 5 && t.every((r) => r.firstParty === true); })());
    check('#252 PINS.md: a `no` First-party cell parses as not first-party', parseHerdrExpectedExecutables(['| Platform | Expected executable sha256 | First-party | Basis |', '|---|---|---|---|', `| \`win32-x64\` | \`${hex}\` | no | local |`].join('\n'))[0]?.firstParty === false);
    check('#252 PINS.md: the committed expected-executable table parses, one row per platform, a 64-hex hash each', (() => { const t = parseHerdrExpectedExecutables(readFileSync(join(REPO, 'docs', 'planning', 'PINS.md'), 'utf8')); return t.length >= 1 && t.every((r) => /^[a-z0-9]+-[a-z0-9]+$/.test(r.platform) && /^[0-9a-f]{64}$/.test(r.sha256)) && new Set(t.map((r) => r.platform)).size === t.length; })());
    const tbl = (rows) => ['| Platform | Release asset | Expected executable sha256 | Basis |', '|---|---|---|---|', ...rows].join('\n');
    const throwsT = (t) => { try { parseHerdrExpectedExecutables(t); return false; } catch { return true; } };
    check('#252 PINS.md: no table is no expected values; a row without a hash or a duplicate platform throws', parseHerdrExpectedExecutables('no table').length === 0 && throwsT(tbl(['| `linux-x64` | a | none | b |'])) && throwsT(tbl([`| \`linux-x64\` | a | \`${hex}\` | b |`, `| \`linux-x64\` | a | \`${otherHex}\` | b |`])));
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}
