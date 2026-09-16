//! A deterministic JSON writer, for the documents published under `protocol/`.
//!
//! The published schema and the published test vectors are compared to the working tree
//! **byte for byte**, so the thing that renders them has to be as deterministic as the encoding it
//! describes. This module is that renderer, and it is deliberately a writer only:
//!
//! * **Member order is insertion order, never sorted.** The document builder decides the order
//!   once, in code, and the renderer preserves it. A sort would be a second ordering rule to keep
//!   in step with the schema's.
//! * **One layout, no options.** Two-space indent, `": "` after a key, `\n` line endings, one
//!   trailing newline, no trailing whitespace anywhere.
//! * **Numbers are `u64`.** There is no float in the published documents, because a float is a
//!   value two JSON writers can render differently.
//! * **No reader.** Nothing in this crate parses JSON. The compatibility oracle regenerates the
//!   documents and compares text, which needs no parser and additionally catches formatting drift
//!   that a parse-and-compare would silently accept.
//!
//! Strings are emitted as UTF-8 with only the escapes RFC 8259 requires, so a non-ASCII directory
//! entry name appears in the published vectors as itself. That is deliberate: a vector file an
//! external implementer opens should show the name it is about.

use core::fmt::Write as _;

/// A JSON value, in the subset the published documents use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Json {
    /// A string.
    String(String),
    /// An unsigned integer.
    Unsigned(u64),
    /// A boolean.
    Bool(bool),
    /// An array.
    Array(Vec<Json>),
    /// An object, rendered in the order its members were added.
    Object(Vec<(String, Json)>),
}

impl Json {
    /// A string member.
    #[must_use]
    pub fn string(value: impl Into<String>) -> Self {
        Self::String(value.into())
    }

    /// An object from ordered members.
    #[must_use]
    pub fn object<K: Into<String>>(members: impl IntoIterator<Item = (K, Self)>) -> Self {
        Self::Object(
            members
                .into_iter()
                .map(|(key, value)| (key.into(), value))
                .collect(),
        )
    }

    /// An array from its items.
    #[must_use]
    pub fn array(items: impl IntoIterator<Item = Self>) -> Self {
        Self::Array(items.into_iter().collect())
    }

    /// Render as a complete document: the value, then exactly one newline.
    #[must_use]
    pub fn to_document(&self) -> String {
        let mut out = String::new();
        self.render(0, &mut out);
        out.push('\n');
        out
    }

    fn render(&self, depth: usize, out: &mut String) {
        match self {
            Self::String(value) => write_escaped(value, out),
            Self::Unsigned(value) => {
                // `u64` has no locale, no exponent and no sign: one rendering per value.
                let _ = write!(out, "{value}");
            }
            Self::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
            Self::Array(items) => {
                if items.is_empty() {
                    out.push_str("[]");
                    return;
                }
                out.push_str("[\n");
                for (index, item) in items.iter().enumerate() {
                    indent(depth + 1, out);
                    item.render(depth + 1, out);
                    if index + 1 < items.len() {
                        out.push(',');
                    }
                    out.push('\n');
                }
                indent(depth, out);
                out.push(']');
            }
            Self::Object(members) => {
                if members.is_empty() {
                    out.push_str("{}");
                    return;
                }
                out.push_str("{\n");
                for (index, (key, value)) in members.iter().enumerate() {
                    indent(depth + 1, out);
                    write_escaped(key, out);
                    out.push_str(": ");
                    value.render(depth + 1, out);
                    if index + 1 < members.len() {
                        out.push(',');
                    }
                    out.push('\n');
                }
                indent(depth, out);
                out.push('}');
            }
        }
    }
}

fn indent(depth: usize, out: &mut String) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

/// Write `value` as a JSON string literal, escaping exactly what RFC 8259 §7 requires.
fn write_escaped(value: &str, out: &mut String) {
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other if (other as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", other as u32);
            }
            other => out.push(other),
        }
    }
    out.push('"');
}

/// The 64-character lowercase hex form of `bytes`, as the published documents carry byte strings.
#[must_use]
pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(DIGITS[(byte >> 4) as usize] as char);
        text.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scalar_renders_as_itself() {
        assert_eq!(Json::Unsigned(0).to_document(), "0\n");
        assert_eq!(
            Json::Unsigned(u64::MAX).to_document(),
            "18446744073709551615\n"
        );
        assert_eq!(Json::Bool(true).to_document(), "true\n");
        assert_eq!(Json::Bool(false).to_document(), "false\n");
        assert_eq!(Json::string("x").to_document(), "\"x\"\n");
    }

    #[test]
    fn empty_containers_stay_on_one_line() {
        assert_eq!(Json::array([]).to_document(), "[]\n");
        assert_eq!(
            Json::object(Vec::<(&str, Json)>::new()).to_document(),
            "{}\n"
        );
    }

    #[test]
    fn nesting_indents_by_two_spaces_and_keeps_insertion_order() {
        let document = Json::object([
            ("z", Json::Unsigned(1)),
            ("a", Json::array([Json::Unsigned(2), Json::Bool(false)])),
        ])
        .to_document();
        assert_eq!(
            document,
            "{\n  \"z\": 1,\n  \"a\": [\n    2,\n    false\n  ]\n}\n"
        );
    }

    /// The order is the builder's, never the renderer's. Sorting would be a second rule.
    #[test]
    fn members_are_never_sorted() {
        let document = Json::object([
            ("b", Json::Unsigned(1)),
            ("a", Json::Unsigned(2)),
            ("c", Json::Unsigned(3)),
        ])
        .to_document();
        assert_eq!(document, "{\n  \"b\": 1,\n  \"a\": 2,\n  \"c\": 3\n}\n");
    }

    #[test]
    fn every_required_escape_is_produced() {
        let mut out = String::new();
        write_escaped("\"\\\u{8}\u{c}\n\r\t\u{1}", &mut out);
        assert_eq!(out, "\"\\\"\\\\\\b\\f\\n\\r\\t\\u0001\"");
    }

    #[test]
    fn non_ascii_is_emitted_as_utf8() {
        let mut out = String::new();
        write_escaped("caf\u{e9}-\u{6c34}", &mut out);
        assert_eq!(out, "\"caf\u{e9}-\u{6c34}\"");
    }

    #[test]
    fn a_document_ends_with_exactly_one_newline() {
        let document = Json::object([("a", Json::Unsigned(1))]).to_document();
        assert!(document.ends_with("}\n"));
        assert!(!document.ends_with("\n\n"));
    }

    #[test]
    fn no_line_carries_trailing_whitespace() {
        let document = Json::object([
            ("a", Json::array([Json::object([("b", Json::Unsigned(1))])])),
            ("c", Json::array([])),
        ])
        .to_document();
        for line in document.lines() {
            assert_eq!(line.trim_end(), line, "trailing whitespace in {line:?}");
        }
    }

    #[test]
    fn hex_is_lowercase_and_two_characters_per_byte() {
        assert_eq!(hex(&[]), "");
        assert_eq!(hex(&[0x00, 0x0f, 0xa0, 0xff]), "000fa0ff");
    }

    /// Rendering is a pure function of the value: the same tree twice is the same text.
    #[test]
    fn rendering_is_deterministic() {
        let value = Json::object([
            ("one", Json::array([Json::string("a"), Json::string("b")])),
            ("two", Json::Unsigned(7)),
        ]);
        assert_eq!(value.to_document(), value.to_document());
    }
}
