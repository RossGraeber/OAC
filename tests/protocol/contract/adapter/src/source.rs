// SPDX-License-Identifier: Apache-2.0

//! The static half of "adapters route through core policy rather than straight to a
//! transport" (#59 acceptance; `spec/interfaces.md` §5.1): a scan of an adapter's Rust
//! source, comments removed, for the core and transport items an adapter has no business
//! using.
//!
//! - [IFC-ADP-001]: no transport operation. The adapter names no transport type or crate.
//!   (`scripts/check-crate-deps.mjs` already stops an adapter from depending on a
//!   transport crate; this also catches `oac_core::transport` itself.)
//! - [IFC-ADP-002]: no envelope, presence record, registration record or receipt is
//!   created, signed, verified or altered. The adapter names no signing, key, trust,
//!   registration, replay or envelope-building item of the core.
//! - [IFC-ADP-007]: no binding, session id or authorization decision. The adapter mints no
//!   session id (parsing one a harness names is allowed) and names no authorization item.
//!
//! The scan is a tripwire, not a proof: it reads names, not behaviour. The dynamic checks
//! of the suite carry the rest ([IFC-ADP-003], [IFC-ADP-004]).

use std::fmt;
use std::path::{Path, PathBuf};

/// One forbidden name found in an adapter source file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    /// The requirement it breaks.
    pub requirement: &'static str,
    /// The file.
    pub file: PathBuf,
    /// The 1-based line.
    pub line: usize,
    /// The name found.
    pub pattern: &'static str,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}: `{}` ({})",
            self.file.display(),
            self.line,
            self.pattern,
            self.requirement
        )
    }
}

/// The names an adapter's code must not contain, with the requirement each breaks.
pub const FORBIDDEN: &[(&str, &str)] = &[
    ("IFC-ADP-001", "oac_core::transport"),
    ("IFC-ADP-001", "oac_transport_"),
    ("IFC-ADP-001", "dyn Transport"),
    ("IFC-ADP-001", "impl Transport"),
    ("IFC-ADP-002", "oac_core::signing"),
    ("IFC-ADP-002", "oac_core::keys"),
    ("IFC-ADP-002", "oac_core::trust"),
    ("IFC-ADP-002", "oac_core::registration"),
    ("IFC-ADP-002", "oac_core::replay"),
    ("IFC-ADP-002", "EnvelopeDraft"),
    ("IFC-ADP-002", "sign_envelope"),
    ("IFC-ADP-002", "receive_envelope"),
    ("IFC-ADP-002", "DeliveryReceipt::new"),
    ("IFC-ADP-007", "SessionId::from_random_octets"),
    ("IFC-ADP-007", "oac_core::authorization"),
    ("IFC-ADP-007", "AuthorizationDecision"),
];

/// Scan `files`; a file that cannot be read is skipped with no finding.
pub fn scan(files: &[PathBuf]) -> Vec<Finding> {
    let mut out = Vec::new();
    for f in files {
        if let Ok(text) = std::fs::read_to_string(f) {
            out.extend(scan_text(f, &text));
        }
    }
    out
}

/// Scan one file's text.
pub fn scan_text(file: &Path, text: &str) -> Vec<Finding> {
    let code = strip_comments(text);
    let mut out = Vec::new();
    for (i, line) in code.lines().enumerate() {
        for (req, pat) in FORBIDDEN {
            if line.contains(pat) {
                out.push(Finding {
                    requirement: req,
                    file: file.to_path_buf(),
                    line: i + 1,
                    pattern: pat,
                });
            }
        }
    }
    out
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

/// The text with `//` and `/* */` comments replaced by spaces, line breaks kept, string
/// literals left as they are.
pub fn strip_comments(text: &str) -> String {
    let b = text.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    let mut in_str = false;
    while i < b.len() {
        let c = b[i];
        if in_str {
            out.push(c);
            if c == b'\\' && i + 1 < b.len() {
                out.push(b[i + 1]);
                i += 2;
                continue;
            }
            if c == b'"' {
                in_str = false;
            }
            i += 1;
        } else if c == b'\'' && b.get(i + 1) == Some(&b'"') && b.get(i + 2) == Some(&b'\'') {
            // The char literal '"' opens no string.
            out.extend_from_slice(&b[i..i + 3]);
            i += 3;
        } else if c == b'"' {
            in_str = true;
            out.push(c);
            i += 1;
        } else if c == b'/' && b.get(i + 1) == Some(&b'/') {
            while i < b.len() && b[i] != b'\n' {
                out.push(b' ');
                i += 1;
            }
        } else if c == b'/' && b.get(i + 1) == Some(&b'*') {
            let mut depth = 0usize;
            while i < b.len() {
                if b[i] == b'/' && b.get(i + 1) == Some(&b'*') {
                    depth += 1;
                    out.extend_from_slice(b"  ");
                    i += 2;
                } else if b[i] == b'*' && b.get(i + 1) == Some(&b'/') {
                    depth -= 1;
                    out.extend_from_slice(b"  ");
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    out.push(if b[i] == b'\n' { b'\n' } else { b' ' });
                    i += 1;
                }
            }
        } else {
            out.push(c);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comments_do_not_count_and_code_does() {
        let p = Path::new("x.rs");
        let clean = "//! routes through oac_core::transport? never.\n/* EnvelopeDraft */\nlet s = \"// not a comment\";\n";
        assert!(scan_text(p, clean).is_empty());
        let bad = "use oac_core::transport::Transport;\nfn f() { let d = EnvelopeDraft::new(); }\n";
        let f = scan_text(p, bad);
        assert_eq!(
            f.iter().map(|x| (x.requirement, x.line)).collect::<Vec<_>>(),
            [("IFC-ADP-001", 1), ("IFC-ADP-002", 2)]
        );
        assert!(
            scan_text(p, "let s = SessionId::from_random_octets(x);")[0].requirement
                == "IFC-ADP-007"
        );
    }
}
