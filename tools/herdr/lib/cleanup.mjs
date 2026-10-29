// Cleanup hooks a scenario registers for things it changed outside the run's scratch directory (G8 S6, lead decision
// 2026-10-29: the driver registers an MCP server with the Codex CLI and must remove it on every path). run.mjs runs
// them in its own teardown, after the scenario body has been given its two seconds to finish and before it hashes
// the harness configuration for the last time, so an abort or an expired timebox that cuts the scenario's own
// `finally` short still leaves the machine as it was. A hook is run once, in the order registered; it should be
// idempotent (the scenario's own `finally` may have run it already); a throw or a timeout is recorded, never raised.

const DEFAULT_TIMEOUT_MS = 60000;

export function makeCleanups() {
  const list = [];
  return {
    // fn: () => Promise|any. name: a label for the record.
    add(name, fn) {
      if (typeof fn !== 'function') throw new TypeError('a cleanup hook needs a function');
      list.push({ name: String(name), fn, done: false });
    },
    get size() {
      return list.length;
    },
    // -> [{ name, ok, error, ms }] for the hooks that had not run yet.
    async runAll({ timeoutMs = DEFAULT_TIMEOUT_MS } = {}) {
      const out = [];
      for (const c of list) {
        if (c.done) continue;
        c.done = true;
        const t0 = Date.now();
        let timer = null;
        try {
          await Promise.race([
            Promise.resolve().then(c.fn),
            new Promise((_, reject) => {
              timer = setTimeout(() => reject(new Error(`timed out after ${timeoutMs} ms`)), timeoutMs);
            }),
          ]);
          out.push({ name: c.name, ok: true, error: null, ms: Date.now() - t0 });
        } catch (err) {
          out.push({ name: c.name, ok: false, error: String(err?.message ?? err).slice(0, 200), ms: Date.now() - t0 });
        } finally {
          clearTimeout(timer);
        }
      }
      return out;
    },
  };
}
