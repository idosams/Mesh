//! Test-only support: a minimal JSON reader, and the paths to the repository's own artifacts.
//!
//! This crate declares no dependency, so a test that needs to read a published JSON artifact needs
//! a reader. It is deliberately small and strict: it accepts the subset the artifacts in
//! `protocol/test-vectors/` and `docs/` actually use, and refuses everything else rather than
//! guessing. A lenient parser here would let a malformed vector file pass as an empty one, which
//! is the failure mode that matters — a conformance test that silently checks nothing.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::PathBuf;

use mesh_operations::{CanonicalValue, CborWriter};

/// The `mesh-cbor/0` encoding of one canonical record: `[ domain_tag, field… ]`.
///
/// The same shape `encode_canonical` produces, driven by values a test supplies rather than by a
/// Rust type. That is what lets a test encode a record this crate has no type for — a published
/// `file-manifest` vector — and what lets it encode a *mutated* record to show the encoding moves.
pub fn encode_record(domain: &str, fields: &[CanonicalValue]) -> Vec<u8> {
    let mut writer = CborWriter::with_capacity(64);
    writer.array((fields.len() + 1) as u64);
    writer.text(domain);
    for field in fields {
        write_value(&mut writer, field);
    }
    writer.finish()
}

fn write_value(writer: &mut CborWriter, value: &CanonicalValue) {
    match value {
        CanonicalValue::Unsigned(number) => {
            writer.unsigned(*number);
        }
        CanonicalValue::Bool(flag) => {
            writer.bool(*flag);
        }
        CanonicalValue::Bytes(bytes) => {
            writer.bytes(bytes);
        }
        CanonicalValue::Text(text) => {
            writer.text(text);
        }
        CanonicalValue::Sequence(items) | CanonicalValue::Group(items) => {
            writer.array(items.len() as u64);
            for item in items {
                write_value(writer, item);
            }
        }
        CanonicalValue::Record(encoding) => {
            writer.nested(encoding);
        }
    }
}

/// The repository root, from this crate's manifest directory.
pub fn repo_root() -> PathBuf {
    [env!("CARGO_MANIFEST_DIR"), "..", ".."].iter().collect()
}

/// A path inside the repository.
pub fn repo_path(relative: &str) -> PathBuf {
    let mut path = repo_root();
    for segment in relative.split('/') {
        path.push(segment);
    }
    path
}

/// Read a repository file, failing with a message that says what is now unchecked.
pub fn read_repo_file(relative: &str) -> String {
    let path = repo_path(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "cannot read {} ({error}). This test holds mesh-operations against an artifact the \
             repository publishes; if the file has moved, the property it checks is unchecked \
             until this path is corrected.",
            path.display()
        )
    })
}

/// A JSON value, in the subset the artifacts use.
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Json>),
    Object(BTreeMap<String, Json>),
}

impl Json {
    /// The member of an object, or `None`.
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Self::Object(members) => members.get(key),
            _ => None,
        }
    }

    /// The member of an object, panicking with the key when it is absent.
    pub fn field(&self, key: &str) -> &Json {
        self.get(key)
            .unwrap_or_else(|| panic!("the artifact has no {key:?} member: {self:?}"))
    }

    /// This value as a string.
    pub fn as_str(&self) -> &str {
        match self {
            Self::String(text) => text,
            other => panic!("expected a string, found {other:?}"),
        }
    }

    /// This value as an unsigned integer.
    pub fn as_u64(&self) -> u64 {
        match self {
            Self::Number(value) => {
                assert!(
                    value.fract() == 0.0 && *value >= 0.0,
                    "expected an unsigned integer, found {value}"
                );
                *value as u64
            }
            other => panic!("expected a number, found {other:?}"),
        }
    }

    /// This value as a boolean.
    pub fn as_bool(&self) -> bool {
        match self {
            Self::Bool(value) => *value,
            other => panic!("expected a boolean, found {other:?}"),
        }
    }

    /// This value as an array.
    pub fn as_array(&self) -> &[Json] {
        match self {
            Self::Array(items) => items,
            other => panic!("expected an array, found {other:?}"),
        }
    }
}

/// Parse `text` as JSON, panicking on anything the subset does not admit.
pub fn parse_json(text: &str) -> Json {
    let bytes: Vec<char> = text.chars().collect();
    let mut at = 0usize;
    let value = parse_value(&bytes, &mut at);
    skip_whitespace(&bytes, &mut at);
    assert_eq!(at, bytes.len(), "trailing characters after the JSON value");
    value
}

