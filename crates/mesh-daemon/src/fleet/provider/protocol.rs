//! Bounded provider event decoding, separate from Mesh's integer-only IPC protocol.
//!
//! Numbers and message bodies are never observations. Decimal usage/cost fields are valid
//! provider JSON; malformed events, ambiguous objects and explicit errors fail closed.
use std::fmt;

use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};

use crate::ipc::Json;

/// Supported external event formats. This does not admit or launch an executable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderProtocol {
    /// Codex exec JSON events.
    Codex,
    /// Claude Code print stream-json events.
    Claude,
}

/// Bounded activity metadata. Provider message bodies and raw stderr are not retained here.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProviderObservation {
    /// Provider-assigned thread/session identity, when acknowledged.
    pub thread: Option<String>,
    /// Number of stdout events consumed, including ignored future event types.
    pub events: u64,
    /// Number of stderr lines consumed; these are not necessarily failures.
    pub stderr_lines: u64,
    /// Last recognized activity category, with no command text or file contents.
    pub activity: Option<String>,
    /// A completed turn was reported by the provider.
    pub turn_completed: bool,
    /// Explicit provider failure, malformed output or transport-reader failure.
    pub failed: bool,
    /// Both pipes closed. Process exit alone does not imply this.
    pub streams_closed: bool,
}

impl ProviderProtocol {
    /// Consume one bounded stdout event, retaining only fixed categories and a validated UUID.
    /// Failure is sticky; a later successful result cannot erase an earlier error. Completion
    /// here is a protocol fact only, never process success, custody release or review approval.
    pub fn observe(self, bytes: &[u8], observation: &mut ProviderObservation) {
        observation.events = observation.events.saturating_add(1);
        let Some(event) = decode(bytes) else {
            observation.failed = true;
            return;
        };
        let kind = event.get("type").and_then(Json::as_text);
        match (self, kind) {
            (Self::Codex, Some("thread.started")) => {
                identity(event.get("thread_id"), observation);
            }
            (Self::Codex, Some("turn.completed")) => observation.turn_completed = true,
            (_, Some("error")) | (Self::Codex, Some("turn.failed")) => observation.failed = true,
            (
                Self::Codex,
                Some(kind @ ("turn.started" | "item.started" | "item.updated" | "item.completed")),
            ) => {
                observation.activity = Some(kind.into());
            }
            (Self::Claude, Some("system")) => {
                if event.get("subtype").and_then(Json::as_text) == Some("init") {
                    identity(event.get("session_id"), observation);
                    observation.activity = Some("session.started".into());
                }
            }
            (Self::Claude, Some("assistant")) => {
                if event
                    .get("error")
                    .is_some_and(|value| !matches!(value, Json::Null))
                {
                    observation.failed = true;
                }
                observation.activity = Some("assistant.message".into());
            }
            (Self::Claude, Some("result")) => {
                // Observed Claude authentication failures also use subtype=success.
                // Require an explicit false is_error; missing/wrong types are not success.
                if event.get("subtype").and_then(Json::as_text) == Some("success")
                    && event.get("is_error").and_then(Json::as_bool) == Some(false)
                {
                    observation.turn_completed = true;
                } else {
                    observation.failed = true;
                }
            }
            (_, Some(_)) => {} // Future events never grant success or authority.
            (_, None) => observation.failed = true,
        }
    }
}

fn identity(value: Option<&Json>, observation: &mut ProviderObservation) {
    let valid = value.and_then(Json::as_text).filter(|id| {
        id.len() == 36
            && id.bytes().enumerate().all(|(index, b)| {
                if [8, 13, 18, 23].contains(&index) {
                    b == b'-'
                } else {
                    b.is_ascii_hexdigit()
                }
            })
    });
    match valid {
        Some(id)
            if observation
                .thread
                .as_deref()
                .is_none_or(|previous| previous == id) =>
        {
            observation.thread = Some(id.into());
        }
        _ => observation.failed = true,
    }
}

fn decode(bytes: &[u8]) -> Option<Json> {
    if bytes.len() > super::MAX_EVENT_BYTES {
        return None;
    }
    let mut decoder = serde_json::Deserializer::from_slice(bytes);
    let value = EventValue(0).deserialize(&mut decoder).ok()?;
    decoder.end().ok()?;
    matches!(value, Json::Object(_)).then_some(value)
}

