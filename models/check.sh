#!/usr/bin/env bash
# models/check.sh --- run the CWP formal model and its mutation campaign.
#
# The model is only worth having if it can fail. This script runs the base
# configuration, which must PASS, and then every mutation and the divergence
# configuration, each of which must FAIL on a named invariant. It exits
# non-zero if any expectation is unmet --- including a mutation that passes,
# which means the guard it removed was not a guard.
#
# Contract, deliberately:
#   - zero network;
#   - no state outside models/ and the run directory it is given;
#   - the exit code is the verdict, and every failure names the configuration.
#
# TLC is NOT a repository dependency and is NOT part of `npm test`: it needs a
# Java runtime, and the repository's gates are zero-network and under thirty
# seconds. This is an out-of-band check, run deliberately. Point it at TLC:
#
#   TLA2TOOLS_JAR=/path/to/tla2tools.jar models/check.sh
#
# Options:
#   --only <name>   run one configuration by file name, e.g. --only mesh.cfg
#   --quick         base and divergence only
#   --workers <n>   TLC worker threads (default: 4)
#   --run-dir <d>   where TLC writes (default: a fresh mktemp directory)
#   --archive <d>   write the gate artifact into <d>: one JSON row per
#                   configuration, the verdict, and every counterexample.

set -u -o pipefail

MODELS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKERS=4
ONLY=""
QUICK=0
RUN_DIR=""
ARCHIVE=""

while [ $# -gt 0 ]; do
  case "$1" in
    --only)    ONLY="$2"; shift 2 ;;
    --quick)   QUICK=1; shift ;;
    --workers) WORKERS="$2"; shift 2 ;;
    --run-dir) RUN_DIR="$2"; shift 2 ;;
    --archive) ARCHIVE="$2"; shift 2 ;;
    -h|--help) sed -n '2,28p' "${BASH_SOURCE[0]}"; exit 0 ;;
    *) echo "check.sh: unknown option $1" >&2; exit 2 ;;
  esac
done

# --- locate a Java runtime and tla2tools.jar -------------------------------
JAVA_BIN="${JAVA:-java}"
if ! "$JAVA_BIN" -version >/dev/null 2>&1; then
  echo "check.sh: no Java runtime. Set JAVA=/path/to/bin/java." >&2
  echo "check.sh: TLC needs a JRE; this repository does not ship one." >&2
  exit 3
fi

JAR="${TLA2TOOLS_JAR:-}"
if [ -z "$JAR" ]; then
  for candidate in "$MODELS_DIR/tla2tools.jar" "$HOME/.tla/tla2tools.jar" \
                   "/usr/local/lib/tla2tools.jar" "/opt/tlaplus/tla2tools.jar"; do
    [ -f "$candidate" ] && JAR="$candidate" && break
  done
fi
if [ ! -f "$JAR" ]; then
  echo "check.sh: tla2tools.jar not found. Set TLA2TOOLS_JAR." >&2
  echo "check.sh: it is released at https://github.com/tlaplus/tlaplus/releases" >&2
  exit 3
fi

if [ -z "$RUN_DIR" ]; then
  RUN_DIR="$(mktemp -d "${TMPDIR:-/tmp}/mesh-tlc.XXXXXX")"
fi
mkdir -p "$RUN_DIR"
cp "$MODELS_DIR/mesh.tla" "$MODELS_DIR/mesh_tree.tla" "$RUN_DIR/"

