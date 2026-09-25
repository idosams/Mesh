# platform/macos-fskit

**Maturity: feasibility evidence and partial platform work, not a supported mounted journey.**
See [Project status](../../docs/project-status.md) and the
[architecture guide](../../docs/architecture.md).

The planned Swift FSKit extension over `mesh-fskit-ffi`.

**Not yet implemented.** The current product path on macOS is the directory-watching fallback.
That fallback is useful but non-authoritative: it cannot observe kernel open, close, flush or
`fsync` boundaries.

## Feasibility status, 2026-08-20

Research PR [#1115](https://github.com/idosams/Mesh/pull/1115), exact head
`f5c275a2341b0c8c7ef4f9024ab0bc362b75e58d`, contains a useful historical FSKit API inventory and
a synthetic Swift/C/Rust reply-handler experiment. It must not be merged as current evidence:
its host facts and its seventeen-capability conformance baseline predate the present tree.

The current host and tree were rechecked at
`f6b966a29d27b7d15bd359d5391f90a23d2b304a`:

| Surface | Current observation |
|---|---|
| Host | Apple silicon, macOS 26.6.1 (25G76) |
| Runtime | `/System/Library/Frameworks/FSKit.framework` is present |
| Developer tools | Command Line Tools only; `xcodebuild -version` refuses because full Xcode is absent |
| Selected SDK | macOS 14.4; it contains no FSKit framework, header or module |
| Swift / Rust | Swift 5.10; rustc 1.97.1 |
| Product adapter contract | 18 capabilities |
| Directory fallback | 16 declared capabilities; current APFS grade 72 pass, 1 fail, 58 unsupported across 131 cases |
| FSKit implementation | No extension, C ABI operations, mount, entitlement evidence or FSKit conformance run |

The old operating-system blocker has therefore expired, while the build-and-measurement blocker
has not. A runtime framework does not make this checkout capable of compiling an FSKit extension:
the selected SDK is older than FSKit and the machine has no full Xcode installation.

The historical reply-handler result remains useful only for choosing the shape of the bridge. In
three 200,000-iteration runs, replying inline from Swift was about 80 ns per operation; moving the
reply to a Rust worker cost about 2.5 us; parking the Swift side as well cost about 10.4 us. This
supports keeping the portable adapter seam synchronous. It measured no FSKit call, extension,
kernel/XPC hop, filesystem operation or native-APFS comparison, so it is not evidence that FSKit
meets the product's latency or throughput thresholds.

## What closes the feasibility question

A current result needs all of the following on one named host:

1. full Xcode with an FSKit-capable SDK and the required extension entitlement;
2. a minimal, signed passthrough extension over the stable Rust C ABI;
3. the current 18-capability conformance oracle, with every restriction stated explicitly;
4. lookup, open, create, rename, enumerate and sequential-read measurements against native APFS;
5. a real-project build/test slowdown measurement.

Until that run exists, the supported conclusion is narrower than either “FSKit works” or “FSKit is
infeasible”: retain the directory fallback for the POC and keep FSKit deferred as the candidate
authoritative macOS adapter.

Find the tasks that fill this directory:

Use the [public issue tracker](https://github.com/idosams/Mesh/issues) to propose or track this work.
