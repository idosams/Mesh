//! Read-only validation of native dependency payload history.
//!
//! A valid projection is not an access or publication capability. The caller must obtain the
//! expected project binding from native registration, never from imported bytes. Native control
//! authorization, old-writer fencing, source ancestry and full closure validation remain separate.
use crate::ipc::Json;
use mesh_cas::{Blake3, ContentDigest};
use mesh_store::{DependencyKind, DependencyRecord, RecordDigest};
use std::collections::{BTreeMap, BTreeSet};

const SCHEMA: &str = "mesh.dependency-policy/v1";
const MAX_BYTES: usize = 65_536;
const MAX_INPUTS: usize = 256;
const MAX_RECORDS: usize = 8192;
const MAX_HISTORY_BYTES: usize = 16 * 1024 * 1024;
const ZERO: RecordDigest = RecordDigest::from_bytes([0; 32]);

/// Expected identity obtained independently from the owning native registration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeDependencyBinding {
    /// Local authority identity; readable imported records cannot select this value.
    pub authority: RecordDigest,
    /// Stable registered project identity.
    pub project: RecordDigest,
    /// Exact private-history installation identity.
    pub installation: RecordDigest,
}

/// Invalid, contradictory, noncanonical or oversized history. Contains no private payload text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidDependencyHistory;
impl std::fmt::Display for InvalidDependencyHistory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("native dependency history is invalid or incomplete")
    }
}
impl std::error::Error for InvalidDependencyHistory {}
type Result<T> = std::result::Result<T, InvalidDependencyHistory>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Work(RecordDigest, RecordDigest);
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Input(Work, RecordDigest);
#[derive(Clone, Debug, PartialEq, Eq)]
enum Event {
    Enrollment,
    Grant {
        source: Input,
        destination: Work,
        generation: u64,
        previous: RecordDigest,
        allowed: bool,
        bindings: Option<(RecordDigest, RecordDigest)>,
    },
    Consumption {
        grant: RecordDigest,
        start: Input,
        inputs: Vec<Input>,
    },
    Eligibility {
        input: Input,
        revision: u64,
        previous: RecordDigest,
        state: String,
        replacement: Option<Input>,
    },
    Review {
        output: Input,
        decisions: Vec<(Input, RecordDigest)>,
    },
}

/// Bounded, reconstructible historical projection. It grants no permission to consume or publish.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DependencyPolicyHistory {
    binding: NativeDependencyBinding,
    records: BTreeMap<RecordDigest, (DependencyRecord, Event)>,
    requests: BTreeMap<RecordDigest, RecordDigest>,
    grants: BTreeMap<(Input, Work), (u64, RecordDigest, bool)>,
    decisions: BTreeMap<Input, (u64, RecordDigest, bool)>,
    roots: BTreeSet<RecordDigest>,
    head: Option<(u64, RecordDigest)>,
    payload_bytes: usize,
}
impl DependencyPolicyHistory {
    /// Start replay against independently pinned native identity, not an identity in the payload.
    pub fn new(binding: NativeDependencyBinding) -> Result<Self> {
        if [binding.authority, binding.project, binding.installation].contains(&ZERO) {
            return Err(InvalidDependencyHistory);
        }
        Ok(Self {
            binding,
            records: BTreeMap::new(),
            requests: BTreeMap::new(),
            grants: BTreeMap::new(),
            decisions: BTreeMap::new(),
            roots: BTreeSet::new(),
            head: None,
            payload_bytes: 0,
        })
    }

