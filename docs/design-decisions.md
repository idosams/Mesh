# Public design decision summaries

These summaries preserve the decision identifiers cited by the public source and documentation.
They are not new protocol rules or copies of the internal planning history. The versioned
[protocol artifacts](../protocol/README.md), source, and tests remain the implementation authority.
Historical measurements do not establish present performance or platform readiness.

## ADR-0003

Actor identity is derived from the actor key rather than a mutable identifier-to-key registry.
Renaming an actor does not change authorship. A new key names a new actor; continuity and capability
reissue cannot be inferred from a display name. See [mesh-types](../crates/mesh-types/).

## ADR-0007

Signed records use fixed-order, definite-length CBOR arrays in a closed profile. Restricting the
allowed shapes and rejecting alternate encodings makes signatures reproducible across independent
implementations. Schema order, not map iteration, determines bytes. See the
[canonical schema](../protocol/schemas/canonical-encoding-v0.json). ADR-0033 supersedes the original
separate identity-framing choice.

## ADR-0013

Define a reproducible benchmark corpus before generating large on-disk workloads. Seeds, workload
shape, scale, and provenance must be explicit so measurements can be repeated and compared.
See the [workload corpus](../benchmarks/workloads/README.md).

## ADR-0015

Deterministic actor-head convergence depends on verified record identity binding the causal parent
set. Accepting an identifier without verifying the bytes it names can make peers disagree even
when their identifier sets match. Admission checks belong where canonical bytes are available.
See [state advancement](../crates/mesh-state/) and [synchronization](../crates/mesh-sync-engine/).

## ADR-0019

The terminology scanner rejects unresolved glob re-exports instead of guessing which public names
they expose. Explicit names make the diagnostic's coverage reviewable. This diagnostic is separate
from the public test gate; see [protocol terminology](protocol.md).

## ADR-0021

Content-defined chunking is selected from file bytes and length, not paths or extensions. The
historical experiment chose separate profiles above and below one MiB to balance metadata overhead
and changed-byte transfer. Current parameters and evidence belong to the
[chunk-policy experiment](../benchmarks/workloads/chunk-policy/README.md) and
[chunking implementation](../crates/mesh-chunking/); the experiment is not a fresh benchmark run.

## ADR-0033

An immutable canonical record is named by the digest of its canonical encoding. Keeping a second
identity-only byte representation would make an external verifier depend on an unpublished format.
Raw chunks remain named by the digest of their bytes. See the
[protocol identity rules](protocol.md) and [conformance suite](../protocol/conformance/README.md).

## ADR-0039

A checkpoint captures one view at one sequence point. Boundary candidates do not prove that a
whole view has settled. Recovery preservation can run before settling without claiming that an
editor completed a meaningful save. Folder observation cannot invent close or fsync evidence.
See the [save-pattern corpus](../tests/compatibility/save-patterns/v0/README.md) and
[recovery-preservation corpus](../tests/compatibility/recovery-preservation/v0/README.md).
