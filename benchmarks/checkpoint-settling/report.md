# TASK-347 settling measurement report

## Decision

**Selected from the complete twelve-cell timed matrix.** Every value is the smallest predeclared candidate satisfying its frozen rule.

- idle interval: 50 ms
- maximum uncheckpointed bytes: 65536
- maximum uncheckpointed time: 25 ms
- timed cells: 12/12
- missing timing: none

The historical 12,225-change npm observation remains aggregate provenance only and was not read as a selection row. The candidate grid and rule were frozen before the comparative revision-2 captures were read.

## Full-scale successor captures

| Platform | OS / filesystem | Runtime | Polls | Observations | Changes | Elapsed ms |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| macos | darwin 25.6.0 / apfs | v24.7.0 / npm 11.5.1 | 13,863 | 33 | 3,148 | 1139.107 |
| linux | linux 6.10.14-linuxkit / overlayfs | v18.20.4 / npm 9.2.0 | 121,529 | 147 | 2,159 | 1378.290 |

Both captures use fixture sha256:b3ec3a10d9e2bb3f1b81923388ec73f307ac6f40ffc2c997a2d79ee15ebcf78a and tarball sha256:c1b54969703721121522c531b66bc08dd20b35fc9cbc4338fb4c705a88e4725d. Both installed all 2,048 files, produced both lockfiles and left the consumer naming the dependency. Different final-tree digests are expected because the pinned npm versions write platform/version-specific metadata.

## Tool captures

| Family | Platform | Samples | Observations | Min ms | Median ms | p95 ms | Max ms |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| vim or neovim | macOS | 5 | 26 | 112.086 | 113.049 | 128.121 | 128.121 |
| Git operations | macOS | 5 | 17 | 167.007 | 180.666 | 280.577 | 280.577 |
| a formatter | macOS | 5 | 10 | 111.634 | 111.978 | 287.554 | 287.554 |
| Git operations | Linux | 5 | 10 | 116.154 | 117.215 | 123.065 | 123.065 |
| a JetBrains IDE | Linux | 5 | 19 | 3952.474 | 4227.035 | 4357.910 | 4357.910 |
| VS Code | macOS | 5 | 10 | 3246.628 | 3249.185 | 3433.102 | 3433.102 |
| VS Code | Linux | 5 | 10 | 1948.431 | 1971.697 | 3235.837 | 3235.837 |
| vim or neovim | Linux | 5 | 9 | 102.160 | 103.223 | 106.881 | 106.881 |
| a formatter | Linux | 5 | 9 | 103.372 | 103.856 | 105.239 | 105.239 |
| a JetBrains IDE | macOS | 5 | 25 | 2817.940 | 2874.949 | 15283.055 | 15283.055 |

Every tool sample carries its exact input, corpus, script, environment, invocation and final-tree digest in the raw capture and normalized JSONL. All ten editor, version-control, and formatter cells are timed; the two package-manager cells come from the full-scale successor captures.

## Package-manager candidate distributions

### Idle interval

| Candidate ms | macOS false cuts | Linux false cuts | macOS max gap ms | Linux max gap ms |
| ---: | ---: | ---: | ---: | ---: |
| 25 | 0 | 0 | 19.591 | 21.174 |
| 50 | 0 | 0 | 19.591 | 21.174 |
| 100 | 0 | 0 | 19.591 | 21.174 |
| 250 | 0 | 0 | 19.591 | 21.174 |
| 500 | 0 | 0 | 19.591 | 21.174 |
| 1000 | 0 | 0 | 19.591 | 21.174 |
| 2000 | 0 | 0 | 19.591 | 21.174 |
| 5000 | 0 | 0 | 19.591 | 21.174 |

### Maximum uncheckpointed bytes

| Candidate bytes | macOS recovery checkpoints | Linux recovery checkpoints | macOS max bytes at risk | Linux max bytes at risk |
| ---: | ---: | ---: | ---: | ---: |
| 65536 | 19 | 53 | 983261 | 471040 |
| 262144 | 16 | 31 | 983261 | 471040 |
| 1048576 | 6 | 8 | 1904861 | 1048789 |
| 4194304 | 1 | 2 | 4681941 | 4194517 |
| 16777216 | 0 | 0 | 8390429 | 8390037 |

### Maximum uncheckpointed time

| Candidate ms | macOS recovery checkpoints | Linux recovery checkpoints | macOS max time at risk ms | Linux max time at risk ms |
| ---: | ---: | ---: | ---: | ---: |
| 25 | 6 | 10 | 25.000 | 25.000 |
| 50 | 3 | 5 | 50.000 | 50.000 |
| 100 | 1 | 2 | 100.000 | 100.000 |
| 250 | 0 | 1 | 182.546 | 250.000 |
| 500 | 0 | 0 | 182.546 | 291.546 |
| 1000 | 0 | 0 | 182.546 | 291.546 |
| 2000 | 0 | 0 | 182.546 | 291.546 |
| 5000 | 0 | 0 | 182.546 | 291.546 |
| 10000 | 0 | 0 | 182.546 | 291.546 |
| 30000 | 0 | 0 | 182.546 | 291.546 |

## What the rows contain

`results.jsonl` contains all 16 corpus replay results, all 12 required matrix cells, every normalized successor and tool observation, every predeclared candidate for every acquired sample, and the exact decision. Its digest is sha256:b15c011ef3b9f4f8f79b5336af760ae3e04dc0730d5c37f1aaf8cb3204cd49e7.

False meaningful-save cuts, recovery-preservation frequency, and bytes/time at risk remain separate columns. Only the globally selected candidate in each dimension is marked eligible. Polling can miss changes completed between readings, so counts are lower bounds on filesystem activity and no reliability claim is made.

## Reproduction

`node benchmarks/checkpoint-settling/analyze.mjs --check` regenerates the exact JSONL and report in memory. `node benchmarks/checkpoint-settling/verify.mjs --full --mutations` verifies capture/environment/corpus digests, the complete candidate grid, fail-closed selection and planted defects. Exact host and network-disabled Docker acquisition recipes are in `manifest.json`.
