// SPDX-License-Identifier: Apache-2.0

//! The static half of "adapters route through core policy rather than straight to a
//! transport" (#59 acceptance; `spec/interfaces.md` §5.1): every adapter source file is
//! parsed with `syn`, every path is resolved through the file's `use` declarations and
//! `extern crate` renames, and the resolved paths are checked against the core and
//! transport items an adapter has no business reaching.
//!
//! - [IFC-ADP-001]: no transport operation. No path into `oac_core::transport`, and no
//!   `oac_transport_*` crate. (`scripts/check-crate-deps.mjs` already stops an adapter from
//!   depending on a transport crate.)
//! - [IFC-ADP-002]: no envelope, presence record, registration record or receipt is
//!   created, signed, verified or altered. No path into the core's signing, key, trust,
//!   canonical-form, registration or replay modules, no `EnvelopeDraft`,
//!   `receive_envelope`, `DeliveryReceipt::new` or `PresenceRecord::new`, and no
//!   `.sign_envelope(..)` call.
//! - [IFC-ADP-007]: no binding, session id or authorization decision. No
//!   `SessionId::from_random_octets` (parsing an id a harness names is allowed) and no path
//!   into the core's authorization or pairing modules.
//! - [IFC-ADP-013]: no `Connection::accept`.
//! - [`PLANT`]: no path into this suite's own planted breaches (`oac_contract_adapter::plant`).
//!
//! [`implements_provider_adapter`] is the tripwire of `tests/real_adapters.rs`: an `impl`
//! whose trait resolves to `oac_core::adapter::ProviderAdapter`, under any alias.
//!
//! # How paths are resolved
//!
//! `use` trees are expanded (groups, nesting, `self`, renames such as
//! `use oac_core as c` or `use oac_core::adapter::ProviderAdapter as PA`), renames chain
//! (`use c::transport as t`), `extern crate oac_core as c` counts as a rename, and
//! whitespace inside a path does not matter (the parser sees tokens). Raw identifiers
//! (`r#transport`) are read without their prefix. Leading `crate`, `self` and `super`
//! segments are dropped, every suffix of a path is resolved, and the imports and `pub use`
//! re-exports of all the files given are collected before any is checked, so
//! `crate::Port` in one file reaches `pub use oac_core::adapter::ProviderAdapter as Port`
//! in another. Paths in attribute token lists (`#[derive(..)]`) are checked. A glob import
//! (`use oac_core::*`) is itself checked, and a path whose first segment is not otherwise
//! resolved is also tried under every glob prefix of the file. Renames are collected for the
//! whole file, whatever block or module they appear in, which can only add findings.
//!
//! # Which files are read
//!
//! The files given, and every file a `mod` declaration in them loads (#324): `mod m;` at
//! `m.rs` or `m/mod.rs`, and `#[path = ".."] mod m;` at its path, resolved against the
//! declaring file's directory as rustc resolves it, inline modules included. Every candidate
//! location that exists is read, which can only add findings. Paths are resolved lexically,
//! so a symlink on the way to a file the scan would read, or among the files given, fails
//! closed.
//!
//! What the scan cannot follow fails closed: a finding under every requirement, as for a
//! file that does not parse. That is
//! - a `#[path]` that names no file or is not a string literal;
//! - a `path = ..` anywhere inside a `cfg_attr`, at any depth of nesting;
//! - any invocation of `include!`, `include_str!` or `include_bytes!`, under any path
//!   (`::core::include!`) or alias (`use std::include as inc;`, which itself fails closed),
//!   in code or inside a macro's tokens;
//! - inside a macro's tokens (an invocation's arguments or a `macro_rules!` body), a
//!   `#[path]` attribute and any out-of-line `mod m;` or `mod $m;`. Such a module's file
//!   depends on where the macro expands, which the scan does not model (under an inline
//!   `#[path]` module it can sit outside `src/`).
//!
//! # Macro tokens
//!
//! Macro bodies are token streams, not paths. The scan walks each macro's tokens (an
//! invocation's arguments, a `macro_rules!` body, an attribute's list) and resolves every
//! run of `::`-joined identifiers as it resolves a parsed path, so renames, globs and
//! `crate`/`self`/`super` apply, and `$crate` is read as `crate` (#324). An
//! `impl Trait for ..` in a macro's tokens is seen by the tripwire under the same
//! resolution; an `impl $t for ..` whose trait is a metavariable counts when some macro
//! invocation in the crate is handed a path to `ProviderAdapter`. The token text, string
//! literals included, is also matched for the forbidden paths written in full.
//!
//! # What a static scan still cannot see
//!
//! It is a tripwire, not a proof. It reads source, not behaviour:
//!
//! - A macro that builds a path from pieces (`concat_idents!`, a `macro_rules!` that pastes
//!   an ident, as `oac_core::$i::..`) or a procedural macro that expands to one is not seen.
//! - Code the scan is not given: `build.rs` output, and other crates (a helper crate the
//!   adapter depends on is scanned only if its files are passed in; `check-crate-deps.mjs`
//!   stops that crate from being a transport).
//! - Calls through values: a `dyn Transport` handed in from outside is a type, and
//!   `oac_core::transport` would be named to get it, but a closure or trait object
//!   that wraps a transport operation is invisible here. The dynamic checks of the suite
//!   carry that ([IFC-ADP-003], [IFC-ADP-004]).
//! - Re-exports from other crates: if a dependency re-exports a forbidden item under
//!   another path, only that dependency's own scan finds it. Within the files given,
//!   module structure is not modelled: every import counts everywhere, which can only add
//!   findings.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::path::{Component, Path, PathBuf};

use proc_macro2::{Delimiter, Spacing, TokenStream, TokenTree};
use syn::visit::{self, Visit};

/// One forbidden reach found in an adapter source file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    /// The requirement it breaks.
    pub requirement: &'static str,
    /// The file.
    pub file: PathBuf,
    /// The 1-based line, when known.
    pub line: usize,
    /// What was found: the resolved path, or why the file could not be read.
    pub what: String,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}: `{}` ({})",
            self.file.display(),
            self.line,
            self.what,
            self.requirement
        )
    }
}

/// The forbidden path prefixes, with the requirement each breaks.
pub const FORBIDDEN: &[(&str, &[&str])] = &[
    ("IFC-ADP-001", &["oac_core", "transport"]),
    ("IFC-ADP-002", &["oac_core", "signing"]),
    ("IFC-ADP-002", &["oac_core", "keys"]),
    ("IFC-ADP-002", &["oac_core", "trust"]),
    ("IFC-ADP-002", &["oac_core", "canonical"]),
    ("IFC-ADP-002", &["oac_core", "registration"]),
    ("IFC-ADP-002", &["oac_core", "replay"]),
    ("IFC-ADP-002", &["oac_core", "envelope", "EnvelopeDraft"]),
    ("IFC-ADP-002", &["oac_core", "envelope", "receive_envelope"]),
    (
        "IFC-ADP-002",
        &["oac_core", "receipt", "DeliveryReceipt", "new"],
    ),
    (
        "IFC-ADP-002",
        &["oac_core", "presence", "PresenceRecord", "new"],
    ),
    (
        "IFC-ADP-007",
        &["oac_core", "ids", "SessionId", "from_random_octets"],
    ),
    ("IFC-ADP-007", &["oac_core", "authorization"]),
    ("IFC-ADP-007", &["oac_core", "pairing"]),
    (
        "IFC-ADP-013",
        &["oac_core", "adapter", "Connection", "accept"],
    ),
    // The suite's own planted breaches (`plant`): an adapter, its tests included, never
    // reaches them. A row of its own, not a suite requirement, so that the stand-ins that
    // plant them still report each requirement by their behaviour; `tests/real_adapters.rs`
    // fails on any finding, this one too (PR #336 review N6).
    (PLANT, &["oac_contract_adapter", "plant"]),
];

