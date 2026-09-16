# Checkpoint settling measurement

This directory is TASK-347's evidence boundary. It separates the unreplayable historical
12,225-change npm observation from a deterministic successor fixture whose bytes and full
macOS/Linux polling streams can be checked by another runner.

The first implementation commit freezes `manifest.json`, the fixture generator, the exact
candidate grids and the lexicographic selection rule before any comparative capture is read.
Three instrumentation-only calibration samples per platform then exposed missing environment
metadata and were discarded. A cold review then found that the first accepted schedules counted
fixture paths without comparing their bytes. Those schedules remain at `captures/macos.json` and
`captures/linux.json` as superseded evidence and are not selection inputs. The accepted exact-byte
schedules use the repaired capture script at commit
`b64a72840b42bcd86620fac8bf03d18972f383e8`; none of the instrumentation or semantic-oracle
corrections changes the candidate grid or selection rule.

`fixture.mjs` generates an 8 MiB, 2,048-file local npm package without network access.
`capture.mjs` packs it outside the timed region, installs it into an empty consumer with a private
empty cache, records monotonic polling observations, and refuses the run unless the consumer,
both lockfiles, and the exact expected path and bytes of all fixture files exist at the end. The
Linux run is built from the digest-pinned base and timestamped Debian snapshot in
`Dockerfile.linux`; both measured installs run with the network disabled.

The complete Linux reacquisition is three commands from the repository root. The image ID is
inspected once, passed into the capture, and used as the exact `docker run` target; the mutable tag
is never used for execution:

```sh
docker build -f benchmarks/checkpoint-settling/Dockerfile.linux -t mesh-settling-exact:b64a7284 .
IMAGE_ID="$(docker image inspect --format '{{.Id}}' mesh-settling-exact:b64a7284)"
docker run --rm --network none -e MESH_SETTLING_IMAGE="$IMAGE_ID" --mount type=bind,src="$(pwd)/benchmarks/checkpoint-settling/captures",dst=/out "$IMAGE_ID" --platform linux-overlayfs --commit b64a72840b42bcd86620fac8bf03d18972f383e8 --fixture-digest sha256:b3ec3a10d9e2bb3f1b81923388ec73f307ac6f40ffc2c997a2d79ee15ebcf78a --network-evidence docker-network-none --out /out/linux-exact-v3.json
```

The image entrypoint contains the exact capture, fixture, and semantic-oracle sources. The output
directory is a host bind mount, and the measured process records the same inspected image ID that
Docker executes. A rebuild can have a different image ID; that is a new acquisition, not permission
to relabel the checked-in sample.

`capture-tools.mjs` adds five bounded non-GUI arms without changing the frozen grid or rule. Each
arm has five fresh-state samples and records the exact input, script, corpus, environment,
invocation and final-tree digest. The Linux Git and IntelliJ command-line formatter captures run
by immutable image ID with Docker networking disabled. Exact invocations are frozen in
`manifest.json`; a rerun is a new separately digested schedule.

The selection rule is intentionally fail-closed. A full npm successor on two platforms is necessary
but not sufficient: every one of the twelve editor/tool/platform cells must also have a timed stream.
If an arm cannot be acquired, the report names it and leaves the affected parameter unresolved rather
than turning an incomplete matrix into a default.

That is the current outcome. Both full-scale npm successors and five non-GUI tool cells are checked
in, all sixteen source patterns replay, and all twelve matrix cells are enumerated. Seven cells have
timing. VS Code on both platforms, IntelliJ on macOS, vim/neovim on Linux and rustfmt on Linux remain
explicitly absent; no synthetic timestamp fills them. ADR-0042 therefore selects no value and leaves
TASK-73 blocked.

```sh
node benchmarks/checkpoint-settling/fixture.mjs
node benchmarks/checkpoint-settling/verify.mjs
node benchmarks/checkpoint-settling/analyze.mjs --check
node benchmarks/checkpoint-settling/verify.mjs --full --mutations
```