# --- the expectations ------------------------------------------------------
# config file : module : expectation : the invariant or property : depth
#   pass       TLC exits zero with everything holding
#   fail       TLC reports exactly that invariant violated
#   failprop   TLC reports a temporal property violated
#   failaction TLC reports exactly that ACTION property violated. Separate from
#              failprop because an action property is what idempotence is: a
#              claim about a transition, which no invariant over a state can
#              make.
#
# `depth` is the number of states in the counterexample --- the minimal
# violating depth. It is ASSERTED ONLY AT ONE WORKER and printed otherwise,
# because it is only reproducible there: TLC's workers are not level
# synchronized, so a parallel run can report a violation one level deeper than
# the shallowest one that exists. Measured directly rather than assumed ---
# `mesh-mut-replay-approval.cfg` reports 8 states at one worker and 9 at four
# and at eight on the same machine, and `mesh-mut-collect-unpublished.cfg`
# reports 3 and 4. Asserting it at every worker count would be a flaky gate,
# and a flaky gate is worse than an absent one.
EXPECTATIONS=(
  "mesh.cfg:mesh.tla:pass:-:-"
  "mesh-publication.cfg:mesh.tla:pass:-:-"
  "mesh-history.cfg:mesh.tla:pass:-:-"
  "mesh-liveness.cfg:mesh.tla:pass:-:-"
  "mesh-idempotence.cfg:mesh.tla:pass:-:-"
  "mesh-divergence-conditional.cfg:mesh.tla:pass:-:-"
  "mesh-tree.cfg:mesh_tree.tla:pass:-:-"
  "mesh-divergence.cfg:mesh.tla:fail:NoSilentDivergence:5"
  "mesh-liveness-mut-collect.cfg:mesh.tla:failprop:EventuallyEveryReconnectedPeerConverges:6"
  "mesh-mut-apply-without-parents.cfg:mesh.tla:fail:AppliedIsExactlyWhatTheCausalRuleAdmits:4"
  "mesh-mut-refuses-honest-work.cfg:mesh.tla:fail:NoHonestRefusal:5"
  "mesh-mut-drop-orphan.cfg:mesh.tla:fail:NoDeliveredChangeSetIsDropped:4"
  "mesh-mut-order-by-clock.cfg:mesh.tla:fail:CausalOrderIsRespected:3"
  "mesh-mut-order-by-arrival.cfg:mesh.tla:fail:Convergence:5"
  "mesh-mut-redeliver-reorders.cfg:mesh.tla:failaction:DuplicateDeliveryIsIdempotent:6"
  "mesh-mut-no-compare-and-swap.cfg:mesh.tla:fail:CompareAndSwapHeld:7"
  "mesh-mut-agent-may-approve.cfg:mesh.tla:fail:OnlyAnExactHumanReviewedStateAdvances:5"
  "mesh-mut-silent-rebase.cfg:mesh.tla:fail:OnlyAnExactHumanReviewedStateAdvances:10"
  "mesh-mut-publish-without-content.cfg:mesh.tla:fail:CanonicalContentIsAvailable:6"
  "mesh-mut-collect-unpublished.cfg:mesh.tla:fail:AcknowledgedWorkIsNeverDiscarded:3"
  "mesh-mut-replay-approval.cfg:mesh.tla:fail:ApprovalIsSingleUse:8"
  "mesh-mut-epoch-ignored.cfg:mesh.tla:fail:CanonicalAdmittedInItsOwnEpoch:6"
  "mesh-tree-mut-no-cycle-check.cfg:mesh_tree.tla:fail:DirectoryAncestryIsAcyclic:4"
  "mesh-tree-mut-arrival-order.cfg:mesh_tree.tla:fail:ConcurrentMovesResolveIdenticallyOnEveryPeer:5"
)

# The digest of one file, on either of the two spellings a mac or a Linux box
# has. An artifact that cannot say which bytes produced it is not an artifact.
sha256() {
  if command -v shasum >/dev/null 2>&1; then shasum -a 256 "$1" | cut -d' ' -f1
  elif command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1
  else echo "no-sha256-tool"; fi
}

run_one() {
  local cfg="$1" module="$2" expect="$3" invariant="$4" depth="$5"
  local log="$RUN_DIR/${cfg%.cfg}.log"
  cp "$MODELS_DIR/$cfg" "$RUN_DIR/"
  local started ended elapsed code
  started=$(date +%s)
  ( cd "$RUN_DIR" && "$JAVA_BIN" -XX:+UseParallelGC -cp "$JAR" tlc2.TLC \
      -workers "$WORKERS" -config "$cfg" "$module" ) >"$log" 2>&1
  code=$?
  ended=$(date +%s)
  elapsed=$((ended - started))

  local verdict="UNEXPECTED"
  case "$expect" in
    pass)
      if [ "$code" -eq 0 ] && \
         grep -q "Model checking completed. No error has been found" "$log"; then
        verdict="ok"
      fi ;;
    fail)
      if [ "$code" -ne 0 ] && grep -q "Invariant $invariant is violated" "$log"; then
        verdict="ok"
      fi ;;
    failprop)
      if [ "$code" -ne 0 ] && grep -q "Temporal properties were violated" "$log"; then
        verdict="ok"
      fi ;;
    failaction)
      if [ "$code" -ne 0 ] && \
         grep -q "Action property $invariant is violated" "$log"; then
        verdict="ok"
      fi ;;
  esac

  local states
  states=$(grep -o '[0-9,]* distinct states found' "$log" | tail -1 | cut -d' ' -f1)
  states=${states:-"-"}

  # The counterexample, in states. Asserted only at one worker --- see the
  # note above EXPECTATIONS for the measurement that says why.
  local trace="-" trace_note=""
  if [ "$expect" != "pass" ]; then
    trace=$(grep -c '^State ' "$log")
    if [ "$depth" != "-" ] && [ "$WORKERS" -eq 1 ]; then
      if [ "$trace" -ne "$depth" ]; then
        verdict="UNEXPECTED"
        trace_note="  (expected a $depth-state counterexample, got $trace)"
      fi
    elif [ "$depth" != "-" ]; then
      trace_note="  (depth $depth asserted only at --workers 1)"
    fi
  fi

  local trace_col=""
  [ "$trace" = "-" ] || trace_col="${trace}-state trace"
  printf '  %-10s %-38s %-11s %4ss %12s states  %-14s %s%s\n' \
         "$verdict" "$cfg" "$expect" "$elapsed" "$states" "$trace_col" \
         "$invariant" "$trace_note"

  # The gate artifact. One JSON object per configuration --- what was run,
  # against which module, what was expected, what happened, and the
  # counterexample when there is one.
  #
  # The configuration is identified by the SHA-256 of its bytes rather than
  # copied. A second copy of a model in the same repository is a model that can
  # silently disagree with the first one; a digest cannot, and `verify.sh` in
  # this artifact fails when the file it names has moved on.
  if [ -n "$ARCHIVE" ]; then
    [ "$expect" = "pass" ] || \
      sed -n '/^Error: /,$p' "$log" > "$ARCHIVE/counterexamples/${cfg%.cfg}.txt"
    printf '{"config":"%s","config_sha256":"%s","module":"%s","module_sha256":"%s","expect":"%s","verdict":"%s","named":"%s","distinct_states":"%s","trace_states":"%s","declared_depth":"%s","seconds":%s,"workers":%s}\n' \
           "$cfg" "$(sha256 "$MODELS_DIR/$cfg")" \
           "$module" "$(sha256 "$MODELS_DIR/$module")" \
           "$expect" "$verdict" "$invariant" "$states" \
           "$trace" "$depth" "$elapsed" "$WORKERS" >> "$ARCHIVE/campaign.jsonl"
  fi

  [ "$verdict" = "ok" ] || {
    echo "       --- last 20 lines of $log ---"
    tail -20 "$log" | sed 's/^/       /'
    return 1
  }
  return 0
}