/// The finding for a reach into the suite's planted breaches ([`FORBIDDEN`]).
pub const PLANT: &str = "TEST-PLANT";

/// Method names an adapter must not call, with the requirement each breaks.
pub const FORBIDDEN_METHODS: &[(&str, &str)] = &[("IFC-ADP-002", "sign_envelope")];

/// The trait whose implementation the tripwire looks for.
pub const PROVIDER_ADAPTER: &[&str] = &["oac_core", "adapter", "ProviderAdapter"];

/// The macros that paste in a file the scan does not read (B2 of the PR #336 review).
const INCLUDE_MACROS: &[&str] = &["include", "include_str", "include_bytes"];

/// Rule [IFC-ADP-001] for crate names: a transport crate.
fn is_transport_crate(first: &str) -> bool {
    first.starts_with("oac_transport")
}

// ---- resolution ---------------------------------------------------------------------------

#[derive(Default)]
struct Imports {
    /// name in scope -> the path it stands for
    renames: HashMap<String, Vec<String>>,
    /// prefixes imported with `*`
    globs: Vec<Vec<String>>,
}

impl Imports {
    fn add_tree(&mut self, prefix: &[String], tree: &syn::UseTree) {
        match tree {
            syn::UseTree::Path(p) => {
                let mut next = prefix.to_vec();
                next.push(id(&p.ident));
                self.add_tree(&next, &p.tree);
            }
            syn::UseTree::Name(n) => {
                let name = id(&n.ident);
                if name == "self" {
                    if let Some(last) = prefix.last() {
                        self.renames.insert(last.clone(), prefix.to_vec());
                    }
                } else {
                    let mut path = prefix.to_vec();
                    path.push(name.clone());
                    self.renames.insert(name, path);
                }
            }
            syn::UseTree::Rename(r) => {
                let name = id(&r.ident);
                let mut path = prefix.to_vec();
                if name != "self" {
                    path.push(name);
                }
                self.renames.insert(id(&r.rename), path);
            }
            syn::UseTree::Glob(_) => self.globs.push(prefix.to_vec()),
            syn::UseTree::Group(g) => {
                for t in &g.items {
                    self.add_tree(prefix, t);
                }
            }
        }
    }

    /// The path with its first segment replaced by what it was imported as, repeatedly.
    /// Leading `crate`, `self` and `super` segments are dropped first, and after each
    /// replacement: imports are collected for every file given, whatever its module, so a
    /// relative path is looked up among all of them (which can only add findings).
    fn resolve(&self, path: &[String]) -> Vec<String> {
        let mut p = strip_relative(path).to_vec();
        for _ in 0..16 {
            let Some(first) = p.first() else { break };
            match self.renames.get(first) {
                Some(full) if full.first() != Some(first) || full.len() > 1 => {
                    let mut next = strip_relative(full).to_vec();
                    next.extend_from_slice(&p[1..]);
                    if next == p {
                        break;
                    }
                    p = next;
                }
                _ => break,
            }
        }
        p
    }

    /// Every reading of `path`: the path as written, and each of its suffixes (so a
    /// segment imported anywhere is resolved wherever it stands in the path, as
    /// `crate::module::Name` reaches a `pub use` re-export), each resolved through the
    /// renames and also under each glob prefix.
    fn candidates(&self, path: &[String]) -> Vec<Vec<String>> {
        let path = strip_relative(path);
        let mut out = Vec::new();
        for start in 0..path.len().max(1) {
            let tail = &path[start.min(path.len())..];
            out.push(self.resolve(tail));
            for g in &self.globs {
                let mut p = self.resolve(g);
                p.extend_from_slice(tail);
                out.push(self.resolve(&p));
            }
        }
        out.sort();
        out.dedup();
        out
    }
}

/// An identifier as a string, without a raw `r#` prefix: `r#transport` and `transport`
/// name the same item.
fn id(i: &syn::Ident) -> String {
    syn::ext::IdentExt::unraw(i).to_string()
}

/// `path` without its leading `crate`, `self` and `super` segments.
fn strip_relative(path: &[String]) -> &[String] {
    let n = path
        .iter()
        .take_while(|s| matches!(s.as_str(), "crate" | "self" | "super"))
        .count();
    &path[n..]
}

fn segments(path: &syn::Path) -> Vec<String> {
    path.segments.iter().map(|s| id(&s.ident)).collect()
}

/// Collects imports from anywhere in the file.
struct ImportCollector<'a>(&'a mut Imports);

impl<'ast> Visit<'ast> for ImportCollector<'_> {
    fn visit_item_use(&mut self, u: &'ast syn::ItemUse) {
        self.0.add_tree(&[], &u.tree);
    }
    fn visit_item_extern_crate(&mut self, e: &'ast syn::ItemExternCrate) {
        let name = id(&e.ident);
        let alias = e
            .rename
            .as_ref()
            .map_or_else(|| name.clone(), |(_, r)| id(r));
        self.0.renames.insert(alias, vec![name]);
    }
}

/// Finds forbidden reaches once imports are known.
struct Checker<'a> {
    imports: &'a Imports,
    file: &'a Path,
    findings: Vec<Finding>,
    implements: Vec<Finding>,
    /// Lines of an `impl $t for ..` in a macro's tokens, the trait a metavariable.
    meta_trait_impls: Vec<usize>,
    /// Lines of a macro invocation handed a path that resolves to `ProviderAdapter`.
    adapter_in_args: Vec<usize>,
}

