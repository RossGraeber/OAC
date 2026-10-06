// SPDX-License-Identifier: Apache-2.0

//! How Claude Code turns one `notifications/claude/channel` into what the model sees.
//!
//! Every rule here is one the fixtures show, with the line it comes from:
//!
//! - **Tag shape.** `<channel source="NAME" k="v" ...>`, a newline, the content, a newline,
//!   `</channel>` (G5 rendered lines 1, 3, 5, 7, 9, 11).
//! - **`source`** is the name the harness loaded the server under, not the server's own
//!   `serverInfo.name`: G1 Box C's server called itself `g1-spike-channel-server` (line 16)
//!   and rendered as `source="g1spike"`, its `server:g1spike` load name
//!   (`docs/planning/gates/G1-result.md`, Box C, criterion 2). First-party docs say the
//!   same: "set automatically from your server's configured name" (`channels-reference.md`,
//!   `docs/planning/REVERIFICATION-B2.md` row 12, retrieved 2026-10-02).
//! - **`meta` keys, in wire order, after `source`.** A key that is not ASCII letters,
//!   digits and underscore is dropped with no signal: hyphen, dot, space and non-ASCII
//!   letters (G5 wire line 30, rendered line 7: none appear, even in the raw log line), `!`
//!   and space (G1 Box C line 21; G1-result criterion 2). With the sender only under an
//!   unsafe key, the tag has no sender at all (G5 C4b, rendered line 9). First-party rule:
//!   "Keys must be identifiers: letters, digits, and underscores only"
//!   (`channels-reference.md`, REVERIFICATION-B2 row 3).
//! - **A `meta` key named `source` is kept**, as a second, trailing `source` attribute
//!   (G5 C5, rendered line 11).
//! - **Attribute values** escape `"` as `&quot;` (G5 C3, rendered line 5); an empty value
//!   renders as `""` (rendered line 1).
//! - **Content** escapes `</channel>` as `<\/channel>`; an opening `<channel ...>` and `"`
//!   in content stay as written (G5 C2, rendered line 3).
//!
//! What no fixture shows is not guessed. The attribute set ([`ChannelTag::attributes`])
//! is evidenced for every notification that renders at all. The exact rendered text is
//! evidenced only for content and values made of the characters the recorded renders
//! contain; for anything else [`ChannelTag::rendered`] is a [`RenderGap`], not a guess.

use std::fmt;

/// A `<channel>` tag as the harness would show it to the model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChannelTag {
    /// The harness's own `source` attribute: the configured server name.
    pub source: String,
    /// The `meta` attributes that survive, in wire order, after `source`. A `meta` key
    /// named `source` appears here too, as the real harness renders it.
    pub attributes: Vec<(String, String)>,
    /// The `meta` keys the harness dropped silently. The real harness gives no signal of
    /// these; the fake keeps them for test assertions only.
    pub dropped_keys: Vec<String>,
    /// The notification's `content`, as received.
    pub content: String,
    /// The tag's text, or why the fixtures do not fix it.
    pub rendered: Result<String, RenderGap>,
}

impl ChannelTag {
    /// Every value of attribute `name`, the harness `source` first when `name` is
    /// `source`.
    pub fn attribute(&self, name: &str) -> Vec<&str> {
        let own = (name == "source").then_some(self.source.as_str());
        own.into_iter()
            .chain(
                self.attributes
                    .iter()
                    .filter(|(k, _)| k == name)
                    .map(|(_, v)| v.as_str()),
            )
            .collect()
    }

    /// Attribute names in rendered order, `source` first.
    pub fn attribute_names(&self) -> Vec<&str> {
        std::iter::once("source")
            .chain(self.attributes.iter().map(|(k, _)| k.as_str()))
            .collect()
    }
}

/// Why the exact rendered text of a tag is not fixed by any fixture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RenderGap {
    /// The content holds a character or sequence no recorded render contains.
    Content(String),
    /// An attribute value holds a character no recorded render contains.
    Value {
        /// The attribute.
        key: String,
        /// What is not evidenced.
        detail: String,
    },
}

impl fmt::Display for RenderGap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RenderGap::Content(d) => write!(f, "content: {d}"),
            RenderGap::Value { key, detail } => write!(f, "value of {key}: {detail}"),
        }
    }
}

/// The fate of one `meta` key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyFate {
    /// Rendered as an attribute.
    Kept,
    /// Dropped silently.
    Dropped,
    /// Neither the fixtures nor the first-party docs say (the empty key).
    Unknown,
}

/// What happens to `meta` key `key`. ASCII letters, digits and underscore are kept; any
/// other character drops the key (see the module docs for the evidence). The empty key is
/// not evidenced either way.
pub fn key_fate(key: &str) -> KeyFate {
    if key.is_empty() {
        KeyFate::Unknown
    } else if key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        KeyFate::Kept
    } else {
        KeyFate::Dropped
    }
}

