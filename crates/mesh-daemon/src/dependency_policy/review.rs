//! Complete snapshot payload validation; this is not native admission or publication authority.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct CompleteReview {
    pub(super) request: RecordDigest,
    pub(super) output: Input,
    pub(super) graph: RecordDigest,
    pub(super) decisions: Vec<(Input, u64, RecordDigest)>,
    pub(super) validation: RecordDigest,
}
impl CompleteReview {
    pub(super) fn decode(body: &Json) -> Result<Self> {
        fields(
            body,
            &[
                "request",
                "revision",
                "output",
                "graph",
                "decisions",
                "validation",
            ],
        )?;
        if number(value(body, "revision")?)? != 1 {
            return Err(InvalidDependencyHistory);
        }
        let request = digest(value(body, "request")?, false)?;
        let output = input(value(body, "output")?)?;
        let graph = digest(value(body, "graph")?, false)?;
        let Json::Array(rows) = value(body, "decisions")? else {
            return Err(InvalidDependencyHistory);
        };
        if rows.len() > MAX_INPUTS {
            return Err(InvalidDependencyHistory);
        }
        let mut decisions = Vec::with_capacity(rows.len());
        for row in rows {
            let Json::Array(parts) = row else {
                return Err(InvalidDependencyHistory);
            };
            if parts.len() != 3 {
                return Err(InvalidDependencyHistory);
            }
            let source = input(&parts[0])?;
            let revision = number(&parts[1])?;
            let record = digest(&parts[2], false)?;
            if source.0 .0 == output.0 .0
                || revision == 0
                || decisions
                    .last()
                    .is_some_and(|(prior, _, _)| *prior >= source)
            {
                return Err(InvalidDependencyHistory);
            }
            decisions.push((source, revision, record));
        }
        let validation = digest(value(body, "validation")?, false)?;
        let expected = Json::object([
            ("schema", Json::text("mesh.native-review-validation/v1")),
            ("output", value(body, "output")?.clone()),
            ("graph", value(body, "graph")?.clone()),
            ("decisions", value(body, "decisions")?.clone()),
        ])
        .encode();
        if Blake3::digest_bytes(expected.as_bytes()).as_bytes() != validation.as_bytes() {
            return Err(InvalidDependencyHistory);
        }
        Ok(Self {
            request,
            output,
            graph,
            decisions,
            validation,
        })
    }
    pub(super) fn references(&self) -> impl Iterator<Item = RecordDigest> + '_ {
        std::iter::once(self.output.1)
            .chain(std::iter::once(self.graph))
            .chain(
                self.decisions
                    .iter()
                    .flat_map(|(input, _, record)| [input.1, *record]),
            )
    }
}

