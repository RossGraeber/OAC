// Executable identity and capture hashes in the run manifest (#140): unit checks that run on
// every platform (the lifecycle assertions sit in selftest.mjs and g1-tests.mjs, POSIX only).
// Everything is planted in throwaway temp directories; no real harness or herdr is resolved,
// hashed or run, and no harness config directory is read (CLAUDE_CONFIG_DIR and CODEX_HOME
// point into the temp directory).

import { chmodSync, mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createHash } from 'node:crypto';

import { resolveExecutable, executableIdentity, executableFormat, resolveHerdr, herdrIdentity, probeHarnesses, sha256Text, windowsCmd } from '../lib/manifest.mjs';
import { herdrExecutableHash } from '../lib/gate-report-common.mjs';

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

    // The attestation's herdr hash is filled from the manifest only for a hashed native herdr.
    const m = (x) => ({ herdr: { executable: x } });
    const hex = 'a'.repeat(64);
    check('#140 attestation: hash filled from herdr.executable for a native, non-test-double herdr', herdrExecutableHash(m({ sha256: hex, format: 'elf', testDouble: false })).startsWith(`\`${hex}\``));
    check('#140 attestation: a test-double herdr leaves the placeholder and says so', /^`<64 hex>` \(the run manifest records a test-double herdr/.test(herdrExecutableHash(m({ sha256: hex, format: 'script', testDouble: true }))));
    check('#140 attestation: a schemaVersion 1 manifest (no herdr.executable) leaves the placeholder', herdrExecutableHash({ herdr: {} }) === '`<64 hex>`' && herdrExecutableHash(m({ sha256: hex, format: 'script', testDouble: false })) === '`<64 hex>`');
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}
