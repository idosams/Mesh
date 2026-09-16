//! The JSON value type and its immutable builders.

/// A JSON value.
///
/// Integers are kept in their own variants rather than collapsed into `f64`:
/// sample durations are nanosecond counts, and a benchmark that silently loses
/// integer precision on its raw results is not a benchmark.
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    /// JSON `null`.
    Null,
    /// JSON `true` / `false`.
    Bool(bool),
    /// A non-negative integer.
    Uint(u64),
    /// A negative integer.
    Int(i64),
    /// A number with a fraction or exponent.
    Float(f64),
    /// A string.
    Str(String),
    /// An array.
    Array(Vec<Json>),
    /// An object, in insertion order.
    Object(JsonObject),
}

impl Json {
    /// Builds a string value from anything string-like.
    pub fn string(value: impl Into<String>) -> Self {
        Json::Str(value.into())
    }

    /// Builds an array from an iterator of values.
    pub fn array(values: impl IntoIterator<Item = Json>) -> Self {
        Json::Array(values.into_iter().collect())
    }

    /// The object behind this value, if it is one.
    pub fn as_object(&self) -> Option<&JsonObject> {
        match self {
            Json::Object(object) => Some(object),
            _ => None,
        }
    }

    /// The array behind this value, if it is one.
    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Array(items) => Some(items),
            _ => None,
        }
    }

    /// The string behind this value, if it is one.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(text) => Some(text),
            _ => None,
        }
    }

    /// The boolean behind this value, if it is one.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(flag) => Some(*flag),
            _ => None,
        }
    }

    /// The non-negative integer behind this value, if it is one.
    ///
    /// A float is never coerced: `20.0` is not a sample count.
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Json::Uint(number) => Some(*number),
            _ => None,
        }
    }

    /// The name of this value's type, for error messages.
    pub fn type_name(&self) -> &'static str {
        match self {
            Json::Null => "null",
            Json::Bool(_) => "boolean",
            Json::Uint(_) | Json::Int(_) | Json::Float(_) => "number",
            Json::Str(_) => "string",
            Json::Array(_) => "array",
            Json::Object(_) => "object",
        }
    }

    /// Looks a dotted path (`hardware.cpu_model`) up in nested objects.
    pub fn get_path(&self, path: &str) -> Option<&Json> {
        let mut current = self;
        for segment in path.split('.') {
            current = current.as_object()?.get(segment)?;
        }
        Some(current)
    }

    /// Returns a copy of this value with the dotted path removed.
    ///
    /// Used by the schema self-check and its tests to manufacture a row that is
    /// missing exactly one required field. Removing a path that is not present
    /// returns an equal copy — the caller asserts on the decode outcome, not on
    /// this call.
    pub fn without_path(&self, path: &str) -> Json {
        let Some((head, rest)) = split_path(path) else {
            return self.clone();
        };
        let Json::Object(object) = self else {
            return self.clone();
        };
        match rest {
            None => Json::Object(object.without(head)),
            Some(tail) => match object.get(head) {
                None => self.clone(),
                Some(child) => Json::Object(object.clone().with(head, child.without_path(tail))),
            },
        }
    }
}

fn split_path(path: &str) -> Option<(&str, Option<&str>)> {
    if path.is_empty() {
        return None;
    }
    match path.split_once('.') {
        Some((head, tail)) => Some((head, Some(tail))),
        None => Some((path, None)),
    }
}

/// A JSON object that preserves insertion order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct JsonObject {
    entries: Vec<(String, Json)>,
}

impl JsonObject {
    /// An empty object.
    pub fn new() -> Self {
        JsonObject {
            entries: Vec::new(),
        }
    }

    /// Returns a new object with `key` set to `value`.
    ///
    /// An existing key keeps its position, so a row rebuilt field by field
    /// serialises identically however it was assembled.
    #[must_use]
    pub fn with(mut self, key: impl Into<String>, value: Json) -> Self {
        let key = key.into();
        match self.entries.iter_mut().find(|(name, _)| *name == key) {
            Some(entry) => entry.1 = value,
            None => self.entries.push((key, value)),
        }
        self
    }

    /// Returns a new object without `key`.
    #[must_use]
    pub fn without(&self, key: &str) -> Self {
        JsonObject {
            entries: self
                .entries
                .iter()
                .filter(|(name, _)| name != key)
                .cloned()
                .collect(),
        }
    }

    /// The value stored under `key`, if any.
    pub fn get(&self, key: &str) -> Option<&Json> {
        self.entries
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value)
    }

    /// The entries, in insertion order.
    pub fn entries(&self) -> &[(String, Json)] {
        &self.entries
    }

    /// The number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the object has no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl From<JsonObject> for Json {
    fn from(object: JsonObject) -> Self {
        Json::Object(object)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Json {
        Json::Object(
            JsonObject::new()
                .with("top", Json::Uint(1))
                .with(
                    "nested",
                    Json::Object(
                        JsonObject::new()
                            .with("kept", Json::string("yes"))
                            .with("dropped", Json::Bool(true)),
                    ),
                )
                .with("list", Json::array([Json::Uint(1), Json::Uint(2)])),
        )
    }

    #[test]
    fn get_path_walks_nested_objects() {
        assert_eq!(sample().get_path("nested.kept"), Some(&Json::string("yes")));
        assert_eq!(sample().get_path("nested.missing"), None);
        assert_eq!(sample().get_path("list.0"), None);
    }

    #[test]
    fn without_path_removes_only_the_named_field() {
        let trimmed = sample().without_path("nested.dropped");
        assert_eq!(trimmed.get_path("nested.dropped"), None);
        assert_eq!(trimmed.get_path("nested.kept"), Some(&Json::string("yes")));
        assert_eq!(trimmed.get_path("top"), Some(&Json::Uint(1)));
    }

    #[test]
    fn without_path_is_immutable() {
        let original = sample();
        let _ = original.without_path("top");
        assert_eq!(original.get_path("top"), Some(&Json::Uint(1)));
    }

    #[test]
    fn with_replaces_in_place_and_keeps_order() {
        let object = JsonObject::new()
            .with("a", Json::Uint(1))
            .with("b", Json::Uint(2))
            .with("a", Json::Uint(3));
        let keys: Vec<&str> = object
            .entries()
            .iter()
            .map(|(key, _)| key.as_str())
            .collect();
        assert_eq!(keys, vec!["a", "b"]);
        assert_eq!(object.get("a"), Some(&Json::Uint(3)));
    }

    #[test]
    fn as_u64_does_not_coerce_floats() {
        assert_eq!(Json::Float(20.0).as_u64(), None);
        assert_eq!(Json::Uint(20).as_u64(), Some(20));
    }
}