# TLC prints its build on the first line of any invocation, including the one
# that then complains there is no module. There is no -version flag, and that
# invocation exits non-zero --- which under `set -o pipefail` is why the run is
# wrapped in `|| true` rather than followed by a `||` fallback. The fallback
# spelling put BOTH the version and the word "unknown" into the artifact.
TLC_VERSION="$( { "$JAVA_BIN" -cp "$JAR" tlc2.TLC 2>&1 || true; } \
                | grep -m1 'TLC2 Version' )"
TLC_VERSION="${TLC_VERSION:-TLC2 Version unknown}"
# The double quotes in `openjdk version "21"` would make run.json unparseable,
# and an artifact a reader has to repair is not one.
JAVA_VERSION="$("$JAVA_BIN" -version 2>&1 | head -1 | tr -d '"')"

if [ -n "$ARCHIVE" ]; then
  mkdir -p "$ARCHIVE/counterexamples"
  : > "$ARCHIVE/campaign.jsonl"
fi

echo "mesh formal model --- base configurations and mutation campaign"
echo "  java: $JAVA_VERSION"
echo "  tlc:  $JAR"
echo "        $TLC_VERSION"
echo "  runs: $RUN_DIR"
[ -n "$ARCHIVE" ] && echo "  artifact: $ARCHIVE"
echo

failures=0
ran=0
for row in "${EXPECTATIONS[@]}"; do
  IFS=':' read -r cfg module expect invariant depth <<< "$row"
  [ -n "$ONLY" ] && [ "$ONLY" != "$cfg" ] && continue
  if [ "$QUICK" -eq 1 ] && [ "$cfg" != "mesh.cfg" ] && [ "$cfg" != "mesh-divergence.cfg" ]; then
    continue
  fi
  ran=$((ran + 1))
  run_one "$cfg" "$module" "$expect" "$invariant" "$depth" || failures=$((failures + 1))
done

echo
if [ -n "$ARCHIVE" ]; then
  printf '{"campaign":"mesh formal model","configurations":%s,"failures":%s,"workers":%s,"java":"%s","tlc":"%s","uname":"%s","models_sha256":{' \
         "$ran" "$failures" "$WORKERS" "$JAVA_VERSION" "$TLC_VERSION" \
         "$(uname -srm)" > "$ARCHIVE/run.json"
  sep=""
  for f in "$MODELS_DIR"/*.tla "$MODELS_DIR"/*.cfg "$MODELS_DIR/check.sh"; do
    printf '%s"%s":"%s"' "$sep" "$(basename "$f")" "$(sha256 "$f")" \
      >> "$ARCHIVE/run.json"
    sep=","
  done
  printf '}}\n' >> "$ARCHIVE/run.json"
fi
if [ "$failures" -eq 0 ]; then
  echo "formal model: clean --- every base configuration holds and every mutation was caught"
  exit 0
fi
echo "formal model: $failures configuration(s) did not behave as declared"
exit 1
