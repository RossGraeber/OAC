// Cross-checks between the requirement indexes and the fixtures:
//
// - spec/session-channels.md Appendix A, spec/security.md Appendix A, spec/bindings/mcp.md
//   §12.3 and spec/interfaces.md Appendix A each list every requirement id their document
//   defines, once, at the level (MUST, MUST NOT, SHOULD, SHOULD NOT, MAY) its defining
//   sentence uses;
// - every fixture an index row names exists, as exactly one file;
// - every fixture file is named by at least one index row;
// - every fixture's `requirement` is an id its `spec` document defines.

const DOCS = [
  { prefix: 'SC', path: 'spec/session-channels.md', heading: '## Appendix A. Requirement index' },
  { prefix: 'SEC', path: 'spec/security.md', heading: '## Appendix A. Requirement index' },
  { prefix: 'MCPB', path: 'spec/bindings/mcp.md', heading: '### 12.3 Index' },
  { prefix: 'IFC', path: 'spec/interfaces.md', heading: '## Appendix A. Requirement index' },
];
export const DOC_OF_PREFIX = Object.fromEntries(DOCS.map((d) => [d.prefix, d.path]));

const KEYWORD = /\b(MUST NOT|MUST|SHOULD NOT|SHOULD|MAY)\b/;
const ID = '((?:SC|SEC|MCPB|IFC)-[A-Z]+-[0-9]{3})';

// Requirement ids defined in a document's body: a paragraph (or list item) that starts with
// `[ID]`, and the first keyword of that paragraph.
export function definitions(text, prefix) {
  const defs = new Map();
  const lines = text.split('\n');
  const re = new RegExp(`^\\s*(?:- )?\\[${ID}\\]\\s`);
  for (let k = 0; k < lines.length; k++) {
    const m = re.exec(lines[k]);
    if (!m || !m[1].startsWith(prefix + '-')) continue;
    let para = lines[k];
    for (let j = k + 1; j < lines.length && lines[j].trim() !== '' && !re.test(lines[j]) && !/^\s*- /.test(lines[j]); j++) para += ' ' + lines[j];
    // A paragraph that only starts with an id ("[X] also closes a gap") defines nothing:
    // a defining sentence carries a keyword in its first sentence.
    const first = para.replace(re, '').split(/\.(?:\s|$)/)[0];
    const kw = KEYWORD.exec(first);
    if (!kw) continue;
    if (defs.has(m[1])) defs.get(m[1]).duplicate = true;
    else defs.set(m[1], { level: kw ? kw[1] : null, line: k + 1 });
  }
  return defs;
}