impl Checker<'_> {
    fn check_path(&mut self, raw: &[String], line: usize) {
        for c in self.imports.candidates(raw) {
            for (req, prefix) in FORBIDDEN {
                if c.len() >= prefix.len() && c.iter().zip(prefix.iter()).all(|(a, b)| a == b) {
                    self.push(req, line, c.join("::"));
                }
            }
            if c.first().is_some_and(|f| is_transport_crate(f)) {
                self.push("IFC-ADP-001", line, c.join("::"));
            }
        }
    }

    /// A macro path that names `include!`, `include_str!` or `include_bytes!`, as written
    /// (`::core::include`) or through an alias.
    fn is_include(&self, raw: &[String]) -> bool {
        raw.last()
            .is_some_and(|l| INCLUDE_MACROS.contains(&l.as_str()))
            || self.imports.candidates(raw).iter().any(|c| {
                c.last()
                    .is_some_and(|l| INCLUDE_MACROS.contains(&l.as_str()))
            })
    }

    fn is_provider_adapter(&self, raw: &[String]) -> bool {
        self.imports
            .candidates(raw)
            .iter()
            .any(|c| c == PROVIDER_ADAPTER)
    }

    fn push(&mut self, requirement: &'static str, line: usize, what: String) {
        let f = Finding {
            requirement,
            file: self.file.to_path_buf(),
            line,
            what,
        };
        if !self.findings.contains(&f) {
            self.findings.push(f);
        }
    }

    fn implementation(&mut self, line: usize, what: String) {
        let f = Finding {
            requirement: "IFC-ADP-010",
            file: self.file.to_path_buf(),
            line,
            what,
        };
        if !self.implements.contains(&f) {
            self.implements.push(f);
        }
    }

    /// Code the scan cannot read: a finding under every requirement, so it fails closed.
    fn fail_closed(&mut self, line: usize, what: &str) {
        // As for a file that does not parse: every row in both lists.
        for f in everywhere(self.file, line, what) {
            if !self.implements.contains(&f) {
                self.implements.push(f.clone());
            }
            if !self.findings.contains(&f) {
                self.findings.push(f);
            }
        }
    }

    /// A macro's or an attribute's tokens: walked for paths, `impl`s and hazards, and
    /// matched as text for the forbidden paths written in full (string literals included).
    fn check_tokens(&mut self, tokens: &TokenStream, line: usize, invocation: bool) {
        self.check_text(&tokens.to_string(), line);
        self.walk(tokens.clone(), invocation);
    }

    fn check_text(&mut self, tokens: &str, line: usize) {
        let text: String = tokens
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .replace("r#", "");
        let mut prefixes: Vec<(String, Vec<String>)> =
            vec![("oac_core".into(), vec!["oac_core".into()])];
        for (name, full) in &self.imports.renames {
            prefixes.push((name.clone(), full.clone()));
        }
        for (name, full) in prefixes {
            for (req, prefix) in FORBIDDEN {
                // The forbidden path written from `name`: the part of the prefix beyond
                // what `name` stands for.
                if full.len() <= prefix.len() && full.iter().zip(prefix.iter()).all(|(a, b)| a == b)
                {
                    let rest = &prefix[full.len()..];
                    let written = std::iter::once(name.as_str())
                        .chain(rest.iter().copied())
                        .collect::<Vec<_>>()
                        .join("::");
                    if text.contains(&written) {
                        self.push(req, line, format!("{written} (in a macro)"));
                    }
                }
            }
        }
        if text.contains("oac_transport") {
            self.push("IFC-ADP-001", line, "oac_transport_* (in a macro)".into());
        }
    }

    /// One level of a token stream; groups are walked in turn.
    fn walk(&mut self, ts: TokenStream, invocation: bool) {
        let toks: Vec<TokenTree> = ts.into_iter().collect();
        let mut i = 0;
        while i < toks.len() {
            match &toks[i] {
                TokenTree::Group(g) => {
                    let attribute = g.delimiter() == Delimiter::Bracket
                        && (is_punct(toks.get(i.wrapping_sub(1)), '#')
                            || (is_punct(toks.get(i.wrapping_sub(1)), '!')
                                && is_punct(toks.get(i.wrapping_sub(2)), '#')));
                    if attribute && attribute_names_a_path(&g.stream()) {
                        self.fail_closed(
                            line_of(g.span()),
                            "#[path] inside a macro's tokens: a module the scan cannot follow",
                        );
                    }
                    self.walk(g.stream(), invocation);
                    i += 1;
                }
                // An out-of-line `mod m;` or `mod $m;`: its file depends on where the macro
                // expands (B3 of the PR #336 review).
                TokenTree::Ident(id) if id == "mod" && out_of_line_mod(&toks, i + 1) => {
                    self.fail_closed(
                        line_of(id.span()),
                        "an out-of-line `mod ..;` inside a macro's tokens: a module the scan cannot follow",
                    );
                    i += 1;
                }
                TokenTree::Ident(id) if id == "impl" => {
                    self.macro_impl(&toks, i);
                    i += 1;
                }
                _ => match path_at(&toks, i) {
                    Some((segs, end, line)) => {
                        if is_punct(toks.get(end), '!') && self.is_include(&segs) {
                            self.fail_closed(
                                line,
                                "an include macro inside a macro's tokens: a file the scan cannot read",
                            );
                        }
                        self.check_path(&segs, line);
                        if invocation && self.is_provider_adapter(&segs) {
                            self.adapter_in_args.push(line);
                        }
                        i = end;
                    }
                    None => i += 1,
                },
            }
        }
    }

    /// `impl [<..>] [!] Trait [<..>] for` at `toks[i]` (the `impl`).
    fn macro_impl(&mut self, toks: &[TokenTree], i: usize) {
        let line = line_of(toks[i].span());
        let mut j = skip_generics(toks, i + 1);
        if is_punct(toks.get(j), '!') {
            j += 1;
        }
        // A metavariable trait: `impl $t for ..`.
        if is_punct(toks.get(j), '$')
            && matches!(toks.get(j + 1), Some(TokenTree::Ident(v)) if v != "crate")
        {
            let k = skip_generics(toks, j + 2);
            if matches!(toks.get(k), Some(TokenTree::Ident(f)) if f == "for") {
                self.meta_trait_impls.push(line);
            }
            return;
        }
        let Some((segs, end, _)) = path_at(toks, j) else {
            return;
        };
        let k = skip_generics(toks, end);
        if matches!(toks.get(k), Some(TokenTree::Ident(f)) if f == "for")
            && self.is_provider_adapter(&segs)
        {
            self.implementation(
                line,
                format!("impl {} for .. (in a macro)", segs.join("::")),
            );
        }
    }
}

fn is_punct(t: Option<&TokenTree>, c: char) -> bool {
    matches!(t, Some(TokenTree::Punct(p)) if p.as_char() == c)
}

/// The two-token `::` at `toks[i]`.
fn is_path_sep(toks: &[TokenTree], i: usize) -> bool {
    matches!(toks.get(i), Some(TokenTree::Punct(p)) if p.as_char() == ':' && p.spacing() == Spacing::Joint)
        && is_punct(toks.get(i + 1), ':')
}

/// One path segment at `toks[i]`: an identifier (without `r#`), or `$crate` read as
/// `crate`. Returns the segment and the index after it.
fn segment_at(toks: &[TokenTree], i: usize) -> Option<(String, usize)> {
    match toks.get(i)? {
        TokenTree::Ident(id) => Some((id.to_string().trim_start_matches("r#").to_owned(), i + 1)),
        TokenTree::Punct(p) if p.as_char() == '$' => match toks.get(i + 1)? {
            TokenTree::Ident(id) if id == "crate" => Some(("crate".to_owned(), i + 2)),
            _ => None,
        },
        _ => None,
    }
}

/// A run of `::`-joined segments at `toks[i]`, with an optional leading `::`: the
/// segments, the index after the run and the line of its first token.
fn path_at(toks: &[TokenTree], i: usize) -> Option<(Vec<String>, usize, usize)> {
    let line = line_of(toks.get(i)?.span());
    let mut j = i;
    if is_path_sep(toks, j) {
        j += 2;
    }
    let (first, mut j) = segment_at(toks, j)?;
    let mut segs = vec![first];
    while is_path_sep(toks, j) {
        match segment_at(toks, j + 2) {
            Some((s, next)) => {
                segs.push(s);
                j = next;
            }
            None => break,
        }
    }
    Some((segs, j, line))
}

/// The index after a `<..>` at `toks[i]`, or `i` when there is none. A `>` after `-` (an
/// `->` in a bound) does not close it.
fn skip_generics(toks: &[TokenTree], i: usize) -> usize {
    if !is_punct(toks.get(i), '<') {
        return i;
    }
    let mut depth = 0usize;
    let mut j = i;
    while j < toks.len() {
        if is_punct(toks.get(j), '<') {
            depth += 1;
        } else if is_punct(toks.get(j), '>') && !is_punct(toks.get(j.wrapping_sub(1)), '-') {
            depth -= 1;
            if depth == 0 {
                return j + 1;
            }
        }
        j += 1;
    }
    j
}

/// An attribute's tokens (inside `#[..]`) that set a module path: `path = ..`, or a
/// `cfg_attr(.., path = ..)`.
fn attribute_names_a_path(ts: &TokenStream) -> bool {
    let toks: Vec<TokenTree> = ts.clone().into_iter().collect();
    match toks.first() {
        Some(TokenTree::Ident(i)) if i == "path" => is_punct(toks.get(1), '='),
        Some(TokenTree::Ident(i)) if i == "cfg_attr" => match toks.get(1) {
            Some(TokenTree::Group(g)) => names_path(&g.stream()),
            _ => true,
        },
        _ => false,
    }
}