fn skip_whitespace(bytes: &[char], at: &mut usize) {
    while *at < bytes.len() && matches!(bytes[*at], ' ' | '\t' | '\n' | '\r') {
        *at += 1;
    }
}

fn expect(bytes: &[char], at: &mut usize, character: char) {
    assert!(
        *at < bytes.len() && bytes[*at] == character,
        "expected {character:?} at {at}"
    );
    *at += 1;
}

fn parse_value(bytes: &[char], at: &mut usize) -> Json {
    skip_whitespace(bytes, at);
    assert!(*at < bytes.len(), "the JSON value ended early");
    match bytes[*at] {
        '{' => parse_object(bytes, at),
        '[' => parse_array(bytes, at),
        '"' => Json::String(parse_string(bytes, at)),
        't' => {
            take_literal(bytes, at, "true");
            Json::Bool(true)
        }
        'f' => {
            take_literal(bytes, at, "false");
            Json::Bool(false)
        }
        'n' => {
            take_literal(bytes, at, "null");
            Json::Null
        }
        _ => parse_number(bytes, at),
    }
}

fn take_literal(bytes: &[char], at: &mut usize, literal: &str) {
    for expected in literal.chars() {
        expect(bytes, at, expected);
    }
}

fn parse_object(bytes: &[char], at: &mut usize) -> Json {
    expect(bytes, at, '{');
    let mut members = BTreeMap::new();
    skip_whitespace(bytes, at);
    if bytes[*at] == '}' {
        *at += 1;
        return Json::Object(members);
    }
    loop {
        skip_whitespace(bytes, at);
        let key = parse_string(bytes, at);
        skip_whitespace(bytes, at);
        expect(bytes, at, ':');
        let value = parse_value(bytes, at);
        members.insert(key, value);
        skip_whitespace(bytes, at);
        match bytes[*at] {
            ',' => *at += 1,
            '}' => {
                *at += 1;
                return Json::Object(members);
            }
            other => panic!("expected ',' or '}}', found {other:?}"),
        }
    }
}

fn parse_array(bytes: &[char], at: &mut usize) -> Json {
    expect(bytes, at, '[');
    let mut items = Vec::new();
    skip_whitespace(bytes, at);
    if bytes[*at] == ']' {
        *at += 1;
        return Json::Array(items);
    }
    loop {
        items.push(parse_value(bytes, at));
        skip_whitespace(bytes, at);
        match bytes[*at] {
            ',' => *at += 1,
            ']' => {
                *at += 1;
                return Json::Array(items);
            }
            other => panic!("expected ',' or ']', found {other:?}"),
        }
    }
}

fn parse_string(bytes: &[char], at: &mut usize) -> String {
    expect(bytes, at, '"');
    let mut out = String::new();
    loop {
        assert!(*at < bytes.len(), "the string ended early");
        match bytes[*at] {
            '"' => {
                *at += 1;
                return out;
            }
            '\\' => {
                *at += 1;
                let escape = bytes[*at];
                *at += 1;
                out.push(match escape {
                    '"' => '"',
                    '\\' => '\\',
                    '/' => '/',
                    'b' => '\u{08}',
                    'f' => '\u{0c}',
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    'u' => {
                        let code: String = bytes[*at..*at + 4].iter().collect();
                        *at += 4;
                        let value =
                            u32::from_str_radix(&code, 16).expect("a four-digit hex escape");
                        char::from_u32(value).expect("a scalar value")
                    }
                    other => panic!("unsupported escape {other:?}"),
                });
            }
            character => {
                out.push(character);
                *at += 1;
            }
        }
    }
}

fn parse_number(bytes: &[char], at: &mut usize) -> Json {
    let start = *at;
    while *at < bytes.len() && matches!(bytes[*at], '-' | '+' | '.' | 'e' | 'E' | '0'..='9') {
        *at += 1;
    }
    let text: String = bytes[start..*at].iter().collect();
    Json::Number(
        text.parse()
            .unwrap_or_else(|_| panic!("not a number: {text:?}")),
    )
}

/// Decode a lowercase or uppercase hex string.
pub fn from_hex(text: &str) -> Vec<u8> {
    assert!(
        text.len() % 2 == 0,
        "a hex string has an even length: {text:?}"
    );
    (0..text.len() / 2)
        .map(|index| {
            u8::from_str_radix(&text[index * 2..index * 2 + 2], 16)
                .unwrap_or_else(|_| panic!("not hex: {text:?}"))
        })
        .collect()
}

/// Encode bytes as lowercase hex.
pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