// The rows of a document's index: [{ id, level, cell, line }].
export function indexRows(text, heading) {
  const start = text.indexOf(heading);
  if (start < 0) throw new Error(`index heading not found: ${heading}`);
  const base = text.slice(0, start).split('\n').length;
  const rows = [];
  const lines = text.slice(start).split('\n');
  for (let k = 0; k < lines.length; k++) {
    if (k > 0 && /^#{1,3} /.test(lines[k])) break;
    const m = new RegExp(`^\\| ${ID} \\| (MUST NOT|MUST|SHOULD NOT|SHOULD|MAY) \\|(.*)\\|\\s*$`).exec(lines[k]);
    if (m) rows.push({ id: m[1], level: m[2], cell: m[3], line: base + k });
  }
  return rows;
}

// Fixture references in an index cell, as { dir, id, suffix } or { dir, id, suffix: '*' }.
// Forms: `dir/ID.p01`, `tests/protocol/dir/ID.p01-slug.json`, `ID.n01` (directory from
// the id), `.n02` (same id as the reference before), and "`X` to `.n06`" ranges.
export function cellReferences(cell) {
  const refs = [];
  const spanRe = /`([^`]*)`/g;
  let prev = null;
  let lastEnd = 0;
  let m;
  while ((m = spanRe.exec(cell)) !== null) {
    const between = cell.slice(lastEnd, m.index);
    lastEnd = spanRe.lastIndex;
    const span = m[1];
    let ref = null;
    let full;
    if ((full = /^(?:tests\/protocol\/)?([a-z]+-[a-z]+)\/([A-Z]+-[A-Z]+-[0-9]{3})\.([pn][0-9]{2}|\*)(-[a-z0-9-]+\.json)?$/.exec(span))) {
      ref = { dir: full[1], id: full[2], suffix: full[3], exact: full[4] ? `${full[2]}.${full[3]}${full[4]}` : null };
    } else if ((full = /^([A-Z]+-[A-Z]+-[0-9]{3})\.([pn][0-9]{2}|\*)$/.exec(span))) {
      ref = { dir: dirOfId(full[1]), id: full[1], suffix: full[2] };
    } else if ((full = /^\.([pn][0-9]{2})$/.exec(span)) && prev) {
      ref = { dir: prev.dir, id: prev.id, suffix: full[1] };
    }
    if (!ref) continue;
    if (/^\s*to\s*$/.test(between) && prev && prev.id === ref.id && prev.suffix[0] === ref.suffix[0]) {
      const from = Number(prev.suffix.slice(1));
      const to = Number(ref.suffix.slice(1));
      for (let n = from + 1; n < to; n++) refs.push({ dir: ref.dir, id: ref.id, suffix: ref.suffix[0] + String(n).padStart(2, '0') });
    }
    refs.push(ref);
    prev = ref;
  }
  return refs;
}

export const dirOfId = (id) => id.split('-').slice(0, 2).join('-').toLowerCase();

// Runs every cross-check. `read(path)` returns a file's text; `fixtures` is a list of
// { dir, file, fixture } for every fixture file. Returns a list of problem strings.
export function checkIndexes(read, fixtures) {
  const problems = [];
  const referenced = new Set();
  const defined = new Map(); // id -> doc path
  const byDir = new Map();
  for (const f of fixtures) {
    if (!byDir.has(f.dir)) byDir.set(f.dir, []);
    byDir.get(f.dir).push(f.file);
  }
  for (const doc of DOCS) {
    const text = read(doc.path);
    const defs = definitions(text, doc.prefix);
    const rows = indexRows(text, doc.heading);
    const rowIds = new Map();
    for (const [id, d] of defs) {
      defined.set(id, doc.path);
      if (d.duplicate) problems.push(`${doc.path}: ${id} is defined by more than one sentence`);
    }
    for (const row of rows) {
      if (rowIds.has(row.id)) problems.push(`${doc.path}:${row.line}: ${row.id} has more than one index row`);
      rowIds.set(row.id, row);
      const d = defs.get(row.id);
      if (!d) problems.push(`${doc.path}:${row.line}: index row ${row.id} names no requirement defined in the document`);
      else if (d.level !== row.level) problems.push(`${doc.path}:${row.line}: ${row.id} is indexed as ${row.level} but its sentence (line ${d.line}) uses ${d.level}`);
      for (const ref of cellReferences(row.cell)) {
        const files = (byDir.get(ref.dir) || []).filter((f) => (ref.exact ? f === ref.exact : ref.suffix === '*' ? f.startsWith(`${ref.id}.`) : f.startsWith(`${ref.id}.${ref.suffix}-`) || f === `${ref.id}.${ref.suffix}.json`));
        if (files.length === 0) problems.push(`${doc.path}:${row.line}: ${row.id} names ${ref.dir}/${ref.id}.${ref.suffix}, which matches no fixture`);
        else if (ref.suffix !== '*' && files.length > 1) problems.push(`${doc.path}:${row.line}: ${row.id} names ${ref.dir}/${ref.id}.${ref.suffix}, which matches ${files.length} fixtures`);
        for (const f of files) referenced.add(`${ref.dir}/${f}`);
      }
    }
    for (const id of defs.keys()) if (!rowIds.has(id)) problems.push(`${doc.path}: ${id} is defined but has no index row`);
  }
  for (const f of fixtures) {
    const key = `${f.dir}/${f.file}`;
    if (!referenced.has(key)) problems.push(`tests/protocol/${key}: named by no index row`);
    const id = f.fixture && f.fixture.requirement;
    if (id && defined.get(id) !== f.fixture.spec) problems.push(`tests/protocol/${key}: requirement ${id} is not defined in ${f.fixture.spec}`);
  }
  return problems;
}