/// A `path = ..` anywhere in a token list, at any depth (a `cfg_attr` nested in a
/// `cfg_attr` included; B1 of the PR #336 review).
fn names_path(ts: &TokenStream) -> bool {
    let toks: Vec<TokenTree> = ts.clone().into_iter().collect();
    toks.windows(2)
        .any(|w| matches!(&w[0], TokenTree::Ident(i) if i == "path") && is_punct(Some(&w[1]), '='))
        || toks.iter().any(|t| match t {
            TokenTree::Group(g) => names_path(&g.stream()),
            _ => false,
        })
}

/// At `toks[i]` (after a `mod`): a name or `$name`, then `;`.
fn out_of_line_mod(toks: &[TokenTree], i: usize) -> bool {
    match toks.get(i) {
        Some(TokenTree::Ident(_)) => is_punct(toks.get(i + 1), ';'),
        Some(TokenTree::Punct(p)) if p.as_char() == '$' => {
            matches!(toks.get(i + 1), Some(TokenTree::Ident(_))) && is_punct(toks.get(i + 2), ';')
        }
        _ => false,
    }
}

fn line_of(span: proc_macro2::Span) -> usize {
    span.start().line
}

impl<'ast> Visit<'ast> for Checker<'_> {
    fn visit_item_use(&mut self, u: &'ast syn::ItemUse) {
        let mut flat = Imports::default();
        flat.add_tree(&[], &u.tree);
        let line = line_of(u.use_token.span);
        let mut paths: Vec<Vec<String>> = flat.renames.into_values().collect();
        paths.extend(flat.globs);
        for p in paths {
            // An include macro under a name of the file's choosing (`use std::include as
            // inc;`): fail closed on the import itself (B2 of the PR #336 review).
            if p.last()
                .is_some_and(|l| INCLUDE_MACROS.contains(&l.as_str()))
            {
                self.fail_closed(
                    line,
                    "an include macro imported: a file the scan cannot read",
                );
            }
            self.check_path(&p, line);
        }
    }

    fn visit_item_extern_crate(&mut self, e: &'ast syn::ItemExternCrate) {
        let line = line_of(e.ident.span());
        self.check_path(&[id(&e.ident)], line);
    }

    fn visit_path(&mut self, p: &'ast syn::Path) {
        let line = p.segments.first().map_or(0, |s| line_of(s.ident.span()));
        self.check_path(&segments(p), line);
        visit::visit_path(self, p);
    }

    fn visit_expr_method_call(&mut self, m: &'ast syn::ExprMethodCall) {
        let name = id(&m.method);
        for (req, method) in FORBIDDEN_METHODS {
            if name == *method {
                self.push(req, line_of(m.method.span()), format!(".{name}(..)"));
            }
        }
        visit::visit_expr_method_call(self, m);
    }

    fn visit_attribute(&mut self, a: &'ast syn::Attribute) {
        // The attribute's own path is a path like any other; a token list (`derive(..)`,
        // a tool attribute) is walked as a macro's tokens are.
        let line = a
            .path()
            .segments
            .first()
            .map_or(0, |s| line_of(s.ident.span()));
        if let syn::Meta::List(list) = &a.meta {
            self.check_tokens(&list.tokens, line, false);
        }
        visit::visit_attribute(self, a);
    }

    fn visit_macro(&mut self, m: &'ast syn::Macro) {
        let line = m
            .path
            .segments
            .first()
            .map_or(0, |s| line_of(s.ident.span()));
        let name = m.path.segments.last().map(|s| id(&s.ident));
        if self.is_include(&segments(&m.path)) {
            self.fail_closed(line, "an include macro: a file the scan cannot read");
        }
        let invocation = name.as_deref() != Some("macro_rules");
        self.check_tokens(&m.tokens, line, invocation);
        visit::visit_macro(self, m);
    }

    fn visit_item_impl(&mut self, i: &'ast syn::ItemImpl) {
        if let Some((_, path, _)) = &i.trait_ {
            let line = path.segments.first().map_or(0, |s| line_of(s.ident.span()));
            if self.is_provider_adapter(&segments(path)) {
                self.implementation(line, format!("impl {} for ..", segments(path).join("::")));
            }
        }
        visit::visit_item_impl(self, i);
    }
}

/// A finding under every requirement, so that a file the scan cannot read fails closed.
fn everywhere(file: &Path, line: usize, what: &str) -> Vec<Finding> {
    [
        "IFC-ADP-001",
        "IFC-ADP-002",
        "IFC-ADP-007",
        "IFC-ADP-013",
        "IFC-ADP-010",
    ]
    .into_iter()
    .map(|requirement| Finding {
        requirement,
        file: file.to_path_buf(),
        line,
        what: what.to_owned(),
    })
    .collect()
}

// ---- following `mod` declarations ---------------------------------------------------------

/// `path` with `.` dropped and `..` applied where it can be, without touching the disk.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(out.components().next_back(), Some(Component::Normal(_))) {
                    out.pop();
                } else {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// One out-of-line `mod` declaration: the files it may load. A `#[path]` one must load one.
struct ModDecl {
    candidates: Vec<PathBuf>,
    required: bool,
    line: usize,
    what: String,
}

/// The `#[path = ".."]` value among `attrs`; `Err` when there is a module path the scan
/// cannot resolve (not a string literal, or inside a `cfg_attr`).
fn path_attribute(attrs: &[syn::Attribute]) -> Result<Option<String>, &'static str> {
    let mut found = None;
    for a in attrs {
        if a.path().is_ident("path") {
            match &a.meta {
                syn::Meta::NameValue(syn::MetaNameValue {
                    value:
                        syn::Expr::Lit(syn::ExprLit {
                            lit: syn::Lit::Str(s),
                            ..
                        }),
                    ..
                }) => found = Some(s.value()),
                _ => return Err("a #[path] that is not a string literal"),
            }
        } else if a.path().is_ident("cfg_attr")
            && let syn::Meta::List(l) = &a.meta
            && names_path(&l.tokens)
        {
            return Err("a #[cfg_attr(.., path = ..)] the scan cannot resolve");
        }
    }
    Ok(found)
}

/// Collects the `mod` declarations of one file, as rustc resolves them: relative to the
/// file's directory, through the inline modules around them (for a file other than
/// `lib.rs`, `main.rs` or `mod.rs`, under a directory named for the file too). Both readings
/// are tried for every file, since a file reached through `#[path]` may be read either way;
/// an extra candidate that exists can only add findings.
struct ModFollower<'a> {
    file: &'a Path,
    inline: Vec<String>,
    decls: Vec<ModDecl>,
    unresolvable: Vec<(usize, String)>,
}

impl ModFollower<'_> {
    fn bases(&self) -> Vec<PathBuf> {
        let dir = self.file.parent().unwrap_or_else(|| Path::new(""));
        let stem = self.file.file_stem().unwrap_or_default();
        [dir.to_path_buf(), dir.join(stem)]
            .into_iter()
            .map(|mut b| {
                b.extend(&self.inline);
                b
            })
            .collect()
    }
}

impl<'ast> Visit<'ast> for ModFollower<'_> {
    fn visit_item_mod(&mut self, m: &'ast syn::ItemMod) {
        let name = id(&m.ident);
        let line = line_of(m.ident.span());
        let attr = path_attribute(&m.attrs).unwrap_or_else(|why| {
            self.unresolvable.push((line, format!("mod {name}: {why}")));
            None
        });
        if m.content.is_some() {
            self.inline.push(attr.unwrap_or(name));
            visit::visit_item_mod(self, m);
            self.inline.pop();
            return;
        }
        let decl = match attr {
            Some(p) => ModDecl {
                candidates: if self.inline.is_empty() {
                    let dir = self.file.parent().unwrap_or_else(|| Path::new(""));
                    vec![dir.join(&p)]
                } else {
                    self.bases().into_iter().map(|b| b.join(&p)).collect()
                },
                required: true,
                line,
                what: format!("#[path = {p:?}] mod {name}"),
            },
            None => ModDecl {
                candidates: self
                    .bases()
                    .into_iter()
                    .flat_map(|b| [b.join(format!("{name}.rs")), b.join(&name).join("mod.rs")])
                    .collect(),
                // A missing file is a build error, or a module compiled out.
                required: false,
                line,
                what: format!("mod {name}"),
            },
        };
        self.decls.push(decl);
    }
}

