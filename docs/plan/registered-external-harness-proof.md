# Registered external-harness acceptance

This opt-in test runs an external Codex process in a fresh synthetic consumed lane. Mesh begins watching after the provider's first edit, acknowledges saved progress, then stops and joins. The same provider process makes another edit after stop. The test verifies unchanged Git HEAD/index, root inode, original input bytes, retained history after stop, and executable bytes. It preserves the fixture and raw logs for inspection.

This is a controlled noninteractive native test. It does not establish existing interactive-session compatibility, graphical review, packaged acceptance, protected-main approval, integration/restore, another provider or another host. Timing includes provider startup and coordination and is not a velocity comparison. Native registration and input consumption are prepared by the fixture; this is not proof of UI provisioning.

## Reproduce

Use a macOS checkout containing the native fixture export and registered harness commands. The installed Codex CLI must already be authenticated. The runner uses workspace-write sandboxing, ignores user configuration for the controlled run, and does not copy credentials. Starting it uses the signed-in provider account.

Choose new absolute paths for the fixture manifest and output directory. Do not reuse a fixture modified by another acceptance test: this runner requires four initial versions, no Git repository and no prior gate files. Only the synthetic lane is edited; the input owner is checked byte-for-byte.

```sh
MESH_REGISTERED_CAPTURE_FIXTURE=/absolute/new-fixture.json \
  cargo nextest run -p mesh-desktop \
  -E 'test(desktop_reopens_consumed_review_and_preserves_ordinary_projects)'

MESH_BUILD_REVISION="$(git rev-parse HEAD)" \
  cargo build -p mesh-desktop --bin mesh-desktop

python3 apps/desktop/scripts/prove-registered-external-harness.py \
  --fixture /absolute/new-fixture.json \
  --executable "$PWD/target/debug/mesh-desktop" \
  --revision "$(git rev-parse HEAD)" \
  --sha256 "$(shasum -a 256 target/debug/mesh-desktop | awk '{print $1}')" \
  --output /absolute/new-proof-output
```

An absolute `--provider` path can select the Codex executable; otherwise the runner uses the executable found on PATH. Keep the source and executable fixed during the run. The runner checks exact embedded revision plus the supplied SHA-256 before editing, then checks the executable again after completion. A mismatched digest is refused before fixture or output mutation.

Inspect `proof.json` and both processes' stdout/stderr in the output directory. Failure leaves evidence and the synthetic fixture in place. The successful result requires at least two new saved versions, a joined stopped watcher, a still-live provider after stop, its final edit, and no subsequent journal or version-list change. Gate files are synthetic test coordination and may enter captured content. The current runner separately previews the exact first and second saved versions and verifies both remain identical after the final uncaptured edit. It waits for the expected second saved text, so a gate-only save cannot satisfy the second-edit assertion. Earlier recorded v1 results did not include these preview checks. Process cleanup covers child handles started by the runner and is not an OS-containment claim.

## Recorded run

The 2026-10-05 portable run used exact native executable revision `dff7384212c0ed7422594fab06bc35dfcd092036`, SHA-256 `1af4edfe29e790f002d75840f8140f3d87df14ed19e7da9857f8ede96a974fd5`, and Codex CLI `0.158.0-alpha.2.1`. Fresh fixture creation passed in 27.435s. The provider run passed in 24.503s, taking history from four to six versions. Wrong-SHA refusal was verified separately before fixture mutation. The runner, result and process logs were retained and seven artifact hashes independently verified. Raw provider logs and private paths are not published.


The saved-preview extension records `mesh.registered-external-harness-acceptance/v2` with both exact
saved identities and native content digests. Each preview comes from a separate native command
process over retained history. The extension requires a new exact executable/provider run; existing
v1 evidence does not retroactively prove saved previews. Native inspection is still not graphical
review or protected-main approval.
