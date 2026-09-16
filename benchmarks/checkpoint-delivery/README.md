# Checkpoint delivery measurement

This is the standing W5-reduced proof for the production file-save path. It measures the bytes
newly linked into CAS after the first 1 KiB overwrite described by generator version 1, seed 42.
The before-state and edited state travel through `save_file_version`: content-defined chunking,
CAS promotion, the atomic metadata transaction, actor-head/outbox derivation, and the immutable
journal. The harness explicitly selects candidate-B physical paging at 256 references, so the
linked-byte result includes newly written page/index objects as well as file chunks and the
ChangeSet. A row is printed only after the edited bytes reconstruct exactly from the durable CAS.
It records generator, baseline save, edited save, reconstruction and cleanup time, plus staged and
reused CAS-object counts. A row is invalid unless the edited save actually reuses objects, stages
fewer than the fresh baseline, and the complete process finishes inside 120 seconds.

Five invocations mean five processes and five fresh workspace/CAS/SQLite roots:

```sh
benchmarks/checkpoint-delivery/capture.sh > /tmp/checkpoint-delivery.jsonl
node benchmarks/checkpoint-delivery/verify.mjs /tmp/checkpoint-delivery.jsonl
```

The budget is strict: `0 < novel_cas_bytes < 4 MiB`. The byte count includes every newly linked
file chunk, physical manifest page/index object and canonical ChangeSet payload; scratch files,
SQLite and journal framing are not CAS content and are not counted. A missing or repeated process
id, generator drift, edit drift,
byte disagreement, or reconstruction failure invalidates the run rather than becoming a number.

The verifier has a mutation self-test independent of a capture, including missing reuse and a
return of the 120-second blocker:

```sh
node benchmarks/checkpoint-delivery/verify.mjs --self-test
```

## Current bounded-run status

No delivery number is published yet. On 2026-08-20, before physical paging landed, from exact main
`f6b966a29d27b7d15bd359d5391f90a23d2b304a`, one optimized W5-reduced invocation exceeded 120
seconds and had written about 196 MiB under its fresh workspace without reaching row emission. The
bounded run was stopped and therefore supplies no sample and no pass/fail result for the 4 MiB
byte ceiling. This is an execution-time blocker for collecting five fresh-process rows on this
host, not evidence that the byte ceiling failed. The harness deliberately emits nothing before
the complete durable save, reconstruction, and cleanup all succeed.

The first profiled run of the early-reuse implementation is retained in
`profile-2026-08-20.json`. It completed correctly in 123,682 ms: the fresh baseline staged 14,470
objects and took 118,399 ms, while the edited save staged only 4 objects, reused 14,466 existing
names, and took 1,890 ms. Reconstruction had zero failures and the edited save linked 18,612 novel
CAS bytes, but one row is not the required five-process capture and the total remains 3,682 ms over
the collection bound. The verifier therefore rejects it as a completed result.

This diagnoses two separate facts. Redundant edited-save staging was safe to remove because the old
path made the same name-presence observation at link time and step 5 still rechecks every manifest
reference before metadata. The remaining blocker is the fresh baseline's 14,470 individually
durable promotions. This change does not batch, remove, or weaken those fsyncs. A future optimization
needs an explicit equivalent durability proof; the timing bound is not permission to invent one.