/// Where file texts come from: `None` when there is no such file.
type Load<'a> = dyn Fn(&Path) -> Option<Result<String, String>> + 'a;

/// Is `path` itself a symlink (not followed)?
type IsSymlink<'a> = dyn Fn(&Path) -> bool + 'a;

fn is_symlink_on_disk(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
}

/// A file's text from disk. A symlink is never read: the scan resolves paths lexically and
/// rustc physically, so the two could name different files (PR #336 review N7).
fn load_from_disk(path: &Path) -> Option<Result<String, String>> {
    if is_symlink_on_disk(path) {
        return Some(Err(
            "a symlink: the scan resolves paths lexically, so it does not follow one".into(),
        ));
    }
    path.is_file()
        .then(|| std::fs::read_to_string(path).map_err(|e| e.to_string()))
}

/// The first symlink among the components of `candidate` beyond `dir` (the declaring
/// file's directory), before `..` is applied: `#[path = "l/../x.rs"]` with `l` a symlink
/// names a file beside `l`'s target, not `x.rs` beside `l`.
fn symlink_on_the_way(dir: &Path, candidate: &Path, is_symlink: &IsSymlink<'_>) -> Option<PathBuf> {
    let base = dir.components().count();
    let mut prefix = PathBuf::new();
    for (n, c) in candidate.components().enumerate() {
        prefix.push(c.as_os_str());
        if n >= base && matches!(c, Component::Normal(_)) && is_symlink(&prefix) {
            return Some(prefix);
        }
    }
    None
}

/// Both scans over `roots` and every file their `mod` declarations load, read as one
/// crate: imports and `pub use` re-exports of every file are collected first, then every
/// file is checked against all of them. A file that cannot be read or does not parse, a
/// `#[path]` that cannot be resolved and an `include!` are findings under every
/// requirement, so they fail closed.
fn analyse_set(
    roots: &[PathBuf],
    load: &Load<'_>,
    is_symlink: &IsSymlink<'_>,
) -> (Vec<Finding>, Vec<Finding>) {
    let mut imports = Imports::default();
    let mut parsed = Vec::new();
    let (mut findings, mut implements) = (Vec::new(), Vec::new());
    let mut fail = |all: Vec<Finding>| {
        findings.extend(all.clone());
        implements.extend(all);
    };
    let mut queue: VecDeque<PathBuf> = roots.iter().map(|p| normalize(p)).collect();
    let mut seen = HashSet::new();
    while let Some(path) = queue.pop_front() {
        if !seen.insert(path.clone()) {
            continue;
        }
        let text = load(&path).unwrap_or_else(|| Err("no such file".to_owned()));
        match text.as_ref().map(|t| syn::parse_file(t)) {
            Ok(Ok(ast)) => {
                ImportCollector(&mut imports).visit_file(&ast);
                let mut mods = ModFollower {
                    file: &path,
                    inline: Vec::new(),
                    decls: Vec::new(),
                    unresolvable: Vec::new(),
                };
                mods.visit_file(&ast);
                for (line, why) in mods.unresolvable {
                    fail(everywhere(&path, line, &why));
                }
                let dir = path.parent().unwrap_or_else(|| Path::new("")).to_path_buf();
                for d in mods.decls {
                    if let Some(link) = d
                        .candidates
                        .iter()
                        .find_map(|c| symlink_on_the_way(&dir, c, is_symlink))
                    {
                        let why = format!(
                            "{}: {} is a symlink, and the scan resolves paths lexically",
                            d.what,
                            link.display()
                        );
                        fail(everywhere(&path, d.line, &why));
                        continue;
                    }
                    let found: Vec<PathBuf> = d
                        .candidates
                        .iter()
                        .map(|c| normalize(c))
                        .filter(|c| seen.contains(c) || load(c).is_some())
                        .collect();
                    if d.required && found.is_empty() {
                        let why = format!("{}: names no file the scan can read", d.what);
                        fail(everywhere(&path, d.line, &why));
                    }
                    queue.extend(found);
                }
                parsed.push((path, ast));
            }
            Ok(Err(e)) => fail(everywhere(
                &path,
                line_of(e.span()),
                &format!("does not parse: {e}"),
            )),
            Err(why) => fail(everywhere(&path, 0, &format!("cannot be read: {why}"))),
        }
    }
    let (mut meta_trait_impls, mut adapter_in_args) = (Vec::new(), Vec::new());
    for (path, ast) in &parsed {
        let mut c = Checker {
            imports: &imports,
            file: path,
            findings: Vec::new(),
            implements: Vec::new(),
            meta_trait_impls: Vec::new(),
            adapter_in_args: Vec::new(),
        };
        c.visit_file(ast);
        findings.extend(c.findings);
        implements.extend(c.implements);
        meta_trait_impls.extend(c.meta_trait_impls.into_iter().map(|l| (path.clone(), l)));
        adapter_in_args.extend(c.adapter_in_args.into_iter().map(|l| (path.clone(), l)));
    }
    // A macro that implements a trait it is handed, handed `ProviderAdapter`.
    if !meta_trait_impls.is_empty() {
        for (file, line) in adapter_in_args {
            implements.push(Finding {
                requirement: "IFC-ADP-010",
                file,
                line,
                what: "ProviderAdapter handed to a macro, where a macro implements a trait \
                       it is handed (impl $t for ..)"
                    .into(),
            });
        }
    }
    (findings, implements)
}

/// A loader over in-memory files, keyed by normalized path.
fn in_memory<'a>(
    files: &'a [(&str, &str)],
) -> impl Fn(&Path) -> Option<Result<String, String>> + 'a {
    move |p: &Path| {
        files
            .iter()
            .find(|(f, _)| normalize(Path::new(f)) == p)
            .map(|(_, t)| Ok((*t).to_owned()))
    }
}

/// Both scans of one file's text: forbidden reaches, and `ProviderAdapter` implementations.
pub fn analyse(file: &Path, text: &str) -> (Vec<Finding>, Vec<Finding>) {
    let name = file.to_string_lossy();
    analyse_files(&[name.as_ref()], &[(name.as_ref(), text)])
}

/// Both scans of several files' texts, read as one crate ([`scan`]).
pub fn analyse_texts(files: &[(&str, &str)]) -> (Vec<Finding>, Vec<Finding>) {
    let roots: Vec<&str> = files.iter().map(|(p, _)| *p).collect();
    analyse_files(&roots, files)
}

/// Both scans from `roots`, with `files` the in-memory file system the `mod` declarations
/// are followed through (a file not in `files` does not exist).
pub fn analyse_files(roots: &[&str], files: &[(&str, &str)]) -> (Vec<Finding>, Vec<Finding>) {
    analyse_files_with_symlinks(roots, files, &[])
}

/// [`analyse_files`], with `symlinks` the paths of the in-memory file system that are
/// symlinks.
pub fn analyse_files_with_symlinks(
    roots: &[&str],
    files: &[(&str, &str)],
    symlinks: &[&str],
) -> (Vec<Finding>, Vec<Finding>) {
    let roots: Vec<PathBuf> = roots.iter().map(PathBuf::from).collect();
    let links: Vec<PathBuf> = symlinks.iter().map(|s| normalize(Path::new(s))).collect();
    analyse_set(&roots, &in_memory(files), &|p: &Path| {
        links.contains(&p.to_path_buf())
    })
}

/// The forbidden reaches in one file's text.
pub fn scan_text(file: &Path, text: &str) -> Vec<Finding> {
    analyse(file, text).0
}

