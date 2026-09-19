//! The JSON subset the local IPC surface speaks, with a strict reader and a deterministic writer.
//!
//! # Why a subset, and why it is written here
//!
//! The desktop client is JavaScript, so the wire has to be something JavaScript reads without a
//! build step. It is also a **trust boundary**: the daemon holds the guards the user interface is
//! deliberately not allowed to bypass, and everything arriving on the socket is untrusted input.
//! A parser that accepts more than the surface needs is more attack surface for no benefit, so
//! this one accepts exactly:
//!
//! - objects, arrays, strings, `true`, `false`, `null`;
//! - numbers that are **non-negative integers inside `u64`** — no fractions, no exponents, no
//!   minus sign. Every number on this surface is a count, an identifier or a version.
//!
//! It rejects, by construction: trailing input, unterminated values, nesting past
//! [`MAX_DEPTH`], lone surrogates, control characters inside a string literal, and duplicate keys.
//!
//! # Why not a dependency
//!
//! `tools/program/arch-check/architecture.json` registers every third-party crate this workspace
//! may reach, with the capabilities it and its whole closure carry. That file belongs to task
//! `01KZC25P0N5WMX8FY2Q7BFH84Z` and is outside this task's allowed paths, so adding
//! `serde_json` here is not a decision this run can take — and `Cargo.lock` is fenced by
//! ADR-0014 besides. The subset above is small enough that the honest move is to write it.
//!
//! # Determinism
//!
//! [`Json::Object`] keeps insertion order rather than sorting, and the writer emits no whitespace.
//! Together with the fixed key order every message in [`crate::ipc::message`] declares, one value
//! has exactly one encoding, which is what lets the shared corpus in
//! `crates/mesh-daemon/ipc-contract.json` be compared **byte for byte** against the JavaScript
//! client's encoder. The escaping rule is deliberately the one `JSON.stringify` uses: escape `"`,
//! `\` and every code point below `0x20`, and pass everything else through as UTF-8.

use core::fmt;

/// How deeply a value may nest before the reader refuses it.
///
/// The surface's deepest legal shape is `call` → `params` → a value, so 16 is far past anything
/// legitimate and far below anything that could exhaust the stack.
pub const MAX_DEPTH: usize = 16;

/// A JSON value in the subset above.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Json {
    /// `null`.
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// A non-negative integer.
    Number(u64),
    /// A string.
    Text(String),
    /// An array.
    Array(Vec<Json>),
    /// An object, in insertion order.
    Object(Vec<(String, Json)>),
}

impl Json {
    /// An empty object — the shape of a call with no parameters.
    #[must_use]
    pub const fn empty_object() -> Self {
        Self::Object(Vec::new())
    }

    /// Build an object from pairs, preserving the order given.
    #[must_use]
    pub fn object<K: Into<String>, I: IntoIterator<Item = (K, Self)>>(pairs: I) -> Self {
        Self::Object(
            pairs
                .into_iter()
                .map(|(key, value)| (key.into(), value))
                .collect(),
        )
    }

    /// A string value.
    #[must_use]
    pub fn text<S: Into<String>>(value: S) -> Self {
        Self::Text(value.into())
    }

