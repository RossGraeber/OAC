#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0
//
// Mutation check for the security suite (#60, F11): does the suite catch a regression in
// the core mitigations it claims to prove?
//
// Copies the workspace to a work directory, then for each mutation below plants one
// regression in a copy of a core/ source file (a mitigation switched off or weakened, the
// way a careless change would), runs `cargo test -p oac-security-suite` against the copy,
// and expects at least one test to fail. An unmodified copy runs first and must pass.
// The real core/ is never touched.
//
//   node tests/security/mutation-check.mjs                    # every mutation
//   node tests/security/mutation-check.mjs --only <substring> # the mutations whose name matches
//   node tests/security/mutation-check.mjs --work-dir <dir>   # default: target/security-mutation
//
// Not part of the default CI run: each mutation recompiles core/ and the suite (a few
// minutes in all, with the work directory's target/ reused between runs). Run it when the
// suite or a core mitigation changes. Node built-ins only; cargo runs with --offline.
// Exit codes: 0 = every mutation caught and the control passes; 1 = a mutation survived
// or the control failed; 2 = usage or environment error.

import { spawnSync } from 'node:child_process';
import { cpSync, mkdirSync, readdirSync, readFileSync, rmSync, utimesSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');

// Each mutation: the threat row whose mitigation it breaks, the file, and the exact text to
// replace (it must occur `count` times, default once) with its replacement, or `edits`, a
// list of such replacements applied together. `coreOnly` marks a mutant the suite cannot
// reach through the public API: it must then be caught by that cited `oac-core` test, the
// one THREATS cites for the row.
export const MUTATIONS = [
  {
    name: '06 rows 1/3: signature not checked (verify_strict result ignored)',
    file: 'core/src/signing.rs',
    from: '.map_err(|_| invalid("SEC-SIG-024"))?;',
    to: '.ok();',
  },
  {
    name: '06 row 4: replay window always passes',
    file: 'core/src/replay.rs',
    from: 'if inside_replay_window(msg.envelope().created_at(), now) {',
    to: 'if true {',
  },
  {
    name: '06 row 4: every hand-off settles as not handed off (no duplicate suppression)',
    file: 'core/src/receiver.rs',
    from: 'if outcome.may_be_handed_off() {\n        reservation.handed_off();',
    to: 'if false {\n        reservation.handed_off();',
  },
  {
    name: '06 row 2: step 4 ignores a deny',
    file: 'core/src/authorization.rs',
    from: 'if !decision.permits(Kind::Deliver) {\n            return Err(refuse("SEC-AUZ-001", None));\n        }',
    to: '',
  },
  {
    name: '06 row 7: a scope grant covers every own session',
    file: 'core/src/authorization.rs',
    from: 'LocalSide::Scope(scope) => own.get(s) == Some(scope),',
    to: 'LocalSide::Scope(_) => own.contains_key(s),',
  },
  {
    name: '06 row 20: a claim on a bound session id is not refused at step 4',
    file: 'core/src/authorization.rs',
    from: 'Some(Binding::Key(bound)) if bound != &key => {',
    to: 'Some(Binding::Key(bound)) if false && bound != &key => {',
  },
  {
    name: '06 row 8: key removal keeps the key\'s grants',
    file: 'core/src/authorization.rs',
    from: '.partition(|g| g.names_key(key_id));',
    to: '.partition(|_| false);',
  },
  {
    name: '06 row 11: relay permitted for every own session',
    file: 'core/src/authorization.rs',
    from: '(self.own_sessions.contains_key(session) && self.relay.contains(session))',
    to: '(self.own_sessions.contains_key(session))',
  },
  {
    name: 'S13 presence: audience not checked',
    file: 'core/src/presence_auth.rs',
    from: 'if audience.as_str() != Some(engine.own_key_id().as_str()) {',
    to: 'if false {',
  },
  {
    name: 'S13 presence: freshness not checked',
    file: 'core/src/presence_auth.rs',
    from: '.is_some_and(|t| inside_replay_window(&t, &engine.now()));',
    to: '.is_some();',
  },
  {
    name: 'S13 receipts: wrong receiver accepted (check 4 skipped)',
    file: 'core/src/receipt_auth.rs',
    from: 'if bound != Some(Binding::Key(key)) {',
    to: 'if false {',
  },
  {
    name: 'S13 pairing: the code no longer binds the keys and nonces',
    file: 'core/src/pairing.rs',
    from: 'format!("{:06}", u64::from_be_bytes(first) % 1_000_000)',
    to: 'String::from("000000")',
  },
  {
    name: 'X exhaustion: no per-issuer presence quota',
    file: 'core/src/registry.rs',
    from: 'if self.held_by(issuer) >= self.per_issuer_quota {\n            let own_stale',
    to: 'if false {\n            let own_stale',
  },
  {
    name: 'X exhaustion: presence registry capacity not enforced',
    file: 'core/src/registry.rs',
    from: 'if self.len() < self.capacity {\n            return Ok(());\n        }\n        self.sweep(now);',
    to: 'if true {\n            return Ok(());\n        }\n        self.sweep(now);',
  },
  {
    name: 'X exhaustion: envelope size limit not enforced',
    file: 'core/src/envelope.rs',
    from: 'if octets.len() as u64 > limits.max_envelope_octets {',
    to: 'if false {',
  },
  // The finer variants of PR #327 review N2.
  {
    name: 'S13 malleability: non-strict ed25519 verification (verify for verify_strict)',
    file: 'core/src/signing.rs',
    edits: [
      {
        from: '.verify_strict(&input, &ed25519_dalek::Signature::from_bytes(&octets))',
        to: '.verify(&input, &ed25519_dalek::Signature::from_bytes(&octets))',
      },
      {
        from: 'let input = signing_input(domain, obj).map_err(',
        to: 'use ed25519_dalek::Verifier as _;\n    let input = signing_input(domain, obj).map_err(',
      },
    ],
  },
  {
    name: '06 row 4: replay window closed at its ends (<= for <)',
    file: 'core/src/replay.rs',
    from: 't - WINDOW_NANOS < c && c < t + WINDOW_NANOS',
    to: 't - WINDOW_NANOS <= c && c <= t + WINDOW_NANOS',
  },
  {
    name: '06 row 4: replay window widened by 0.5 s',
    file: 'core/src/replay.rs',
    from: 't - WINDOW_NANOS < c && c < t + WINDOW_NANOS',
    to: 't - WINDOW_NANOS - 500_000_000 < c && c < t + WINDOW_NANOS + 500_000_000',
  },
  // The store and record bounds of #320, #325 and #328: each bound and fairness rule.
  {
    name: 'X exhaustion: no per-key duplicate-store quota (#320)',
    file: 'core/src/replay.rs',
    from: 'n < per_key_share && self.entries.len() + n < capacity',
    to: 'self.entries.len() + n < capacity',
  },
  {
    name: 'X exhaustion: no duplicate-store headroom rule (#320)',
    file: 'core/src/replay.rs',
    from: 'n < per_key_share && self.entries.len() + n < capacity',
    to: 'n < per_key_share && self.entries.len() < capacity',
  },
  {
    name: 'X exhaustion: a key\'s duplicate-store count is never released (#320)',
    file: 'core/src/replay.rs',
    from: 'if let Some(n) = self.per_key.get_mut(key.key_id()) {\n                *n -= 1;',
    to: 'if let Some(n) = self.per_key.get_mut(key.key_id()) {\n                *n -= 0;',
  },
  {
    name: 'X exhaustion: envelope-created bindings unbounded (#325)',
    file: 'core/src/authorization.rs',
    from: 'if !self.envelope_bound.has_room(key) {',
    to: 'if false {',
  },
  {
    name: 'X exhaustion: an envelope binding a hand-off record uses can be evicted (#325)',
    file: 'core/src/authorization.rs',
    from: 'if &e.key != key || e.pinned == pinned {',
    to: 'if true {',
  },
  {
    name: 'X exhaustion: binding fair share takes from a key only one ahead (#325)',
    file: 'core/src/authorization.rs',
    from: 'kb.len() >= n + 2',
    to: 'kb.len() >= n + 1',
  },
  {
    name: 'X exhaustion: binding fair share tie broken by key-id order (#325)',
    file: 'core/src/authorization.rs',
    from: '.max_by_key(|(_, kb)| (kb.len(), kb.latest))',
    to: '.max_by_key(|(_, kb)| kb.len())',
  },
  {
    name: 'X exhaustion: an announcement does not take an envelope binding out of the bound (#325)',
    file: 'core/src/authorization.rs',
    from: 'Some(Binding::Key(k)) if k == key => {\n                self.envelope_bound.untrack(session);',
    to: 'Some(Binding::Key(k)) if k == key => {',
    coreOnly: 'authorization::tests::envelope_bindings_leave_the_bound_when_referred_to',
  },
  {
    name: 'X exhaustion: a binding evicted before its hand-off is not bound again (#325)',
    file: 'core/src/authorization.rs',
    from: '&& self.trusted.get(&k).is_some()',
    to: '&& false',
    coreOnly: 'authorization::tests::envelope_bindings_leave_the_bound_when_referred_to',
  },
  {
    name: 'X exhaustion: a late copy from an ended own session makes its partition again (#328)',
    file: 'core/src/authorization.rs',
    from: 'if own && !self.own_sessions.contains_key(&record.from) {\n            return;\n        }',
    to: '',
  },
  {
    name: 'X exhaustion: expired record partitions are never swept (#328)',
    file: 'core/src/authorization.rs',
    from: '        self.sweep(now);\n        if !self.parts.contains_key(&key)',
    to: '        if !self.parts.contains_key(&key)',
  },
  {
    name: 'X exhaustion: a refreshed record partition keeps its old expiry (#328)',
    file: 'core/src/authorization.rs',
    from: 'if after != before {',
    to: 'if false {',
  },
  {
    // With random nonces the suite cannot tell a code that binds the keys from one that
    // binds only the nonces; core's deterministic unit test can (cited in THREATS).
    name: 'S13 pairing: the code hashes the nonces only, not the principals or keys',
    file: 'core/src/pairing.rs',
    from: '    put_principal(&mut h, initiator.0);\n    h.update(initiator.1.to_octets());\n    put_principal(&mut h, responder.0);\n    h.update(responder.1.to_octets());\n',
    to: '',
    coreOnly: 'pairing::tests::substituted_key_or_nonce_changes_the_code_or_fails',
  },
];

const EXCLUDE_TOP = new Set(['.git', 'target', 'node_modules', '.claude', '.agents']);

// Sets the access and modification times of every file under `dir` to `when`. cpSync keeps
// the source's times (on Windows to the millisecond), so a copied core/ file could look
// older than a mutant the reused target/ built from in an earlier run, and cargo would test
// that stale mutant (PR #327 review N6).
function touchTree(dir, when) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const p = join(dir, entry.name);
    if (entry.isDirectory()) touchTree(p, when);
    else if (entry.isFile()) utimesSync(p, when, when);
  }
}

