//! A strict recursive-descent JSON parser.
//!
//! Strict on purpose: result rows arrive from other machines and other people's
//! scripts, so trailing commas, bare identifiers, comments and trailing garbage
//! are errors rather than guesses. Nesting is capped so a hostile file cannot
//! turn a validation run into a stack overflow.

use super::value::{Json, JsonObject};
use std::fmt;

/// The deepest nesting a result document may use.
const MAX_DEPTH: usize = 64;

/// A parse failure, located by byte offset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    /// Byte offset into the input where the parser gave up.
    pub offset: usize,
    /// What the parser expected instead.
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid JSON at byte {}: {}", self.offset, self.message)
    }
}

impl std::error::Error for ParseError {}

/// Parses a complete JSON document.
pub fn parse(input: &str) -> Result<Json, ParseError> {
    let mut parser = Parser {
        bytes: input.as_bytes(),
        offset: 0,
    };
    parser.skip_whitespace();
    let value = parser.value(0)?;
    parser.skip_whitespace();
    if parser.offset != parser.bytes.len() {
        return Err(parser.error("trailing content after the document"));
    }
    Ok(value)
}

struct Parser<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl Parser<'_> {
    fn error(&self, message: &str) -> ParseError {
        ParseError {
            offset: self.offset,
            message: message.to_owned(),
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.offset).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let byte = self.peek();
        if byte.is_some() {
            self.offset += 1;
        }
        byte
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.offset += 1;
        }
    }

    fn expect(&mut self, byte: u8) -> Result<(), ParseError> {
        if self.peek() == Some(byte) {
            self.offset += 1;
            Ok(())
        } else {
            Err(self.error(&format!("expected `{}`", byte as char)))
        }
    }

    fn literal(&mut self, word: &str, value: Json) -> Result<Json, ParseError> {
        if self.bytes[self.offset..].starts_with(word.as_bytes()) {
            self.offset += word.len();
            Ok(value)
        } else {
            Err(self.error(&format!("expected `{word}`")))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, ParseError> {
        if depth > MAX_DEPTH {
            return Err(self.error("nesting deeper than 64 levels"));
        }
        match self.peek() {
            None => Err(self.error("unexpected end of input")),
            Some(b'n') => self.literal("null", Json::Null),
            Some(b't') => self.literal("true", Json::Bool(true)),
            Some(b'f') => self.literal("false", Json::Bool(false)),
            Some(b'"') => self.string().map(Json::Str),
            Some(b'[') => self.array(depth),
            Some(b'{') => self.object(depth),
            Some(byte) if byte == b'-' || byte.is_ascii_digit() => self.number(),
            Some(_) => Err(self.error("expected a JSON value")),
        }
    }

    fn array(&mut self, depth: usize) -> Result<Json, ParseError> {
        self.expect(b'[')?;
        let mut items = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(b']') {
            self.offset += 1;
            return Ok(Json::Array(items));
        }
        loop {
            self.skip_whitespace();
            items.push(self.value(depth + 1)?);
            self.skip_whitespace();
            match self.bump() {
                Some(b',') => continue,
                Some(b']') => return Ok(Json::Array(items)),
                _ => {
                    self.offset = self.offset.saturating_sub(1);
                    return Err(self.error("expected `,` or `]`"));
                }
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, ParseError> {
        self.expect(b'{')?;
        let mut object = JsonObject::new();
        self.skip_whitespace();
        if self.peek() == Some(b'}') {
            self.offset += 1;
            return Ok(Json::Object(object));
        }
        loop {
            self.skip_whitespace();
            let key = self.string()?;
            if object.get(&key).is_some() {
                return Err(self.error(&format!("duplicate key `{key}`")));
            }
            self.skip_whitespace();
            self.expect(b':')?;
            self.skip_whitespace();
            let value = self.value(depth + 1)?;
            object = object.with(key, value);
            self.skip_whitespace();
            match self.bump() {
                Some(b',') => continue,
                Some(b'}') => return Ok(Json::Object(object)),
                _ => {
                    self.offset = self.offset.saturating_sub(1);
                    return Err(self.error("expected `,` or `}`"));
                }
            }
        }
    }

    fn number(&mut self) -> Result<Json, ParseError> {
        let start = self.offset;
        if self.peek() == Some(b'-') {
            self.offset += 1;
        }
        while matches!(self.peek(), Some(byte) if byte.is_ascii_digit()) {
            self.offset += 1;
        }
        let mut floating = false;
        if self.peek() == Some(b'.') {
            floating = true;
            self.offset += 1;
            while matches!(self.peek(), Some(byte) if byte.is_ascii_digit()) {
                self.offset += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            floating = true;
            self.offset += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.offset += 1;
            }
            while matches!(self.peek(), Some(byte) if byte.is_ascii_digit()) {
                self.offset += 1;
            }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.offset])
            .map_err(|_| self.error("number is not valid utf-8"))?;
        if floating {
            return text
                .parse::<f64>()
                .map(Json::Float)
                .map_err(|_| self.error("malformed number"));
        }
        if let Ok(number) = text.parse::<u64>() {
            return Ok(Json::Uint(number));
        }
        text.parse::<i64>()
            .map(Json::Int)
            .map_err(|_| self.error("integer out of range"))
    }

    fn string(&mut self) -> Result<String, ParseError> {
        self.expect(b'"')?;
        let mut out = String::new();
        loop {
            match self.bump() {
                None => return Err(self.error("unterminated string")),
                Some(b'"') => return Ok(out),
                Some(b'\\') => self.escape(&mut out)?,
                Some(byte) if byte < 0x20 => {
                    return Err(self.error("unescaped control character in string"))
                }
                Some(byte) => {
                    // Re-decode the UTF-8 sequence starting at this byte.
                    let start = self.offset - 1;
                    let width = utf8_width(byte);
                    if start + width > self.bytes.len() {
                        return Err(self.error("truncated utf-8 sequence"));
                    }
                    let text = std::str::from_utf8(&self.bytes[start..start + width])
                        .map_err(|_| self.error("invalid utf-8 in string"))?;
                    out.push_str(text);
                    self.offset = start + width;
                }
            }
        }
    }

    fn escape(&mut self, out: &mut String) -> Result<(), ParseError> {
        match self.bump() {
            Some(b'"') => out.push('"'),
            Some(b'\\') => out.push('\\'),
            Some(b'/') => out.push('/'),
            Some(b'b') => out.push('\u{08}'),
            Some(b'f') => out.push('\u{0c}'),
            Some(b'n') => out.push('\n'),
            Some(b'r') => out.push('\r'),
            Some(b't') => out.push('\t'),
            Some(b'u') => {
                let code = self.hex4()?;
                let character = match code {
                    0xD800..=0xDBFF => {
                        self.expect(b'\\')?;
                        self.expect(b'u')?;
                        let low = self.hex4()?;
                        if !(0xDC00..=0xDFFF).contains(&low) {
                            return Err(self.error("unpaired utf-16 surrogate"));
                        }
                        let combined = 0x1_0000
                            + ((u32::from(code) - 0xD800) << 10)
                            + (u32::from(low) - 0xDC00);
                        char::from_u32(combined)
                            .ok_or_else(|| self.error("invalid surrogate pair"))?
                    }
                    0xDC00..=0xDFFF => return Err(self.error("unpaired utf-16 surrogate")),
                    _ => char::from_u32(u32::from(code))
                        .ok_or_else(|| self.error("invalid code point"))?,
                };
                out.push(character);
            }
            _ => return Err(self.error("unknown escape sequence")),
        }
        Ok(())
    }

    fn hex4(&mut self) -> Result<u16, ParseError> {
        if self.offset + 4 > self.bytes.len() {
            return Err(self.error("truncated \\u escape"));
        }
        let text = std::str::from_utf8(&self.bytes[self.offset..self.offset + 4])
            .map_err(|_| self.error("malformed \\u escape"))?;
        let code = u16::from_str_radix(text, 16).map_err(|_| self.error("malformed \\u escape"))?;
        self.offset += 4;
        Ok(code)
    }
}

fn utf8_width(byte: u8) -> usize {
    match byte {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        _ => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::super::write::to_string_compact;
    use super::*;

    #[test]
    fn parses_a_nested_document() {
        let value = parse(r#"{"a":[1,-2,3.5],"b":{"c":true,"d":null},"e":"x"}"#).expect("parses");
        assert_eq!(value.get_path("b.c"), Some(&Json::Bool(true)));
        assert_eq!(value.get_path("e"), Some(&Json::string("x")));
        assert_eq!(
            value
                .get_path("a")
                .and_then(Json::as_array)
                .map(<[Json]>::len),
            Some(3)
        );
    }

    #[test]
    fn round_trips_through_the_writer() {
        let text = r#"{"a":1,"b":[2,3],"c":{"d":"e"},"f":true,"g":null}"#;
        let value = parse(text).expect("parses");
        assert_eq!(to_string_compact(&value), text);
    }

    #[test]
    fn rejects_malformed_documents() {
        for text in [
            "{",
            "{\"a\":1,}",
            "[1,]",
            "{\"a\" 1}",
            "{'a':1}",
            "{\"a\":1}{",
            "nul",
            "",
            "{\"a\":1,\"a\":2}",
        ] {
            assert!(parse(text).is_err(), "expected `{text}` to be rejected");
        }
    }

    #[test]
    fn rejects_nesting_past_the_depth_cap() {
        let deep = format!("{}{}", "[".repeat(200), "]".repeat(200));
        let error = parse(&deep).expect_err("depth cap applies");
        assert!(error.message.contains("nesting"));
    }

    #[test]
    fn decodes_escapes_and_surrogate_pairs() {
        let value = parse(r#""ab\n😀""#).expect("parses");
        assert_eq!(value.as_str(), Some("ab\n\u{1f600}"));

        let escaped = parse(r#""\ud83d\ude00""#).expect("parses");
        assert_eq!(escaped.as_str(), Some("\u{1f600}"));

        assert!(parse(r#""\ud83d""#).is_err(), "unpaired high surrogate");
        assert!(parse(r#""\ude00""#).is_err(), "unpaired low surrogate");
    }

    #[test]
    fn keeps_large_integers_exact() {
        let value = parse("18446744073709551615").expect("parses");
        assert_eq!(value.as_u64(), Some(u64::MAX));
    }
}
