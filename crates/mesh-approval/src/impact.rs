//! Dependency impact: which derived outputs the actor's work made stale.
//!
//! # Why it is in the bundle
//!
//! Plan §4.8's last row — "context input changed → mark affected outputs stale" — is a fact about
//! the work being reviewed, not about the moment somebody reads it. A reviewer who is shown three
//! changed files and not told that six generated artifacts no longer follow from them is being
//! shown a partial truth, and the approval binds what was shown. So the impact is computed once,
//! into the bundle, and absorbed into its identity.
//!
//! # It is transitive, and that is the point
//!
//! An output can be another output's input. [`DependencyGraph::stale_outputs`] walks the closure,
//! so a change three hops upstream still surfaces. The walk is over ordered sets and terminates on
//! a cycle by construction — an object already visited is never queued again — because a
//! dependency graph handed in from outside is not this crate's to trust.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::digest::{Absorb, DigestHasher, DigestWriter};
use crate::ids::ObjectId;

/// Which objects each derived output is derived from.
///
/// Immutable: every method that adds an edge consumes the graph and returns a new one, so an impact
/// computation cannot see a graph change underneath it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DependencyGraph {
    inputs_of: BTreeMap<ObjectId, BTreeSet<ObjectId>>,
}

impl DependencyGraph {
    /// A graph with no edges.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inputs_of: BTreeMap::new(),
        }
    }

    /// This graph with `output` additionally derived from `input`.
    #[must_use]
    pub fn with_edge(self, output: ObjectId, input: ObjectId) -> Self {
        let mut inputs_of = self.inputs_of;
        inputs_of.entry(output).or_default().insert(input);
        Self { inputs_of }
    }

    /// What `output` is derived from, in identifier order.
    #[must_use]
    pub fn inputs_of(&self, output: ObjectId) -> Vec<ObjectId> {
        self.inputs_of
            .get(&output)
            .map(|inputs| inputs.iter().copied().collect())
            .unwrap_or_default()
    }

    /// How many outputs the graph knows about.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inputs_of.len()
    }

    /// Whether the graph has no edges at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inputs_of.is_empty()
    }

    /// Every output made stale by a change to any of `changed`, transitively.
    ///
    /// An output that is itself changed is still reported: a generated artifact somebody edited by
    /// hand and whose input then moved is exactly the case a reviewer must be told about.
    #[must_use]
    pub fn stale_outputs(&self, changed: &BTreeSet<ObjectId>) -> Vec<StaleOutput> {
        let mut stale: BTreeMap<ObjectId, BTreeSet<ObjectId>> = BTreeMap::new();
        let mut dirty: BTreeSet<ObjectId> = changed.clone();
        let mut queue: VecDeque<ObjectId> = changed.iter().copied().collect();

        while let Some(input) = queue.pop_front() {
            for (output, inputs) in &self.inputs_of {
                if !inputs.contains(&input) {
                    continue;
                }
                stale.entry(*output).or_default().insert(input);
                if dirty.insert(*output) {
                    queue.push_back(*output);
                }
            }
        }

        stale
            .into_iter()
            .map(|(output, because)| StaleOutput {
                output,
                because: because.into_iter().collect(),
            })
            .collect()
    }
}

/// One derived output the reviewed work made stale, and the inputs that made it so.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StaleOutput {
    output: ObjectId,
    because: Vec<ObjectId>,
}

impl StaleOutput {
    /// The output that no longer follows from its inputs.
    #[must_use]
    pub const fn output(&self) -> ObjectId {
        self.output
    }

    /// The changed inputs that made it stale, in identifier order.
    ///
    /// Only the *direct* inputs that were themselves dirtied; a three-hop chain names the hop above
    /// it, so a reviewer can walk the chain rather than being handed its transitive closure flat.
    #[must_use]
    pub fn because(&self) -> &[ObjectId] {
        &self.because
    }
}

impl Absorb for StaleOutput {
    fn absorb<H: DigestHasher>(&self, writer: &mut DigestWriter<H>) {
        writer.bytes(self.output.as_bytes());
        writer.sequence(&self.because, |writer, input| {
            writer.bytes(input.as_bytes());
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(byte: u8) -> ObjectId {
        ObjectId::from_bytes([byte; 16])
    }

    fn changed(bytes: &[u8]) -> BTreeSet<ObjectId> {
        bytes.iter().map(|byte| object(*byte)).collect()
    }

    #[test]
    fn an_empty_graph_makes_nothing_stale() {
        assert!(DependencyGraph::new()
            .stale_outputs(&changed(&[1]))
            .is_empty());
        assert!(DependencyGraph::new().is_empty());
    }

    #[test]
    fn a_changed_input_makes_its_output_stale() {
        let graph = DependencyGraph::new().with_edge(object(9), object(1));
        let stale = graph.stale_outputs(&changed(&[1]));
        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0].output(), object(9));
        assert_eq!(stale[0].because(), &[object(1)]);
        assert_eq!(graph.inputs_of(object(9)), vec![object(1)]);
        assert_eq!(graph.len(), 1);
    }

    #[test]
    fn an_untouched_input_makes_nothing_stale() {
        let graph = DependencyGraph::new().with_edge(object(9), object(1));
        assert!(graph.stale_outputs(&changed(&[2])).is_empty());
    }

    #[test]
    fn staleness_is_transitive() {
        let graph = DependencyGraph::new()
            .with_edge(object(8), object(1))
            .with_edge(object(9), object(8));
        let stale = graph.stale_outputs(&changed(&[1]));
        let outputs: Vec<ObjectId> = stale.iter().map(StaleOutput::output).collect();
        assert_eq!(outputs, vec![object(8), object(9)]);
        assert_eq!(stale[1].because(), &[object(8)]);
    }

    #[test]
    fn a_cycle_terminates() {
        let graph = DependencyGraph::new()
            .with_edge(object(1), object(2))
            .with_edge(object(2), object(1));
        let stale = graph.stale_outputs(&changed(&[1]));
        let outputs: Vec<ObjectId> = stale.iter().map(StaleOutput::output).collect();
        assert_eq!(outputs, vec![object(1), object(2)]);
    }

    #[test]
    fn the_result_does_not_depend_on_the_order_the_edges_arrived_in() {
        let one = DependencyGraph::new()
            .with_edge(object(9), object(1))
            .with_edge(object(9), object(2));
        let other = DependencyGraph::new()
            .with_edge(object(9), object(2))
            .with_edge(object(9), object(1));
        assert_eq!(
            one.stale_outputs(&changed(&[1, 2])),
            other.stale_outputs(&changed(&[1, 2]))
        );
    }

    #[test]
    fn adding_an_edge_leaves_the_graph_it_came_from_untouched() {
        let graph = DependencyGraph::new().with_edge(object(9), object(1));
        let grown = graph.clone().with_edge(object(8), object(1));
        assert_eq!(graph.len(), 1);
        assert_eq!(grown.len(), 2);
    }
}