export function copyWorkspace(ws) {
  rmSync(ws, { recursive: true, force: true });
  mkdirSync(ws, { recursive: true });
  // Entry by entry: the work directory may sit inside the repository's target/.
  for (const entry of readdirSync(repoRoot)) {
    if (EXCLUDE_TOP.has(entry)) continue;
    cpSync(join(repoRoot, entry), join(ws, entry), { recursive: true });
  }
  touchTree(ws, new Date());
}

function cargo(ws, targetDir, args) {
  const r = spawnSync('cargo', args, {
    cwd: ws,
    encoding: 'utf8',
    env: { ...process.env, CARGO_TARGET_DIR: targetDir },
    maxBuffer: 256 * 1024 * 1024,
  });
  if (r.error) throw r.error;
  return r;
}

function runTests(ws, targetDir, pkg, filter) {
  const args = ['test', '-p', pkg, '--offline', '--locked', '--no-fail-fast'];
  if (filter) args.push(filter);
  const r = cargo(ws, targetDir, args);
  const out = `${r.stdout}\n${r.stderr}`;
  const failed = [...out.matchAll(/^test (\S+) \.\.\. FAILED$/gm)].map((m) => m[1]);
  const compileError = /error(\[E\d+\])?: /.test(r.stderr) && failed.length === 0;
  return { status: r.status, failed, compileError, tail: out.split(/\r?\n/).slice(-15).join('\n') };
}