/// Scan `files`, and every file their `mod` declarations load, read as one crate (imports
/// and re-exports of every file apply to all).
pub fn scan(files: &[PathBuf]) -> Vec<Finding> {
    analyse_set(files, &load_from_disk, &is_symlink_on_disk).0
}

/// Every `impl` of `oac_core::adapter::ProviderAdapter` in `files` and the files their
/// `mod` declarations load, read as one crate, under any alias or re-export.
pub fn implements_provider_adapter(files: &[PathBuf]) -> Vec<Finding> {
    analyse_set(files, &load_from_disk, &is_symlink_on_disk).1
}

/// Every `.rs` file under `dir`, recursively, in path order. A symlink is listed whatever
/// it names and is not followed, so that reading it fails closed (PR #336 review N7).
pub fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut v = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if e.file_type().is_ok_and(|t| t.is_symlink()) {
                v.push(p);
            } else if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") {
                v.push(p);
            }
        }
    }
    v.sort();
    v
}

/// An adapter crate's sources for the scan: everything under `src/`, and `build.rs` when
/// there is one. The scan also follows their `mod` declarations, `#[path]` ones included.
pub fn crate_files(crate_dir: &Path) -> Vec<PathBuf> {
    let mut v = rust_files(&crate_dir.join("src"));
    let build = crate_dir.join("build.rs");
    if build.is_file() {
        v.push(build);
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reqs(text: &str) -> Vec<&'static str> {
        let mut v: Vec<_> = scan_text(Path::new("x.rs"), text)
            .into_iter()
            .map(|f| f.requirement)
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    fn implements(text: &str) -> bool {
        !analyse(Path::new("x.rs"), text).1.is_empty()
    }

    #[test]
    fn clean_code_and_comments_pass() {
        let clean = "//! routes through oac_core::transport? never.\n/* EnvelopeDraft */\n\
            use oac_core::adapter::{HandOff, ProviderAdapter as _};\n\
            use oac_core::ids::SessionId;\n\
            fn f(s: &str) -> Option<SessionId> { let _ = \"oac_core::transport\"; SessionId::parse(s) }\n";
        assert_eq!(reqs(clean), Vec::<&str>::new());
    }

    // Each evasion listed in the PR #323 review (B1), and the plain forms.
    #[test]
    fn flat_import() {
        assert_eq!(reqs("use oac_core::transport::Transport;"), ["IFC-ADP-001"]);
    }

    #[test]
    fn grouped_import() {
        assert_eq!(
            reqs("use oac_core::{adapter::HandOff, transport::Transport};"),
            ["IFC-ADP-001"]
        );
    }

    #[test]
    fn multi_line_grouped_import() {
        assert_eq!(
            reqs("use oac_core::{\n    transport::{Destination, Transport},\n};"),
            ["IFC-ADP-001"]
        );
    }

    #[test]
    fn crate_alias() {
        assert_eq!(
            reqs("use oac_core as c;\nfn f() { let _: Option<c::transport::Payload> = None; }"),
            ["IFC-ADP-001"]
        );
        assert_eq!(
            reqs(
                "extern crate oac_core as c;\nfn f() { let _: Option<c::transport::Payload> = None; }"
            ),
            ["IFC-ADP-001"]
        );
    }

    #[test]
    fn whitespace_split_path() {
        assert_eq!(
            reqs("use oac_core :: transport :: Transport;"),
            ["IFC-ADP-001"]
        );
        assert_eq!(
            reqs("fn f() { let _: Option<oac_core\n  ::transport\n  ::Payload> = None; }"),
            ["IFC-ADP-001"]
        );
    }

    #[test]
    fn module_import_then_impl() {
        assert_eq!(
            reqs("use oac_core::{transport};\nstruct X;\nimpl transport::Transport for X {}"),
            ["IFC-ADP-001"]
        );
    }

    #[test]
    fn signing_and_keys() {
        assert_eq!(
            reqs("use oac_core::{signing::authenticate, keys::DeviceKey};"),
            ["IFC-ADP-002"]
        );
        assert_eq!(
            reqs("fn f(i: &I, d: D) { i.sign_envelope(d); }"),
            ["IFC-ADP-002"]
        );
        assert_eq!(
            reqs("use oac_core::envelope::EnvelopeDraft as Draft;"),
            ["IFC-ADP-002"]
        );
    }

    #[test]
    fn minted_session_id_through_an_alias() {
        assert_eq!(
            reqs(
                "use oac_core::ids::SessionId as S;\nfn f() { let _ = S::from_random_octets([0; 16]); }"
            ),
            ["IFC-ADP-007"]
        );
        assert_eq!(reqs("use oac_core::authorization::*;"), ["IFC-ADP-007"]);
    }

    #[test]
    fn connection_accept() {
        assert_eq!(
            reqs(
                "use oac_core::adapter::Connection;\nfn f(r: R, w: W) { let _ = Connection::accept(r, w); }"
            ),
            ["IFC-ADP-013"]
        );
        assert_eq!(
            reqs(
                "use oac_core::adapter::{Connection as K};\nfn f(r: R, w: W) { let _ = K::accept(r, w); }"
            ),
            ["IFC-ADP-013"]
        );
    }

    #[test]
    fn chained_and_glob_imports() {
        assert_eq!(
            reqs("use oac_core as c;\nuse c::transport as t;\nfn f(_: &dyn t::Transport) {}"),
            ["IFC-ADP-001"]
        );
        assert_eq!(
            reqs("use oac_core::*;\nfn f(_: &dyn transport::Transport) {}"),
            ["IFC-ADP-001"]
        );
        assert_eq!(
            reqs("use oac_transport_memory::MemoryTransport;"),
            ["IFC-ADP-001"]
        );
    }

    #[test]
    fn paths_inside_a_macro() {
        assert_eq!(
            reqs("fn f() { m!(oac_core :: transport :: Payload); }"),
            ["IFC-ADP-001"]
        );
        assert_eq!(
            reqs("use oac_core as c;\nfn f() { m!(c::signing::authenticate); }"),
            ["IFC-ADP-002"]
        );
    }

    #[test]
    fn unparseable_fails_closed() {
        assert!(reqs("fn (").contains(&"IFC-ADP-001"));
    }

    #[test]
    fn the_tripwire_sees_every_spelling_of_the_trait() {
        assert!(implements(
            "struct P;\nimpl oac_core::adapter::ProviderAdapter for P {}"
        ));
        assert!(implements(
            "use oac_core::adapter::ProviderAdapter as PA;\nstruct P;\nimpl PA for P {}"
        ));
        assert!(implements(
            "use oac_core::adapter::{self as a};\nstruct P;\nimpl a::ProviderAdapter for P {}"
        ));
        assert!(implements(
            "use oac_core as c;\nstruct P;\nimpl c::adapter::ProviderAdapter for P {}"
        ));
        assert!(implements(
            "use oac_core::adapter::*;\nstruct P;\nimpl ProviderAdapter for P {}"
        ));
        assert!(implements(
            "mod m { use oac_core::adapter::ProviderAdapter; struct P; impl ProviderAdapter for P {} }"
        ));
        assert!(!implements("struct P;\nimpl Other for P {}\nimpl P {}"));
    }

    // The PR #323 re-review probes (B2).
    #[test]
    fn raw_identifiers() {
        assert_eq!(
            reqs("use oac_core::r#transport::Transport;"),
            ["IFC-ADP-001"]
        );
        assert_eq!(
            reqs("fn f(_: &dyn oac_core::r#transport::Transport) {}"),
            ["IFC-ADP-001"]
        );
        assert_eq!(reqs("use r#oac_core::transport::Payload;"), ["IFC-ADP-001"]);
        assert!(implements(
            "struct P;\nimpl oac_core::adapter::r#ProviderAdapter for P {}"
        ));
        assert_eq!(
            reqs(
                "use oac_core::adapter::Connection;\nfn f(r: R, w: W) { let _ = Connection::r#accept(r, w); }"
            ),
            ["IFC-ADP-013"]
        );
        assert_eq!(
            reqs("fn f(i: &I, d: D) { i.r#sign_envelope(d); }"),
            ["IFC-ADP-002"]
        );
        assert_eq!(
            reqs("fn f() { m!(oac_core::r#transport::Payload); }"),
            ["IFC-ADP-001"]
        );
    }

    #[test]
    fn crate_self_and_super_paths() {
        for rel in ["crate", "self", "super"] {
            assert_eq!(
                reqs(&format!(
                    "pub use oac_core::*;\nfn f(_: &dyn {rel}::transport::Transport) {{}}"
                )),
                ["IFC-ADP-001"],
                "{rel}"
            );
            assert!(
                implements(&format!(
                    "pub use oac_core::adapter::ProviderAdapter;\nstruct P;\nimpl {rel}::ProviderAdapter for P {{}}"
                )),
                "{rel}"
            );
        }
        assert!(implements(
            "mod m { pub use oac_core::adapter::ProviderAdapter as Port; }\nstruct P;\nimpl crate::m::Port for P {}"
        ));
    }

    #[test]
    fn re_exports_across_files() {
        let (findings, impls) = analyse_texts(&[
            (
                "lib.rs",
                "pub use oac_core::adapter::ProviderAdapter as Port;\npub use oac_core::*;\nmod other;",
            ),
            (
                "other.rs",
                "struct P;\nimpl crate::Port for P {}\nfn f(_: &dyn crate::transport::Transport) {}",
            ),
        ]);
        assert_eq!(impls.len(), 1, "{impls:?}");
        assert!(impls[0].file.ends_with("other.rs"));
        assert!(
            findings
                .iter()
                .any(|f| f.requirement == "IFC-ADP-001" && f.file.ends_with("other.rs")),
            "{findings:?}"
        );
    }

    #[test]
    fn the_review_plant() {
        // The 4-line example planted in adapters/codex/src/lib.rs by the re-review.
        let plant = "pub use oac_core::adapter::ProviderAdapter;\npub struct Planted;\nimpl crate::ProviderAdapter for Planted {}\npub fn f(_: &dyn oac_core::r#transport::Transport) {}";
        assert!(implements(plant));
        assert_eq!(reqs(plant), ["IFC-ADP-001"]);
    }

    #[test]
    fn paths_in_attributes() {
        assert_eq!(
            reqs("#[derive(Debug, oac_core::transport::X)]\nstruct S;"),
            ["IFC-ADP-001"]
        );
        assert_eq!(
            reqs("use oac_core as c;\n#[tool(c::signing::x)]\nfn f() {}"),
            ["IFC-ADP-002"]
        );
    }

    // ---- #324: the PR #323 third-review follow-ups ------------------------------------

    /// Every row, as a file that cannot be read gives (`everywhere`).
    const EVERY: [&str; 5] = [
        "IFC-ADP-001",
        "IFC-ADP-002",
        "IFC-ADP-007",
        "IFC-ADP-010",
        "IFC-ADP-013",
    ];

    fn reqs_of(findings: &[Finding]) -> Vec<&'static str> {
        let mut v: Vec<_> = findings.iter().map(|f| f.requirement).collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    fn fails_closed(text: &str) {
        let (findings, impls) = analyse(Path::new("x.rs"), text);
        assert_eq!(reqs_of(&findings), EVERY, "{text}: {findings:?}");
        assert!(!impls.is_empty(), "{text}");
    }

    const HIDDEN: &str = "pub struct Q;\nimpl oac_core::adapter::ProviderAdapter for Q {}\n\
        pub fn f(_: &dyn oac_core::transport::Transport) {}";

    /// The review's plant: a module outside `src/`, wired in with `#[path]`.
    #[test]
    fn a_path_module_outside_src_is_followed() {
        let (findings, impls) = analyse_files(
            &["a/src/lib.rs"],
            &[
                ("a/src/lib.rs", "#[path = \"../zzhidden/h.rs\"]\nmod h;"),
                ("a/zzhidden/h.rs", HIDDEN),
            ],
        );
        assert_eq!(impls.len(), 1, "{impls:?}");
        assert!(impls[0].file.ends_with("zzhidden/h.rs"), "{impls:?}");
        assert_eq!(reqs_of(&findings), ["IFC-ADP-001"]);
        assert!(findings[0].file.ends_with("zzhidden/h.rs"));
        // Without the file in place the same declaration fails closed.
        let (findings, impls) = analyse_files(
            &["a/src/lib.rs"],
            &[("a/src/lib.rs", "#[path = \"../zzhidden/h.rs\"]\nmod h;")],
        );
        assert_eq!(reqs_of(&findings), EVERY);
        assert!(!impls.is_empty());
    }

    #[test]
    fn path_modules_inside_inline_modules_and_their_own_modules_are_followed() {
        // In lib.rs (a mod-rs file) the inline module is a directory.
        let (_, impls) = analyse_files(
            &["src/lib.rs"],
            &[
                (
                    "src/lib.rs",
                    "mod m {\n    #[path = \"x.rs\"]\n    mod y;\n}",
                ),
                ("src/m/x.rs", HIDDEN),
            ],
        );
        assert_eq!(impls.len(), 1, "{impls:?}");
        // In a.rs, under a directory named for the file.
        let (_, impls) = analyse_files(
            &["src/a.rs"],
            &[
                ("src/a.rs", "mod b {\n    #[path = \"x.rs\"]\n    mod y;\n}"),
                ("src/a/b/x.rs", HIDDEN),
            ],
        );
        assert_eq!(impls.len(), 1, "{impls:?}");
        // A plain `mod` in a file reached through `#[path]`, both readings of its directory.
        for inner in ["out/inner.rs", "out/h/inner.rs"] {
            let (findings, impls) = analyse_files(
                &["src/lib.rs"],
                &[
                    ("src/lib.rs", "#[path = \"../out/h.rs\"]\nmod h;"),
                    ("out/h.rs", "mod inner;"),
                    (inner, HIDDEN),
                ],
            );
            assert_eq!(impls.len(), 1, "{inner}: {impls:?}");
            assert_eq!(reqs_of(&findings), ["IFC-ADP-001"], "{inner}");
        }
        // An absent plain module is the compiler's to refuse, not a finding.
        assert_eq!(reqs("mod not_here;"), Vec::<&str>::new());
    }

    #[test]
    fn unresolvable_module_paths_and_include_fail_closed() {
        fails_closed("#[path = \"nowhere.rs\"]\nmod h;");
        fails_closed("#[cfg_attr(unix, path = \"u.rs\")]\nmod h;");
        fails_closed("#[path = concat!(\"x\", \".rs\")]\nmod h;");
        fails_closed("include!(\"generated.rs\");");
        fails_closed("fn f() { include!(concat!(env!(\"OUT_DIR\"), \"/g.rs\")); }");
        fails_closed("fn f() { std::include!(\"g.rs\"); }");
        // The same, written inside a macro's tokens.
        fails_closed("macro_rules! m { () => { include!(\"g.rs\"); } }");
        fails_closed("macro_rules! m { () => { #[path = \"../x.rs\"] mod h; } }\nm!();");
        fails_closed("macro_rules! m { () => { #[cfg_attr(all(), path = \"x.rs\")] mod h; } }");
        // include_str! and include_bytes! paste in a file the scan does not read too.
        fails_closed("const S: &str = include_str!(\"x.txt\");");
        fails_closed("const B: &[u8] = include_bytes!(\"x.bin\");");
    }

    // ---- the PR #336 review ------------------------------------------------------------

    /// B1: a `path` anywhere in a `cfg_attr`, nested or not.
    #[test]
    fn a_path_in_a_nested_cfg_attr_fails_closed() {
        fails_closed("#[cfg_attr(all(), cfg_attr(all(), path = \"../zz/h.rs\"))]\nmod h;");
        fails_closed(
            "#[cfg_attr(all(), cfg_attr(any(), cfg_attr(all(), path = \"../zz/h.rs\")))]\nmod h;",
        );
        fails_closed(
            "mod o {\n    #[cfg_attr(all(), cfg_attr(all(), path = \"h.rs\"))]\n    mod h;\n}",
        );
        fails_closed(
            "macro_rules! m { () => { #[cfg_attr(all(), cfg_attr(all(), path = \"x.rs\"))] mod h {} } }",
        );
    }

    /// B2: an include macro under any path or alias, in code or in a macro's tokens.
    #[test]
    fn include_macros_under_any_path_or_alias_fail_closed() {
        for src in [
            "use std::include as inc;\nfn f() { inc!(\"../zz/h.rs\"); }",
            "use core::include_str as s;",
            "fn f() { ::core::include!(\"../zz/h.rs\"); }",
            "fn f() { core::prelude::v1::include_bytes!(\"x\"); }",
            "macro_rules! m { () => { ::core::include!(\"../zz/h.rs\"); } }",
            "macro_rules! m { () => { $crate::inc!(\"../zz/h.rs\"); } }\npub use std::include as inc;",
            "fn f() { m!(std::include_str!(\"x\")); }",
        ] {
            fails_closed(src);
        }
    }

    /// B3: an out-of-line module declared in a macro's tokens; the review's plant puts it
    /// under an inline `#[path]` module, outside `src/`.
    #[test]
    fn out_of_line_modules_in_macro_tokens_fail_closed() {
        fails_closed(
            "macro_rules! m { ($i:ident) => { pub mod $i; } }\n#[path = \"../zz\"] mod outer { m!(h); }",
        );
        fails_closed("macro_rules! m { () => { mod h; } }");
        fails_closed("fn f() { m!(mod h;); }");
        // An inline module in a macro is read from the tokens, not from a file.
        assert_eq!(
            reqs("macro_rules! m { () => { mod h { pub fn f() {} } } }"),
            Vec::<&str>::new()
        );
    }

    /// N6: the suite's planted breaches, reached from adapter code.
    #[test]
    fn the_planted_breaches_are_forbidden() {
        assert_eq!(
            reqs(
                "use oac_contract_adapter::plant;\nfn f() { let _ = plant::forged_attachment(); }"
            ),
            [PLANT]
        );
        assert_eq!(
            reqs("fn f() { let _ = oac_contract_adapter::plant::forged_attachment(); }"),
            [PLANT]
        );
    }

    /// N7: a symlink on the way to a module file fails closed.
    #[test]
    fn a_symlink_on_the_way_to_a_module_fails_closed() {
        let files = [
            ("src/lib.rs", "#[path = \"l/../x.rs\"]\nmod h;"),
            ("src/x.rs", "pub fn clean() {}"),
        ];
        let (findings, impls) = analyse_files_with_symlinks(&["src/lib.rs"], &files, &["src/l"]);
        assert_eq!(reqs_of(&findings), EVERY, "{findings:?}");
        assert!(!impls.is_empty());
        let (findings, _) = analyse_files_with_symlinks(
            &["src/lib.rs"],
            &[("src/lib.rs", "mod m;"), ("src/m.rs", "")],
            &["src/m.rs"],
        );
        assert_eq!(reqs_of(&findings), EVERY, "{findings:?}");
        // The same tree without the symlink is clean.
        let (findings, _) = analyse_files(&["src/lib.rs"], &files);
        assert_eq!(reqs_of(&findings), Vec::<&str>::new());
    }

    #[test]
    fn macro_tokens_resolve_globs_crate_and_dollar_crate() {
        for body in [
            "crate::transport::Payload",
            "$crate::transport::Payload",
            "self::transport::Payload",
            "super::transport::Payload",
            "transport::Payload",
            "$crate :: r#transport :: Payload",
        ] {
            assert_eq!(
                reqs(&format!(
                    "pub use oac_core::*;\nmacro_rules! m {{ () => {{ let _: Option<{body}> = None; }} }}"
                )),
                ["IFC-ADP-001"],
                "{body}"
            );
        }
        // A glob of a module, and a rename, reached through `$crate`.
        assert_eq!(
            reqs(
                "pub use oac_core::ids::*;\nmacro_rules! m { () => { $crate::SessionId::from_random_octets([0; 16]) } }"
            ),
            ["IFC-ADP-007"]
        );
        assert_eq!(
            reqs(
                "pub use oac_core::transport as net;\nmacro_rules! m { () => { $crate::net::Payload } }"
            ),
            ["IFC-ADP-001"]
        );
        // In an invocation's arguments too.
        assert_eq!(
            reqs("pub use oac_core::*;\nfn f() { m!(crate::signing::authenticate); }"),
            ["IFC-ADP-002"]
        );
    }

    #[test]
    fn the_tripwire_reads_macro_bodies() {
        for src in [
            "pub use oac_core::adapter::ProviderAdapter;\nmacro_rules! m { ($t:ty) => { impl crate::ProviderAdapter for $t {} } }",
            "pub use oac_core::adapter::ProviderAdapter;\nmacro_rules! m { ($t:ty) => { impl $crate::ProviderAdapter for $t {} } }",
            "pub use oac_core::*;\nmacro_rules! m { ($t:ty) => { impl<T: Fn() -> u8> $crate::adapter::ProviderAdapter for $t {} } }",
            "use oac_core::adapter as a;\nmacro_rules! m { ($t:ident) => { unsafe impl a::ProviderAdapter for $t {} } }",
            "macro_rules! m { ($t:ty) => { impl ::oac_core::adapter::ProviderAdapter for $t {} } }",
            "fn f() { m!(impl oac_core::adapter::ProviderAdapter for P {}); }",
        ] {
            assert!(implements(src), "{src}");
        }
        // The trait as a metavariable, handed ProviderAdapter at the call.
        let generic = "macro_rules! imp { ($tr:path, $t:ty) => { impl $tr for $t {} } }\n";
        assert!(implements(&format!(
            "{generic}pub use oac_core::adapter::ProviderAdapter as Port;\nstruct P;\nimp!(crate::Port, P);"
        )));
        let (_, impls) = analyse_texts(&[
            (
                "lib.rs",
                "pub use oac_core::adapter::ProviderAdapter;\nmod a;\nmod b;",
            ),
            ("a.rs", generic),
            ("b.rs", "struct P;\ncrate::imp!(crate::ProviderAdapter, P);"),
        ]);
        assert_eq!(impls.len(), 1, "{impls:?}");
        assert!(impls[0].file.ends_with("b.rs"));
        // Controls: other traits, and a generic macro handed another trait.
        assert!(!implements(
            "macro_rules! m { ($t:ty) => { impl Clone for $t { fn clone(&self) -> Self { todo!() } } } }"
        ));
        assert!(!implements(&format!("{generic}struct P;\nimp!(Clone, P);")));
        assert!(!implements(
            "pub use oac_core::adapter::ProviderAdapter;\nfn f() { let _ = format!(\"{}\", 1); m!(crate::ProviderAdapter); }"
        ));
    }

    #[test]
    fn clean_macros_pass() {
        assert_eq!(
            reqs(
                "use oac_core::ids::SessionId;\nfn f(transport: u8) -> String { format!(\"{transport} {:?}\", SessionId::parse(\"x\")) }\nmacro_rules! m { ($t:ty) => { impl Default for $t { fn default() -> Self { todo!() } } } }"
            ),
            Vec::<&str>::new()
        );
    }
}
