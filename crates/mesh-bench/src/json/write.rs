//! Deterministic JSON serialisation.

use super::value::Json;

/// Serialises a value on one line — the form written to result files.
pub fn to_string_compact(value: &Json) -> String {
    let mut out = String::new();
    write_value(&mut out, value, None, 0);
    out
}

/// Serialises a value with two-space indentation — the form printed for humans.
pub fn to_string_pretty(value: &Json) -> String {
    let mut out = String::new();
    write_value(&mut out, value, Some(2), 0);
    out
}

fn write_value(out: &mut String, value: &Json, indent: Option<usize>, depth: usize) {
    match value {
        Json::Null => out.push_str("null"),
        Json::Bool(true) => out.push_str("true"),
        Json::Bool(false) => out.push_str("false"),
        Json::Uint(number) => out.push_str(&number.to_string()),
        Json::Int(number) => out.push_str(&number.to_string()),
        Json::Float(number) => out.push_str(&write_float(*number)),
        Json::Str(text) => write_string(out, text),
        Json::Array(items) => write_sequence(out, items.len(), indent, depth, |out, index| {
            write_value(out, &items[index], indent, depth + 1);
        }),
        Json::Object(object) => {
            let entries = object.entries();
            write_object(out, entries.len(), indent, depth, |out, index| {
                let (key, child) = &entries[index];
                write_string(out, key);
                out.push(':');
                if indent.is_some() {
                    out.push(' ');
                }
                write_value(out, child, indent, depth + 1);
            });
        }
    }
}

/// JSON has no literal for infinities or NaN. The result schema never carries a
/// float, so this only fires for callers embedding their own values; `null`
/// keeps the document parseable and the decoder rejects it as a type mismatch
/// rather than letting an unrepresentable number pass as data.
fn write_float(number: f64) -> String {
    if number.is_finite() {
        format!("{number:?}")
    } else {
        "null".to_owned()
    }
}

fn write_sequence(
    out: &mut String,
    len: usize,
    indent: Option<usize>,
    depth: usize,
    mut write_item: impl FnMut(&mut String, usize),
) {
    write_bracketed(out, ('[', ']'), len, indent, depth, &mut write_item);
}

fn write_object(
    out: &mut String,
    len: usize,
    indent: Option<usize>,
    depth: usize,
    mut write_item: impl FnMut(&mut String, usize),
) {
    write_bracketed(out, ('{', '}'), len, indent, depth, &mut write_item);
}

fn write_bracketed(
    out: &mut String,
    brackets: (char, char),
    len: usize,
    indent: Option<usize>,
    depth: usize,
    write_item: &mut dyn FnMut(&mut String, usize),
) {
    out.push(brackets.0);
    if len == 0 {
        out.push(brackets.1);
        return;
    }
    for index in 0..len {
        if index > 0 {
            out.push(',');
        }
        write_newline(out, indent, depth + 1);
        write_item(out, index);
    }
    write_newline(out, indent, depth);
    out.push(brackets.1);
}

fn write_newline(out: &mut String, indent: Option<usize>, depth: usize) {
    if let Some(width) = indent {
        out.push('\n');
        for _ in 0..(width * depth) {
            out.push(' ');
        }
    }
}

fn write_string(out: &mut String, text: &str) {
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            control if control < '\u{20}' => {
                out.push_str(&format!("\\u{:04x}", control as u32));
            }
            other => out.push(other),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::super::value::JsonObject;
    use super::*;

    #[test]
    fn compact_form_has_no_whitespace() {
        let value = Json::Object(
            JsonObject::new()
                .with("a", Json::Uint(1))
                .with("b", Json::array([Json::Uint(2), Json::Uint(3)])),
        );
        assert_eq!(to_string_compact(&value), r#"{"a":1,"b":[2,3]}"#);
    }

    #[test]
    fn pretty_form_indents_by_two() {
        let value = Json::Object(JsonObject::new().with("a", Json::array([Json::Uint(1)])));
        assert_eq!(to_string_pretty(&value), "{\n  \"a\": [\n    1\n  ]\n}");
    }

    #[test]
    fn empty_containers_stay_on_one_line() {
        assert_eq!(to_string_pretty(&Json::Array(vec![])), "[]");
        assert_eq!(to_string_pretty(&Json::Object(JsonObject::new())), "{}");
    }

    #[test]
    fn strings_escape_control_characters() {
        let value = Json::string("a\"b\\c\nd\u{1}");
        assert_eq!(to_string_compact(&value), r#""a\"b\\c\nd\u0001""#);
    }

    #[test]
    fn non_finite_floats_serialise_as_null() {
        assert_eq!(to_string_compact(&Json::Float(f64::NAN)), "null");
        assert_eq!(to_string_compact(&Json::Float(1.5)), "1.5");
        assert_eq!(to_string_compact(&Json::Float(1.0)), "1.0");
    }

    #[test]
    fn serialisation_is_byte_stable() {
        let value = Json::Object(
            JsonObject::new()
                .with("z", Json::Uint(1))
                .with("a", Json::Uint(2)),
        );
        assert_eq!(to_string_compact(&value), to_string_compact(&value.clone()));
        assert_eq!(to_string_compact(&value), r#"{"z":1,"a":2}"#);
    }
}