// Preserve duplicate-key rejection and the existing depth bound while accepting full JSON
// numbers. Numeric values are redacted to a type marker; none grants protocol success/identity.
struct EventValue(usize);
impl<'de> DeserializeSeed<'de> for EventValue {
    type Value = Json;
    fn deserialize<D: de::Deserializer<'de>>(self, deserializer: D) -> Result<Json, D::Error> {
        if self.0 > crate::ipc::json::MAX_DEPTH {
            return Err(de::Error::custom("provider event nesting limit"));
        }
        deserializer.deserialize_any(self)
    }
}
impl<'de> Visitor<'de> for EventValue {
    type Value = Json;
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a bounded provider event")
    }
    fn visit_unit<E: de::Error>(self) -> Result<Json, E> {
        Ok(Json::Null)
    }
    fn visit_bool<E: de::Error>(self, value: bool) -> Result<Json, E> {
        Ok(Json::Bool(value))
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> Result<Json, E> {
        Ok(Json::Number(0))
    }
    fn visit_u64<E: de::Error>(self, _: u64) -> Result<Json, E> {
        Ok(Json::Number(0))
    }
    fn visit_f64<E: de::Error>(self, _: f64) -> Result<Json, E> {
        Ok(Json::Number(0))
    }
    fn visit_str<E: de::Error>(self, value: &str) -> Result<Json, E> {
        Ok(Json::text(value))
    }
    fn visit_string<E: de::Error>(self, value: String) -> Result<Json, E> {
        Ok(Json::Text(value))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Json, A::Error> {
        // Validate bodies without retaining arrays of provider content.
        while sequence
            .next_element_seed(EventValue(self.0 + 1))?
            .is_some()
        {}
        Ok(Json::Array(Vec::new()))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Json, A::Error> {
        let mut values = std::collections::BTreeMap::new();
        while let Some(key) = map.next_key::<String>()? {
            if values.contains_key(&key) {
                return Err(de::Error::custom("duplicate provider event key"));
            }
            values.insert(key, map.next_value_seed(EventValue(self.0 + 1))?);
        }
        Ok(Json::Object(values.into_iter().collect()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn observe(protocol: ProviderProtocol, events: &[&[u8]]) -> ProviderObservation {
        let mut observation = ProviderObservation::default();
        for event in events {
            protocol.observe(event, &mut observation);
        }
        observation
    }
    #[test]
    fn external_numbers_do_not_change_mesh_ipc_or_grant_authority() {
        for protocol in [ProviderProtocol::Codex, ProviderProtocol::Claude] {
            let observation = observe(
                protocol,
                &[br#"{"type":"future","cost":0.052,"delta":-1,"usage":1e3,"body":"private"}"#],
            );
            assert!(!observation.failed);
            assert!(!observation.turn_completed);
            assert!(!format!("{observation:?}").contains("private"));
        }
        assert!(Json::parse(r#"{"cost":0.052}"#).is_err());
    }
    #[test]
    fn claude_error_dominates_success_subtype_and_later_results() {
        let success =
            br#"{"type":"result","subtype":"success","is_error":false,"total_cost_usd":0.002}"#;
        assert!(observe(ProviderProtocol::Claude, &[success]).turn_completed);
        for error in [
            br#"{"type":"assistant","error":"authentication_failed","message":"private"}"#
                .as_slice(),
            br#"{"type":"result","subtype":"success","is_error":true}"#,
            br#"{"type":"assistant","error":0}"#,
            br#"{"type":"assistant","error":[]}"#,
            br#"{"type":"result","subtype":"error_max_turns","is_error":false}"#,
            br#"{"type":"result","subtype":"success"}"#,
            br#"{"type":"result","subtype":"success","is_error":0}"#,
        ] {
            for events in [[error, success.as_slice()], [success.as_slice(), error]] {
                let observation = observe(ProviderProtocol::Claude, &events);
                assert!(observation.failed);
                assert!(!format!("{observation:?}").contains("private"));
            }
        }
    }
    #[test]
    fn shared_conformance_rejects_malformed_ambiguous_or_unbounded_events() {
        let depth = format!(
            r#"{{"type":"future","body":{}0{}}}"#,
            "[".repeat(18),
            "]".repeat(18)
        );
        for protocol in [ProviderProtocol::Codex, ProviderProtocol::Claude] {
            for input in [
                b"not-json".as_slice(),
                b"[]",
                b"{}",
                b"{\"type\":3}",
                br#"{"type":"error","type":"future"}"#,
                br#"{"type":"future","nested":{"a":0,"a":1}}"#,
                br#"{"type":"future"} {}"#,
                br#"{"type":"future","number":NaN}"#,
                depth.as_bytes(),
                &[0xff],
                &vec![b' '; super::super::MAX_EVENT_BYTES + 1],
            ] {
                assert!(observe(protocol, &[input]).failed);
            }
            assert!(!observe(protocol, &[br#"{"type":"future"}"#]).turn_completed);
        }
    }
    #[test]
    fn session_identity_is_validated_and_cannot_be_replaced() {
        for (protocol, prefix, field) in [
            (
                ProviderProtocol::Codex,
                r#""type":"thread.started""#,
                "thread_id",
            ),
            (
                ProviderProtocol::Claude,
                r#""type":"system","subtype":"init""#,
                "session_id",
            ),
        ] {
            let first = format!(r#"{{{prefix},"{field}":"01234567-89ab-cdef-0123-456789abcdef"}}"#);
            let second =
                format!(r#"{{{prefix},"{field}":"11234567-89ab-cdef-0123-456789abcdef"}}"#);
            let observation = observe(protocol, &[first.as_bytes(), second.as_bytes()]);
            assert!(observation.failed);
            assert_eq!(
                observation.thread.as_deref(),
                Some("01234567-89ab-cdef-0123-456789abcdef")
            );
            assert!(!observe(protocol, &[first.as_bytes(), first.as_bytes()]).failed);
            let invalid = format!(r#"{{{prefix},"{field}":"private"}}"#);
            assert!(observe(protocol, &[invalid.as_bytes()]).failed);
        }
    }
}
