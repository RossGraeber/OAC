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
//! # What a static scan still cannot see
//!
//! It is a tripwire, not a proof. It reads source, not behaviour:
//!
//! - Macro bodies are token streams, not paths. The scan checks each macro's tokens as
//!   text, with whitespace removed, for the forbidden paths under the file's renames, but a
//!   macro that builds a path from pieces (`concat_idents!`, a `macro_rules!` that pastes an
//!   ident) or a procedural macro that expands to one is not seen.
//! - Code the scan is not given: `build.rs` output, `include!` of a generated file, a
//!   `#[path]` module outside the scanned set, and other crates (a helper crate the adapter
//!   depends on is scanned only if its files are passed in; `check-crate-deps.mjs` stops
//!   that crate from being a transport).
//! - Calls through values: a `dyn Transport` handed in from outside is a type, and
//!   `oac_core::transport` would be named to get it, but a closure or trait object
//!   that wraps a transport operation is invisible here. The dynamic checks of the suite
//!   carry that ([IFC-ADP-003], [IFC-ADP-004]).
//! - Re-exports from other crates: if a dependency re-exports a forbidden item under
//!   another path, only that dependency's own scan finds it. Within the files given,
//!   module structure is not modelled: every import counts everywhere, which can only add
//!   findings.

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

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
];

/// Method names an adapter must not call, with the requirement each breaks.
pub const FORBIDDEN_METHODS: &[(&str, &str)] = &[("IFC-ADP-002", "sign_envelope")];

/// The trait whose implementation the tripwire looks for.
pub const PROVIDER_ADAPTER: &[&str] = &["oac_core", "adapter", "ProviderAdapter"];

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

    fn check_tokens(&mut self, tokens: &str, line: usize) {
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
        // a tool attribute) is read as comma-separated paths where it parses as such, and
        // as text otherwise.
        let line = a
            .path()
            .segments
            .first()
            .map_or(0, |s| line_of(s.ident.span()));
        if let syn::Meta::List(list) = &a.meta {
            let parser = syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated;
            if let Ok(paths) = syn::parse::Parser::parse2(parser, list.tokens.clone()) {
                for p in paths {
                    self.check_path(&segments(&p), line);
                }
            }
            self.check_tokens(&list.tokens.to_string(), line);
        }
        visit::visit_attribute(self, a);
    }

    fn visit_macro(&mut self, m: &'ast syn::Macro) {
        let line = m
            .path
            .segments
            .first()
            .map_or(0, |s| line_of(s.ident.span()));
        self.check_tokens(&m.tokens.to_string(), line);
        visit::visit_macro(self, m);
    }

    fn visit_item_impl(&mut self, i: &'ast syn::ItemImpl) {
        if let Some((_, path, _)) = &i.trait_ {
            let line = path.segments.first().map_or(0, |s| line_of(s.ident.span()));
            if self
                .imports
                .candidates(&segments(path))
                .iter()
                .any(|c| c == PROVIDER_ADAPTER)
            {
                self.implements.push(Finding {
                    requirement: "IFC-ADP-010",
                    file: self.file.to_path_buf(),
                    line,
                    what: format!("impl {} for ..", segments(path).join("::")),
                });
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

/// Both scans over a set of files read as one crate: imports and `pub use` re-exports of
/// every file are collected first, then every file is checked against all of them. Each
/// item is (path, text, or why it could not be read). A file that cannot be read or does
/// not parse is a finding under every requirement, so it fails closed.
fn analyse_set(files: &[(PathBuf, Result<String, String>)]) -> (Vec<Finding>, Vec<Finding>) {
    let mut imports = Imports::default();
    let mut parsed = Vec::new();
    let (mut findings, mut implements) = (Vec::new(), Vec::new());
    for (path, text) in files {
        match text.as_ref().map(|t| syn::parse_file(t)) {
            Ok(Ok(ast)) => {
                ImportCollector(&mut imports).visit_file(&ast);
                parsed.push((path, ast));
            }
            Ok(Err(e)) => {
                let all = everywhere(path, line_of(e.span()), &format!("does not parse: {e}"));
                findings.extend(all.clone());
                implements.extend(all);
            }
            Err(why) => {
                let all = everywhere(path, 0, &format!("cannot be read: {why}"));
                findings.extend(all.clone());
                implements.extend(all);
            }
        }
    }
    for (path, ast) in parsed {
        let mut c = Checker {
            imports: &imports,
            file: path,
            findings: Vec::new(),
            implements: Vec::new(),
        };
        c.visit_file(&ast);
        findings.extend(c.findings);
        implements.extend(c.implements);
    }
    (findings, implements)
}

fn read_all(files: &[PathBuf]) -> Vec<(PathBuf, Result<String, String>)> {
    files
        .iter()
        .map(|f| {
            (
                f.clone(),
                std::fs::read_to_string(f).map_err(|e| e.to_string()),
            )
        })
        .collect()
}

/// Both scans of one file's text: forbidden reaches, and `ProviderAdapter` implementations.
pub fn analyse(file: &Path, text: &str) -> (Vec<Finding>, Vec<Finding>) {
    analyse_set(&[(file.to_path_buf(), Ok(text.to_owned()))])
}

/// Both scans of several files' texts, read as one crate ([`scan`]).
pub fn analyse_texts(files: &[(&str, &str)]) -> (Vec<Finding>, Vec<Finding>) {
    let set: Vec<_> = files
        .iter()
        .map(|(p, t)| (PathBuf::from(p), Ok((*t).to_owned())))
        .collect();
    analyse_set(&set)
}

/// The forbidden reaches in one file's text.
pub fn scan_text(file: &Path, text: &str) -> Vec<Finding> {
    analyse(file, text).0
}

/// Scan `files`, read as one crate (imports and re-exports of every file apply to all).
pub fn scan(files: &[PathBuf]) -> Vec<Finding> {
    analyse_set(&read_all(files)).0
}

/// Every `impl` of `oac_core::adapter::ProviderAdapter` in `files`, read as one crate,
/// under any alias or re-export.
pub fn implements_provider_adapter(files: &[PathBuf]) -> Vec<Finding> {
    analyse_set(&read_all(files)).1
}

/// Every `.rs` file under `dir`, recursively, in path order.
pub fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut v = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
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
/// there is one.
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
}