    /// The value at `key`, when this is an object that has one.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Self> {
        match self {
            Self::Object(pairs) => pairs
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    /// This value as a string.
    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(value) => Some(value),
            _ => None,
        }
    }

    /// This value as an integer.
    #[must_use]
    pub const fn as_u64(&self) -> Option<u64> {
        match self {
            Self::Number(value) => Some(*value),
            _ => None,
        }
    }

    /// This value as a boolean.
    #[must_use]
    pub const fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    /// This value as an array.
    #[must_use]
    pub fn as_array(&self) -> Option<&[Self]> {
        match self {
            Self::Array(values) => Some(values),
            _ => None,
        }
    }

    /// Whether this value is an object.
    #[must_use]
    pub const fn is_object(&self) -> bool {
        matches!(self, Self::Object(_))
    }

    /// Write this value into `out` with no whitespace.
    pub fn write(&self, out: &mut String) {
        match self {
            Self::Null => out.push_str("null"),
            Self::Bool(true) => out.push_str("true"),
            Self::Bool(false) => out.push_str("false"),
            Self::Number(value) => out.push_str(&value.to_string()),
            Self::Text(value) => write_string(value, out),
            Self::Array(values) => {
                out.push('[');
                for (index, value) in values.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    value.write(out);
                }
                out.push(']');
            }
            Self::Object(pairs) => {
                out.push('{');
                for (index, (key, value)) in pairs.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    write_string(key, out);
                    out.push(':');
                    value.write(out);
                }
                out.push('}');
            }
        }
    }

    /// This value as one line of JSON, with no trailing newline.
    #[must_use]
    pub fn encode(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }

    /// Read one whole value from `input`, rejecting anything left over.
    ///
    /// # Errors
    ///
    /// Returns the byte offset and the reason on any input outside the subset this module
    /// documents.
    pub fn parse(input: &str) -> Result<Self, JsonError> {
        let mut reader = Reader {
            bytes: input.as_bytes(),
            at: 0,
        };
        reader.skip_whitespace();
        let value = reader.value(0)?;
        reader.skip_whitespace();
        if reader.at != reader.bytes.len() {
            return Err(reader.fail("trailing input after the value"));
        }
        Ok(value)
    }
}

impl fmt::Display for Json {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.encode())
    }
}

/// Why a JSON value could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonError {
    /// Byte offset into the input where the reader stopped.
    pub offset: usize,
    /// What the reader wanted instead.
    pub reason: String,
}

impl fmt::Display for JsonError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} at byte {}", self.reason, self.offset)
    }
}

impl std::error::Error for JsonError {}

/// Escape a string the way `JSON.stringify` does, so both sides of the socket agree byte for byte.
fn write_string(value: &str, out: &mut String) {
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
            control if (control as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", control as u32));
            }
            other => out.push(other),
        }
    }
    out.push('"');
}

