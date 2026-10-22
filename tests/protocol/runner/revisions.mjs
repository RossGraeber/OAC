// The revisions each spec document has had (#422). A fixture cites the one it was written
// against (spec/session-channels.md §3.3), and the runner accepts only these.
//
// STANDING RULE: every spec revision bump also updates this list in the same change.
// checkRevisions() below enforces it: the conformance step of scripts/local-ci.mjs fails
// when a revision named by a spec document is not listed here.

export const REVISIONS = {
  'spec/bindings/mcp.md': ['0.1', '0.2', '0.3', '0.4', '0.5'],
  'spec/security.md': ['0.1', '0.2', '0.3', '0.4', '0.5', '0.6', '0.7'],
  'spec/session-channels.md': ['0.1', '0.2', '0.3', '0.4'],
  'spec/interfaces.md': ['0.1', '0.2', '0.3', '0.4', '0.5', '0.6', '0.7', '0.8', '0.9', '0.10', '0.11'],
};

// The spec documents that carry a revision: spec/*.md and spec/bindings/*.md.
export const SPEC_DOCUMENTS = ['spec/bindings/mcp.md', 'spec/security.md', 'spec/session-channels.md', 'spec/interfaces.md'];

// The revisions a document names: the one in its header (`**Revision:** 0.9 ...` or
// `**Binding revision:** `0.3` ...`, within the first 30 lines) and every row of its
// "Revision history" appendix (`| 0.4 (...) | date | change |`).
export function revisionsNamedBy(text) {
  const lines = text.split('\n');
  const header = new Set();
  for (const line of lines.slice(0, 30)) {
    const m = /\*\*(?:Binding )?[Rr]evision:\*\*\s*`?(\d+\.\d+)`?/.exec(line);
    if (m) header.add(m[1]);
  }
  const history = new Set();
  let inHistory = false;
  for (const line of lines) {
    if (/^## /.test(line)) inHistory = /^## Appendix [A-Z]\. Revision history\s*$/.test(line);
    else if (inHistory) {
      const m = /^\|\s*(\d+\.\d+)\b/.exec(line);
      if (m) history.add(m[1]);
    }
  }
  return { header, history };
}

// Problems (strings) for every spec document whose revisions are not all in `revisions`.
// `read(rel)` returns a repo file's text; `docs` and `revisions` default to the real ones.
export function checkRevisions(read, revisions = REVISIONS, docs = SPEC_DOCUMENTS) {
  const problems = [];
  for (const rel of docs) {
    const listed = revisions[rel];
    if (!listed) {
      problems.push(`${rel}: no revision list in tests/protocol/runner/revisions.mjs`);
      continue;
    }
    const { header, history } = revisionsNamedBy(read(rel));
    if (header.size === 0) problems.push(`${rel}: no revision header found in its first 30 lines`);
    if (history.size === 0) problems.push(`${rel}: no "Revision history" appendix rows found`);
    for (const [where, set] of [['header', header], ['revision history', history]]) {
      for (const r of set) {
        if (!listed.includes(r)) problems.push(`${rel}: revision ${r} (${where}) is not in REVISIONS in tests/protocol/runner/revisions.mjs`);
      }
    }
  }
  return problems;
}