    /// Validate one canonical payload and envelope before changing any projection state.
    ///
    /// Exact replay is idempotent. No caller authority, source ancestry, complete transitive
    /// closure or present publication eligibility is established by this read-only operation.
    pub fn apply(&mut self, envelope: DependencyRecord, bytes: &[u8]) -> Result<()> {
        if bytes.len() > MAX_BYTES
            || bytes.is_empty()
            || envelope.authority != self.binding.authority
            || envelope.revision == 0
            || envelope.revision > i64::MAX as u64
            || envelope.payload == ZERO
            || Blake3::digest_bytes(bytes).as_bytes() != envelope.payload.as_bytes()
        {
            return Err(InvalidDependencyHistory);
        }
        let text = std::str::from_utf8(bytes).map_err(|_| InvalidDependencyHistory)?;
        let json = Json::parse(text).map_err(|_| InvalidDependencyHistory)?;
        if json.encode() != text {
            return Err(InvalidDependencyHistory);
        }
        fields(
            &json,
            &[
                "schema",
                "authority",
                "revision",
                "previous",
                "kind",
                "body",
            ],
        )?;
        let bound_grant = value(&json, "schema")?.as_text() == Some("mesh.dependency-policy/v2")
            && envelope.kind == DependencyKind::Grant;
        if (!bound_grant && value(&json, "schema")?.as_text() != Some(SCHEMA))
            || digest(value(&json, "authority")?, false)? != envelope.authority
            || number(value(&json, "revision")?)? != envelope.revision
            || digest(value(&json, "previous")?, true)? != envelope.previous
            || value(&json, "kind")?.as_u64() != Some(u64::from(envelope.kind.code()))
        {
            return Err(InvalidDependencyHistory);
        }
        let (request, event, roots) = decode(
            value(&json, "body")?,
            envelope.kind,
            self.binding,
            bound_grant,
        )?;
        if let Some((old, prior)) = self.records.get(&envelope.payload) {
            return if *old == envelope && *prior == event {
                Ok(())
            } else {
                Err(InvalidDependencyHistory)
            };
        }
        if self.payload_bytes.saturating_add(bytes.len()) > MAX_HISTORY_BYTES
            || self.records.len() >= MAX_RECORDS
            || request.is_some_and(|id| self.requests.contains_key(&id))
        {
            return Err(InvalidDependencyHistory);
        }
        match self.head {
            None if envelope.revision == 1
                && envelope.previous == ZERO
                && event == Event::Enrollment => {}
            Some((revision, previous))
                if revision.checked_add(1) == Some(envelope.revision)
                    && previous == envelope.previous
                    && event != Event::Enrollment => {}
            _ => return Err(InvalidDependencyHistory),
        }
        // Complete every fallible semantic check before mutating any map or retained reference.
        match &event {
            Event::Enrollment => {}
            Event::Grant {
                source,
                destination,
                generation,
                previous,
                ..
            } => {
                let expected = self.grants.get(&(*source, *destination));
                if !next_revision(expected.map(|(r, p, _)| (*r, *p)), *generation, *previous) {
                    return Err(InvalidDependencyHistory);
                }
            }
            Event::Consumption {
                grant,
                start,
                inputs,
            } => {
                let Some((
                    _,
                    Event::Grant {
                        source,
                        destination,
                        generation,
                        allowed: true,
                        ..
                    },
                )) = self.records.get(grant)
                else {
                    return Err(InvalidDependencyHistory);
                };
                if start.0 != *destination
                    || inputs.binary_search(source).is_err()
                    || self.grants.get(&(*source, *destination))
                        != Some(&(*generation, *grant, true))
                {
                    return Err(InvalidDependencyHistory);
                }
            }
            Event::Eligibility {
                input,
                revision,
                previous,
                ..
            } => {
                if !next_revision(
                    self.decisions.get(input).map(|(r, p, _)| (*r, *p)),
                    *revision,
                    *previous,
                ) {
                    return Err(InvalidDependencyHistory);
                }
            }
            Event::Review { decisions, .. } => {
                for (input, decision) in decisions {
                    if !self
                        .decisions
                        .get(input)
                        .is_some_and(|(_, p, eligible)| p == decision && *eligible)
                    {
                        return Err(InvalidDependencyHistory);
                    }
                }
            }
        }
        match &event {
            Event::Grant {
                source,
                destination,
                generation,
                allowed,
                ..
            } => {
                self.grants.insert(
                    (*source, *destination),
                    (*generation, envelope.payload, *allowed),
                );
            }
            Event::Eligibility {
                input,
                revision,
                state,
                ..
            } => {
                self.decisions
                    .insert(*input, (*revision, envelope.payload, state == "eligible"));
            }
            _ => {}
        }
        if let Some(request) = request {
            self.requests.insert(request, envelope.payload);
        }
        self.payload_bytes += bytes.len();
        self.roots.extend(roots);
        self.roots.insert(envelope.payload);
        self.head = Some((envelope.revision, envelope.payload));
        self.records.insert(envelope.payload, (envelope, event));
        Ok(())
    }

    pub(crate) fn native_head(&self) -> Option<(u64, RecordDigest)> {
        self.head
    }
    pub(crate) fn native_request(&self, request: RecordDigest) -> Option<DependencyRecord> {
        self.requests
            .get(&request)
            .and_then(|id| self.records.get(id))
            .map(|(record, _)| *record)
    }
    pub(crate) fn native_grant(
        &self,
        source: (RecordDigest, RecordDigest, RecordDigest),
        destination: (RecordDigest, RecordDigest),
    ) -> Option<(u64, RecordDigest)> {
        self.grants
            .get(&(
                Input(Work(source.0, source.1), source.2),
                Work(destination.0, destination.1),
            ))
            .map(|(generation, payload, _)| (*generation, *payload))
    }