/// The reader's cursor. Byte-oriented, because every structural character is ASCII.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn fail<S: Into<String>>(&self, reason: S) -> JsonError {
        JsonError {
            offset: self.at,
            reason: reason.into(),
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn expect(&mut self, byte: u8) -> Result<(), JsonError> {
        if self.peek() == Some(byte) {
            self.at += 1;
            Ok(())
        } else {
            Err(self.fail(format!("expected `{}`", byte as char)))
        }
    }

    fn literal(&mut self, word: &str) -> Result<(), JsonError> {
        if self.bytes[self.at..].starts_with(word.as_bytes()) {
            self.at += word.len();
            Ok(())
        } else {
            Err(self.fail(format!("expected `{word}`")))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, JsonError> {
        if depth > MAX_DEPTH {
            return Err(self.fail(format!("nesting deeper than {MAX_DEPTH}")));
        }
        match self.peek() {
            None => Err(self.fail("expected a value")),
            Some(b'n') => self.literal("null").map(|()| Json::Null),
            Some(b't') => self.literal("true").map(|()| Json::Bool(true)),
            Some(b'f') => self.literal("false").map(|()| Json::Bool(false)),
            Some(b'"') => self.string().map(Json::Text),
            Some(b'[') => self.array(depth),
            Some(b'{') => self.object(depth),
            Some(byte) if byte.is_ascii_digit() => self.number(),
            Some(b'-') => Err(self.fail("negative numbers are outside this surface")),
            Some(byte) => Err(self.fail(format!("unexpected `{}`", byte as char))),
        }
    }

    fn array(&mut self, depth: usize) -> Result<Json, JsonError> {
        self.expect(b'[')?;
        let mut values = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(b']') {
            self.at += 1;
            return Ok(Json::Array(values));
        }
        loop {
            self.skip_whitespace();
            values.push(self.value(depth + 1)?);
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b']') => {
                    self.at += 1;
                    return Ok(Json::Array(values));
                }
                _ => return Err(self.fail("expected `,` or `]`")),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, JsonError> {
        self.expect(b'{')?;
        let mut pairs: Vec<(String, Json)> = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(b'}') {
            self.at += 1;
            return Ok(Json::Object(pairs));
        }
        loop {
            self.skip_whitespace();
            let key = self.string()?;
            if pairs.iter().any(|(name, _)| *name == key) {
                return Err(self.fail(format!("duplicate key `{key}`")));
            }
            self.skip_whitespace();
            self.expect(b':')?;
            self.skip_whitespace();
            let value = self.value(depth + 1)?;
            pairs.push((key, value));
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    return Ok(Json::Object(pairs));
                }
                _ => return Err(self.fail("expected `,` or `}`")),
            }
        }
    }

    fn number(&mut self) -> Result<Json, JsonError> {
        let start = self.at;
        while matches!(self.peek(), Some(byte) if byte.is_ascii_digit()) {
            self.at += 1;
        }
        if matches!(self.peek(), Some(b'.' | b'e' | b'E')) {
            return Err(self.fail("fractions and exponents are outside this surface"));
        }
        let digits = &self.bytes[start..self.at];
        if digits.len() > 1 && digits[0] == b'0' {
            return Err(self.fail("a leading zero is not a number"));
        }
        // Every byte in the slice is an ASCII digit, so this is valid UTF-8 by construction.
        let text = core::str::from_utf8(digits).map_err(|_| self.fail("not a number"))?;
        text.parse::<u64>()
            .map(Json::Number)
            .map_err(|_| self.fail("number does not fit in 64 unsigned bits"))
    }

    fn string(&mut self) -> Result<String, JsonError> {
        self.expect(b'"')?;
        let mut out = String::new();
        loop {
            let byte = self
                .peek()
                .ok_or_else(|| self.fail("unterminated string"))?;
            match byte {
                b'"' => {
                    self.at += 1;
                    return Ok(out);
                }
                b'\\' => {
                    self.at += 1;
                    let escape = self
                        .peek()
                        .ok_or_else(|| self.fail("unterminated escape"))?;
                    self.at += 1;
                    match escape {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => out.push(self.unicode_escape()?),
                        other => {
                            return Err(self.fail(format!("unknown escape `\\{}`", other as char)))
                        }
                    }
                }
                control if control < 0x20 => {
                    return Err(self.fail("a raw control character is not allowed in a string"))
                }
                _ => {
                    // Structural and control bytes are ASCII and therefore cannot occur inside a
                    // multi-byte UTF-8 code point. Copy the whole ordinary run in one validation
                    // instead of validating the complete remaining message once per character.
                    // The latter made a large but bounded workspace reply quadratic in its wire
                    // size and could keep the desktop command busy for minutes after the daemon
                    // had already completed the import.
                    let start = self.at;
                    while let Some(byte) = self.peek() {
                        if byte == b'"' || byte == b'\\' || byte < 0x20 {
                            break;
                        }
                        self.at += 1;
                    }
                    let ordinary = core::str::from_utf8(&self.bytes[start..self.at])
                        .map_err(|_| self.fail("invalid UTF-8"))?;
                    out.push_str(ordinary);
                }
            }
        }
    }

    /// One `\uXXXX` escape, including the surrogate pair a code point above the BMP needs.
    fn unicode_escape(&mut self) -> Result<char, JsonError> {
        let high = self.hex4()?;
        if (0xD800..0xDC00).contains(&high) {
            self.literal("\\u")?;
            let low = self.hex4()?;
            if !(0xDC00..0xE000).contains(&low) {
                return Err(self.fail("a high surrogate must be followed by a low surrogate"));
            }
            let combined = 0x1_0000 + ((high - 0xD800) << 10) + (low - 0xDC00);
            return char::from_u32(combined).ok_or_else(|| self.fail("not a code point"));
        }
        if (0xDC00..0xE000).contains(&high) {
            return Err(self.fail("a lone low surrogate is not a code point"));
        }
        char::from_u32(high).ok_or_else(|| self.fail("not a code point"))
    }

    fn hex4(&mut self) -> Result<u32, JsonError> {
        if self.at + 4 > self.bytes.len() {
            return Err(self.fail("a `\\u` escape needs four hex digits"));
        }
        let mut value = 0u32;
        for _ in 0..4 {
            let digit = self.bytes[self.at];
            let nibble = match digit {
                b'0'..=b'9' => u32::from(digit - b'0'),
                b'a'..=b'f' => u32::from(digit - b'a') + 10,
                b'A'..=b'F' => u32::from(digit - b'A') + 10,
                _ => return Err(self.fail("a `\\u` escape needs four hex digits")),
            };
            value = (value << 4) | nibble;
            self.at += 1;
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_the_shapes_the_surface_uses() {
        let value = Json::object([
            ("t", Json::text("call")),
            ("id", Json::Number(7)),
            ("versions", Json::Array(vec![Json::Number(1)])),
            ("params", Json::empty_object()),
            ("resumed", Json::Bool(false)),
            ("nothing", Json::Null),
        ]);
        let line = value.encode();
        assert_eq!(
            line,
            r#"{"t":"call","id":7,"versions":[1],"params":{},"resumed":false,"nothing":null}"#
        );
        assert_eq!(Json::parse(&line), Ok(value));
    }

    #[test]
    fn object_order_is_insertion_order_not_sorted() {
        let value = Json::object([("z", Json::Number(1)), ("a", Json::Number(2))]);
        assert_eq!(value.encode(), r#"{"z":1,"a":2}"#);
    }

    #[test]
    fn escapes_exactly_what_json_stringify_escapes() {
        let value = Json::text("a\"b\\c\nd\te\u{1}f\u{e9}g");
        assert_eq!(value.encode(), "\"a\\\"b\\\\c\\nd\\te\\u0001f\u{e9}g\"");
    }

    #[test]
    fn rejects_everything_outside_the_subset() {
        for (input, why) in [
            ("-1", "negative"),
            ("1.5", "fraction"),
            ("1e3", "exponent"),
            ("01", "leading zero"),
            ("{\"a\":1}{}", "trailing input"),
            ("{\"a\":1,\"a\":2}", "duplicate key"),
            ("\"\u{1}\"", "raw control character"),
            ("\"\\ud800\"", "lone surrogate"),
            ("[1,", "unterminated"),
            ("18446744073709551616", "does not fit in u64"),
        ] {
            assert!(Json::parse(input).is_err(), "{why}: {input} was accepted");
        }
    }

    #[test]
    fn rejects_nesting_past_the_depth_limit() {
        let deep = format!("{}1{}", "[".repeat(40), "]".repeat(40));
        assert!(Json::parse(&deep).is_err());
        let shallow = format!("{}1{}", "[".repeat(4), "]".repeat(4));
        assert!(Json::parse(&shallow).is_ok());
    }

    #[test]
    fn reads_the_escapes_it_writes() {
        let value = Json::text("\u{1f600} \u{e9} \\ \" \n");
        assert_eq!(Json::parse(&value.encode()), Ok(value));
    }

    #[test]
    fn reads_a_large_workspace_string_in_linear_runs() {
        let ordinary = "a".repeat(2 * 1024 * 1024);
        let value = Json::text(format!("{ordinary}\u{1f600}\nfinished"));
        assert_eq!(Json::parse(&value.encode()), Ok(value));
    }

    #[test]
    fn accessors_answer_none_off_type() {
        let value = Json::Number(3);
        assert_eq!(value.as_text(), None);
        assert_eq!(value.get("anything"), None);
        assert_eq!(value.as_array(), None);
        assert_eq!(value.as_bool(), None);
        assert!(!value.is_object());
    }
}