/// Builds the tag for one notification. `meta` is in wire order. Returns `Err(key)` when a
/// key's fate is [`KeyFate::Unknown`].
pub fn render(
    source: &str,
    content: &str,
    meta: &[(String, String)],
) -> Result<ChannelTag, String> {
    let mut attributes = Vec::new();
    let mut dropped_keys = Vec::new();
    for (k, v) in meta {
        match key_fate(k) {
            KeyFate::Kept => attributes.push((k.clone(), v.clone())),
            KeyFate::Dropped => dropped_keys.push(k.clone()),
            KeyFate::Unknown => return Err(k.clone()),
        }
    }
    let rendered = render_text(source, content, &attributes);
    Ok(ChannelTag {
        source: source.to_owned(),
        attributes,
        dropped_keys,
        content: content.to_owned(),
        rendered,
    })
}

fn render_text(
    source: &str,
    content: &str,
    attributes: &[(String, String)],
) -> Result<String, RenderGap> {
    let mut out = format!("<channel source=\"{source}\"");
    for (k, v) in attributes {
        out.push(' ');
        out.push_str(k);
        out.push_str("=\"");
        out.push_str(&escape_value(k, v)?);
        out.push('"');
    }
    out.push_str(">\n");
    out.push_str(&escape_content(content)?);
    out.push_str("\n</channel>");
    Ok(out)
}

/// Value characters the recorded renders contain: lowercase ASCII letters, digits, `-`,
/// `_`, space, `=`, and `"` (escaped). The empty value is recorded too.
fn escape_value(key: &str, v: &str) -> Result<String, RenderGap> {
    let mut out = String::with_capacity(v.len());
    for c in v.chars() {
        match c {
            '"' => out.push_str("&quot;"),
            'a'..='z' | '0'..='9' | '-' | '_' | ' ' | '=' => out.push(c),
            other => {
                return Err(RenderGap::Value {
                    key: key.to_owned(),
                    detail: format!("no recorded render contains {other:?} in a value"),
                });
            }
        }
    }
    Ok(out)
}

const CLOSE: &str = "</channel>";

/// Content the recorded renders contain: printable ASCII and newline, not empty, with no
/// leading or trailing whitespace, no `&`, and `</` only as `</channel>`.
fn escape_content(content: &str) -> Result<String, RenderGap> {
    if content.is_empty() {
        return Err(RenderGap::Content(
            "no recorded render has empty content".into(),
        ));
    }
    if content.trim() != content {
        return Err(RenderGap::Content(
            "no recorded render has leading or trailing whitespace".into(),
        ));
    }
    if let Some(c) = content
        .chars()
        .find(|&c| !(c == '\n' || (' '..='~').contains(&c)) || c == '&')
    {
        return Err(RenderGap::Content(format!(
            "no recorded render contains {c:?} in content"
        )));
    }
    let mut out = String::with_capacity(content.len());
    let mut rest = content;
    while let Some(i) = rest.find("</") {
        out.push_str(&rest[..i]);
        let Some(after) = rest[i..].strip_prefix(CLOSE) else {
            return Err(RenderGap::Content(
                "no recorded render contains `</` other than `</channel>`".into(),
            ));
        };
        out.push_str("<\\/channel>");
        rest = after;
    }
    out.push_str(rest);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn key_fates() {
        for k in ["oac_sender", "a1", "A_b", "_x", "9lives"] {
            assert_eq!(key_fate(k), KeyFate::Kept, "{k}");
        }
        for k in [
            "oac-sender",
            "oac.sender",
            "oac sender",
            "oac_s\u{e9}nder",
            "not identifier safe!",
        ] {
            assert_eq!(key_fate(k), KeyFate::Dropped, "{k}");
        }
        assert_eq!(key_fate(""), KeyFate::Unknown);
    }

    #[test]
    fn unknown_key_is_refused_not_guessed() {
        assert_eq!(render("s", "x", &m(&[("", "v")])), Err(String::new()));
    }

    #[test]
    fn gaps_are_reported_not_guessed() {
        let t = render("s", "caf\u{e9}", &[]).expect("renders");
        assert!(matches!(t.rendered, Err(RenderGap::Content(_))));
        let t = render("s", "a & b", &[]).expect("renders");
        assert!(matches!(t.rendered, Err(RenderGap::Content(_))));
        let t = render("s", "x</b>", &[]).expect("renders");
        assert!(matches!(t.rendered, Err(RenderGap::Content(_))));
        let t = render("s", " x", &[]).expect("renders");
        assert!(matches!(t.rendered, Err(RenderGap::Content(_))));
        let t = render("s", "x", &m(&[("k", "A")])).expect("renders");
        assert!(matches!(t.rendered, Err(RenderGap::Value { .. })));
        // The attribute set is still known.
        assert_eq!(t.attribute("k"), vec!["A"]);
    }

    #[test]
    fn duplicate_source_is_kept_after_the_harness_source() {
        let t = render("s", "x", &m(&[("source", "alice")])).expect("renders");
        assert_eq!(t.attribute("source"), vec!["s", "alice"]);
        assert_eq!(
            t.rendered.as_deref(),
            Ok("<channel source=\"s\" source=\"alice\">\nx\n</channel>")
        );
    }
}