function edits(m) {
  return m.edits ?? [{ from: m.from, to: m.to, count: m.count }];
}

function main(argv) {
  let only = null;
  let workDir = join(repoRoot, 'target', 'security-mutation');
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === '--only' && argv[i + 1]) only = argv[++i];
    else if (argv[i] === '--work-dir' && argv[i + 1]) workDir = resolve(argv[++i]);
    else {
      console.error('usage: mutation-check.mjs [--only <substring>] [--work-dir <dir>]');
      return 2;
    }
  }
  const ws = join(workDir, 'ws');
  const targetDir = join(workDir, 'target');
  mkdirSync(workDir, { recursive: true });
  copyWorkspace(ws);
  try {
    // Belt and braces with touchTree: nothing core/ built in an earlier run survives.
    const clean = cargo(ws, targetDir, ['clean', '-p', 'oac-core', '--offline']);
    if (clean.status !== 0) {
      console.log(`FAIL  cargo clean -p oac-core\n${clean.stderr}`);
      return 2;
    }
    const control = runTests(ws, targetDir, 'oac-security-suite');
    if (control.status !== 0) {
      console.log(`FAIL  control: the unmodified suite does not pass\n${control.tail}`);
      return 1;
    }
    console.log('pass  control: the unmodified suite passes');
    let survived = 0;
    const cases = MUTATIONS.filter((m) => !only || m.name.includes(only));
    for (const m of cases) {
      const p = join(ws, m.file);
      const before = readFileSync(p, 'utf8');
      let text = before.replace(/\r\n/g, '\n');
      let bad = null;
      for (const e of edits(m)) {
        const count = text.split(e.from).length - 1;
        if (count !== (e.count ?? 1)) {
          bad = `expected ${JSON.stringify(e.from.slice(0, 60))} ${e.count ?? 1} time(s) in ${m.file}, found ${count}`;
          break;
        }
        text = text.split(e.from).join(e.to);
      }
      if (bad) {
        console.log(`FAIL  ${m.name}: ${bad}`);
        survived++;
        continue;
      }
      writeFileSync(p, text);
      try {
        const r = runTests(ws, targetDir, 'oac-security-suite');
        if (r.compileError) {
          console.log(`FAIL  ${m.name}: the mutant does not compile (fix the mutation)\n${r.tail}`);
          survived++;
        } else if (r.status !== 0) {
          console.log(`pass  ${m.name}: caught by ${r.failed.join(', ')}`);
        } else if (m.coreOnly) {
          // The suite cannot reach this through the public API; THREATS cites the core
          // test that does. Check that it does.
          const name = m.coreOnly.split('::').pop();
          const c = runTests(ws, targetDir, 'oac-core', name);
          if (c.status !== 0 && c.failed.some((f) => f.endsWith(name))) {
            console.log(`pass  ${m.name}: not reachable by the suite; caught by the cited core test ${m.coreOnly}`);
          } else {
            console.log(`FAIL  ${m.name}: SURVIVED the suite and the cited core test ${m.coreOnly}`);
            survived++;
          }
        } else {
          console.log(`FAIL  ${m.name}: SURVIVED, no test failed`);
          survived++;
        }
      } finally {
        writeFileSync(p, before);
      }
    }
    console.log(`mutation check: ${cases.length - survived}/${cases.length} mutations caught`);
    return survived ? 1 : 0;
  } finally {
    rmSync(ws, { recursive: true, force: true });
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    process.exitCode = main(process.argv.slice(2));
  } catch (e) {
    console.error(e?.stack ?? String(e));
    process.exitCode = 2;
  }
}