    pub(crate) fn current_bound_grant(
        &self,
        source: (RecordDigest, RecordDigest, RecordDigest),
        destination: (RecordDigest, RecordDigest),
        expected: RecordDigest,
        bindings: (RecordDigest, RecordDigest),
    ) -> bool {
        let key = (
            Input(Work(source.0, source.1), source.2),
            Work(destination.0, destination.1),
        );
        matches!(self.grants.get(&key), Some((_, current, true)) if *current == expected)
            && matches!(self.records.get(&expected), Some((_, Event::Grant { bindings: Some(recorded), .. })) if *recorded == bindings)
    }

    pub(crate) fn native_decision(
        &self,
        work: RecordDigest,
        installation: RecordDigest,
        version: RecordDigest,
    ) -> Option<(u64, RecordDigest)> {
        self.decisions
            .get(&Input(Work(work, installation), version))
            .map(|(revision, payload, _)| (*revision, *payload))
    }

    /// Direct immutable references only; not a complete retention closure or a collection oracle.
    pub fn referenced_content(&self) -> impl Iterator<Item = &RecordDigest> {
        self.roots.iter()
    }
    /// Number of distinct historical payloads after idempotent replay.
    #[must_use]
    pub fn len(&self) -> usize {
        self.records.len()
    }
    /// Whether enrollment has not yet been replayed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

fn next_revision(
    prior: Option<(u64, RecordDigest)>,
    revision: u64,
    previous: RecordDigest,
) -> bool {
    match prior {
        None => revision == 1 && previous == ZERO,
        Some((old, hash)) => old.checked_add(1) == Some(revision) && hash == previous,
    }
}
fn fields(json: &Json, names: &[&str]) -> Result<()> {
    let Json::Object(values) = json else {
        return Err(InvalidDependencyHistory);
    };
    if values.len() != names.len()
        || values
            .iter()
            .zip(names)
            .any(|((name, _), expected)| name != expected)
    {
        return Err(InvalidDependencyHistory);
    }
    Ok(())
}
fn value<'a>(json: &'a Json, name: &str) -> Result<&'a Json> {
    json.get(name).ok_or(InvalidDependencyHistory)
}
fn number(json: &Json) -> Result<u64> {
    json.as_u64()
        .filter(|n| *n > 0 && *n <= i64::MAX as u64)
        .ok_or(InvalidDependencyHistory)
}
fn digest(json: &Json, zero: bool) -> Result<RecordDigest> {
    let text = json.as_text().ok_or(InvalidDependencyHistory)?;
    let d = RecordDigest::parse_hex(text).map_err(|_| InvalidDependencyHistory)?;
    if d.to_hex() != text || (!zero && d == ZERO) {
        return Err(InvalidDependencyHistory);
    }
    Ok(d)
}
fn work(json: &Json) -> Result<Work> {
    let Json::Array(values) = json else {
        return Err(InvalidDependencyHistory);
    };
    if values.len() != 2 {
        return Err(InvalidDependencyHistory);
    }
    Ok(Work(digest(&values[0], false)?, digest(&values[1], false)?))
}
fn input(json: &Json) -> Result<Input> {
    let Json::Array(values) = json else {
        return Err(InvalidDependencyHistory);
    };
    if values.len() != 2 {
        return Err(InvalidDependencyHistory);
    }
    Ok(Input(work(&values[0])?, digest(&values[1], false)?))
}
fn inputs(json: &Json) -> Result<Vec<Input>> {
    let Json::Array(values) = json else {
        return Err(InvalidDependencyHistory);
    };
    if values.len() > MAX_INPUTS {
        return Err(InvalidDependencyHistory);
    }
    let result: Vec<_> = values.iter().map(input).collect::<Result<_>>()?;
    if result.windows(2).any(|p| p[0] >= p[1]) {
        return Err(InvalidDependencyHistory);
    }
    Ok(result)
}
fn decode(
    body: &Json,
    kind: DependencyKind,
    binding: NativeDependencyBinding,
    bound_grant: bool,
) -> Result<(Option<RecordDigest>, Event, BTreeSet<RecordDigest>)> {
    let mut roots = BTreeSet::new();
    let mut request = None;
    let event = match kind {
        DependencyKind::Enrollment => {
            fields(body, &["project", "installation"])?;
            if digest(value(body, "project")?, false)? != binding.project
                || digest(value(body, "installation")?, false)? != binding.installation
            {
                return Err(InvalidDependencyHistory);
            }
            Event::Enrollment
        }
        DependencyKind::Grant => {
            let mut names = vec![
                "request",
                "source",
                "destination",
                "generation",
                "previous",
                "allowed",
            ];
            if bound_grant {
                names.push("bindings");
            }
            fields(body, &names)?;
            let bindings = if bound_grant {
                let Json::Array(values) = value(body, "bindings")? else {
                    return Err(InvalidDependencyHistory);
                };
                if values.len() != 2 {
                    return Err(InvalidDependencyHistory);
                }
                Some((digest(&values[0], false)?, digest(&values[1], false)?))
            } else {
                None
            };
            request = Some(digest(value(body, "request")?, false)?);
            let source = input(value(body, "source")?)?;
            let destination = work(value(body, "destination")?)?;
            if source.0 .0 == destination.0 {
                return Err(InvalidDependencyHistory);
            }
            let allowed = match value(body, "allowed")? {
                Json::Bool(v) => *v,
                _ => return Err(InvalidDependencyHistory),
            };
            roots.insert(source.1);
            let previous = digest(value(body, "previous")?, true)?;
            if previous != ZERO {
                roots.insert(previous);
            }
            Event::Grant {
                source,
                destination,
                generation: number(value(body, "generation")?)?,
                previous,
                allowed,
                bindings,
            }
        }
        DependencyKind::Consumption => {
            fields(body, &["request", "grant", "start", "inputs"])?;
            request = Some(digest(value(body, "request")?, false)?);
            let grant = digest(value(body, "grant")?, false)?;
            let start = input(value(body, "start")?)?;
            let inputs = inputs(value(body, "inputs")?)?;
            if inputs.is_empty() || inputs.iter().any(|i| i.0 .0 == start.0 .0) {
                return Err(InvalidDependencyHistory);
            }
            roots.insert(grant);
            roots.insert(start.1);
            roots.extend(inputs.iter().map(|i| i.1));
            Event::Consumption {
                grant,
                start,
                inputs,
            }
        }
        DependencyKind::Eligibility => {
            fields(
                body,
                &[
                    "request",
                    "input",
                    "revision",
                    "previous",
                    "state",
                    "replacement",
                ],
            )?;
            request = Some(digest(value(body, "request")?, false)?);
            let input = input(value(body, "input")?)?;
            let state = value(body, "state")?
                .as_text()
                .ok_or(InvalidDependencyHistory)?
                .to_owned();
            let replacement = match value(body, "replacement")? {
                Json::Null => None,
                v => Some(self::input(v)?),
            };
            if !["eligible", "rejected", "replaced"].contains(&state.as_str())
                || (state == "replaced") != replacement.is_some()
                || replacement == Some(input)
            {
                return Err(InvalidDependencyHistory);
            }
            roots.insert(input.1);
            if let Some(v) = replacement {
                roots.insert(v.1);
            }
            let previous = digest(value(body, "previous")?, true)?;
            if previous != ZERO {
                roots.insert(previous);
            }
            Event::Eligibility {
                input,
                revision: number(value(body, "revision")?)?,
                previous,
                state,
                replacement,
            }
        }
        DependencyKind::ReviewSnapshot => {
            fields(body, &["request", "output", "decisions"])?;
            request = Some(digest(value(body, "request")?, false)?);
            let output = input(value(body, "output")?)?;
            let Json::Array(rows) = value(body, "decisions")? else {
                return Err(InvalidDependencyHistory);
            };
            if rows.len() > MAX_INPUTS {
                return Err(InvalidDependencyHistory);
            }
            let mut decisions = Vec::new();
            for row in rows {
                let Json::Array(pair) = row else {
                    return Err(InvalidDependencyHistory);
                };
                if pair.len() != 2 {
                    return Err(InvalidDependencyHistory);
                }
                let i = input(&pair[0])?;
                let d = digest(&pair[1], false)?;
                if i.0 .0 == output.0 .0 || decisions.last().is_some_and(|(old, _)| *old >= i) {
                    return Err(InvalidDependencyHistory);
                }
                decisions.push((i, d));
                roots.insert(i.1);
                roots.insert(d);
            }
            roots.insert(output.1);
            Event::Review { output, decisions }
        }
    };
    Ok((request, event, roots))
}

#[cfg(test)]
mod tests;
