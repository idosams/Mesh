# Local mounted demo

Run the functional local proof from the repository root:

```sh
sh examples/run-local-mounted-demo.sh
```

The wrapper starts a privileged Linux container with `--network none`. The inner demo builds only
the seven dependency-free local crates it needs, opens real `/dev/fuse` sessions, and checks:

1. a fresh daemon state has zero durable records;
2. user and actor mounts are separate and `notes.txt` is absent from the user view;
3. an ordinary Node process, with its working directory inside the actor mount, writes the file;
4. close-path capture promotes the bytes and appends one manifest plus one atomic ChangeSet;
5. daemon state moves from `operations=0` to `operations=1`, raw `records=0` to `records=2`, its
   digest changes, and `notes.txt` is materialized from the canonical payload;
6. a shadow mount returns the saved bytes, rejects writes with kernel `EROFS`, and does not move
   durable state or disturb the actor view;
7. a second mount is ready through `stat(2)` in less than 250 ms; and
8. every mount and the daemon release cleanly.

## Corrected claims

The original handoff's raw `records 0 -> 1` observable is incompatible with the durable
reconstruction contract. One `OperationRecord` is one ChangeSet containing create, write, and link,
but reconstructing the referenced bytes also requires one `ManifestRecord`. The truthful result is
therefore `operations 0 -> 1` and raw `records 0 -> 2`.

This proof calls its boundary **exact close-path/recovery capture**. It does not label the result
`saved privately`: ADR-0039 has no selected, measured idle-settling threshold, and says a consuming
task cannot honestly invent one. The Node harness performs no Mesh-aware action, but this demo is
not the missing product checkpoint policy.

## Environment

The command requires Docker and permission to run a privileged container. If
`mesh-capture:linux` is absent, the wrapper builds it from `rust:1.97-bookworm` and installs Node;
that one-time preparation can require network access. The proof itself always starts the container
with `--network none`. The image supplies the Linux FUSE device and the repository's pinned Rust
toolchain while keeping a non-Linux host unchanged; Docker selects the host's supported container
architecture.

Set `MESH_DEMO_IMAGE` to use or prepare a different local image tag.