fn input_json(i: Input) -> Json {
    Json::Array(vec![
        Json::Array(vec![
            Json::text(i.0 .0.to_hex()),
            Json::text(i.0 .1.to_hex()),
        ]),
        Json::text(i.1.to_hex()),
    ])
}
impl CompleteReview {
    pub(super) fn body(&self) -> Json {
        Json::object([
            ("request", Json::text(self.request.to_hex())),
            ("revision", Json::Number(1)),
            ("output", input_json(self.output)),
            ("graph", Json::text(self.graph.to_hex())),
            (
                "decisions",
                Json::Array(
                    self.decisions
                        .iter()
                        .map(|(i, revision, record)| {
                            Json::Array(vec![
                                input_json(*i),
                                Json::Number(*revision),
                                Json::text(record.to_hex()),
                            ])
                        })
                        .collect(),
                ),
            ),
            ("validation", Json::text(self.validation.to_hex())),
        ])
    }
    pub(super) fn verify_graph(&self, bytes: &[u8]) -> Result<()> {
        if bytes.len() > 4 * 1024 * 1024
            || Blake3::digest_bytes(bytes).as_bytes() != self.graph.as_bytes()
        {
            return Err(InvalidDependencyHistory);
        }
        let text = std::str::from_utf8(bytes).map_err(|_| InvalidDependencyHistory)?;
        let graph = Json::parse(text).map_err(|_| InvalidDependencyHistory)?;
        if graph.encode() != text {
            return Err(InvalidDependencyHistory);
        }
        fields(&graph, &["schema", "root", "nodes"])?;
        if value(&graph, "schema")?.as_text() != Some("mesh.native-dependency-graph/v1")
            || flat_input(value(&graph, "root")?)? != self.output
        {
            return Err(InvalidDependencyHistory);
        }
        let Json::Array(rows) = value(&graph, "nodes")? else {
            return Err(InvalidDependencyHistory);
        };
        if rows.is_empty() || rows.len() > 4096 {
            return Err(InvalidDependencyHistory);
        }
        let mut nodes = BTreeMap::new();
        let mut edges = 0usize;
        for row in rows {
            fields(
                row,
                &["input", "parents", "manifests", "chunks", "consumption"],
            )?;
            let id = flat_input(value(row, "input")?)?;
            if nodes
                .last_key_value()
                .is_some_and(|(previous, _)| *previous >= id)
            {
                return Err(InvalidDependencyHistory);
            }
            let Json::Array(parents) = value(row, "parents")? else {
                return Err(InvalidDependencyHistory);
            };
            edges = edges
                .checked_add(parents.len())
                .ok_or(InvalidDependencyHistory)?;
            if edges > 16384 {
                return Err(InvalidDependencyHistory);
            }
            let parents = parents.iter().map(flat_input).collect::<Result<Vec<_>>>()?;
            if parents.windows(2).any(|pair| pair[0] >= pair[1]) {
                return Err(InvalidDependencyHistory);
            }
            for field in ["manifests", "chunks"] {
                let Json::Array(values) = value(row, field)? else {
                    return Err(InvalidDependencyHistory);
                };
                let ids = values
                    .iter()
                    .map(|v| digest(v, false))
                    .collect::<Result<Vec<_>>>()?;
                if ids.windows(2).any(|pair| pair[0] >= pair[1]) {
                    return Err(InvalidDependencyHistory);
                }
            }
            if !matches!(value(row, "consumption")?, Json::Null) {
                digest(value(row, "consumption")?, false)?;
            }
            nodes.insert(id, parents);
        }
        let expected = nodes
            .keys()
            .filter(|i| i.0 .0 != self.output.0 .0)
            .copied()
            .collect::<Vec<_>>();
        if expected
            != self
                .decisions
                .iter()
                .map(|(i, _, _)| *i)
                .collect::<Vec<_>>()
        {
            return Err(InvalidDependencyHistory);
        }
        let mut active = BTreeSet::new();
        let mut finished = BTreeSet::new();
        let mut stack = vec![(self.output, false)];
        while let Some((id, finish)) = stack.pop() {
            if finish {
                active.remove(&id);
                finished.insert(id);
                continue;
            }
            if finished.contains(&id) {
                continue;
            }
            if !active.insert(id) {
                return Err(InvalidDependencyHistory);
            }
            let parents = nodes.get(&id).ok_or(InvalidDependencyHistory)?;
            stack.push((id, true));
            stack.extend(parents.iter().rev().map(|parent| (*parent, false)));
        }
        if finished.len() != nodes.len() {
            return Err(InvalidDependencyHistory);
        }
        Ok(())
    }
}
fn flat_input(json: &Json) -> Result<Input> {
    let Json::Array(parts) = json else {
        return Err(InvalidDependencyHistory);
    };
    if parts.len() != 3 {
        return Err(InvalidDependencyHistory);
    }
    Ok(Input(
        Work(digest(&parts[0], false)?, digest(&parts[1], false)?),
        digest(&parts[2], false)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn d(n: u8) -> RecordDigest {
        RecordDigest::from_bytes([n; 32])
    }
    fn i(n: u8) -> Input {
        Input(Work(d(n), d(n + 1)), d(n + 2))
    }
    fn flat(i: Input) -> Json {
        Json::Array(vec![
            Json::text(i.0 .0.to_hex()),
            Json::text(i.0 .1.to_hex()),
            Json::text(i.1.to_hex()),
        ])
    }
    fn node(id: Input, parents: &[Input]) -> Json {
        Json::object([
            ("input", flat(id)),
            (
                "parents",
                Json::Array(parents.iter().map(|i| flat(*i)).collect()),
            ),
            ("manifests", Json::Array(vec![])),
            ("chunks", Json::Array(vec![])),
            ("consumption", Json::Null),
        ])
    }
    fn graph(root: Input, nodes: Vec<Json>) -> Vec<u8> {
        Json::object([
            ("schema", Json::text("mesh.native-dependency-graph/v1")),
            ("root", flat(root)),
            ("nodes", Json::Array(nodes)),
        ])
        .encode()
        .into_bytes()
    }
    fn snapshot(bytes: &[u8]) -> CompleteReview {
        CompleteReview {
            request: d(70),
            output: i(20),
            graph: RecordDigest::from_bytes(*Blake3::digest_bytes(bytes).as_bytes()),
            decisions: vec![(i(10), 1, d(80))],
            validation: d(90),
        }
    }
    #[test]
    fn retained_graph_requires_exact_complete_acyclic_canonical_input_set() {
        let good = graph(i(20), vec![node(i(10), &[]), node(i(20), &[i(10)])]);
        let review = snapshot(&good);
        assert_eq!(review.verify_graph(&good), Ok(()));
        let mut omitted = review.clone();
        omitted.decisions.clear();
        assert_eq!(omitted.verify_graph(&good), Err(InvalidDependencyHistory));
        let mut foreign = review.clone();
        foreign.decisions.push((i(30), 1, d(81)));
        assert_eq!(foreign.verify_graph(&good), Err(InvalidDependencyHistory));
        let mut corrupted = good.clone();
        corrupted[0] ^= 1;
        assert_eq!(
            review.verify_graph(&corrupted),
            Err(InvalidDependencyHistory)
        );
        for bad in [
            graph(i(20), vec![node(i(20), &[i(10)])]),
            graph(i(20), vec![node(i(10), &[i(20)]), node(i(20), &[i(10)])]),
            graph(i(20), vec![node(i(10), &[]), node(i(20), &[])]),
            graph(i(20), vec![node(i(20), &[i(10)]), node(i(10), &[])]),
            graph(
                i(20),
                vec![node(i(10), &[]), node(i(10), &[]), node(i(20), &[i(10)])],
            ),
            graph(i(20), vec![node(i(10), &[]), node(i(20), &[i(10), i(10)])]),
            graph(i(10), vec![node(i(10), &[]), node(i(20), &[i(10)])]),
        ] {
            assert_eq!(
                snapshot(&bad).verify_graph(&bad),
                Err(InvalidDependencyHistory)
            );
        }
    }
}
