#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/benchmark-ai-review-response.sh [options]

Run a same-binary Release A/B for the AI Review response renderer on macOS.

Options:
  --runs N             Isolated processes per variant (default: 3; minimum: 3)
  --samples N          RSS/footprint samples per phase (default/minimum: 5)
  --interval SECONDS   Delay between samples (default/minimum: 0.5)
  --settle SECONDS     Settling delay used by benchmark-memory (default/minimum: 2)
  --startup-timeout N  Window and fixture readiness timeout (default: 30)
  --fixture-messages N Fixture response pairs (default: 12; range: 1..64)
  --fixture-bytes N    Bytes per assistant response (default: 4096; range: 512..16384)
  --scroll-events N    Wheel events per direction in the balanced cycle (default: 24)
  --scroll-delta N     Pixel wheel delta per event (default: 120)
  --max-percent N      Candidate increase limit (default/max: 5; may tighten)
  --max-mib N          Candidate increase limit in MiB (default/max: 8; may tighten)
  --output DIR         Result root (default: memory-benchmark-results/ai-review-response-ab)
  --binary PATH        Release binary (default: target/release/usher)
  --no-build           Reuse the selected Release binary
  -h, --help           Show this help

The binary is built at most once. Both variants then run that exact binary with:

  USHER_AI_REVIEW_RESPONSE_FIXTURE=markdown-v1
  USHER_AI_REVIEW_RESPONSE_VARIANT=baseline|formatted

Each process records benchmark-memory's raw startup sample first. The internal
prepare phase then waits for the in-memory fixture, fixes the native window at
1280x860, injects one balanced AI Review panel scroll cycle, and returns control
to the static protocol. Acceptance applies only to the two settled 1280x860
periods. Startup samples remain in the raw results as diagnostic evidence.

Both the relative and absolute limits must pass independently for settled RSS
and physical footprint. A negative candidate-minus-baseline delta is a saving.
EOF
}

die() {
  echo "error: $*" >&2
  exit 1
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || die "$1 is required"
}

require_positive_integer() {
  [[ "$2" =~ ^[1-9][0-9]*$ ]] || die "$1 must be a positive integer: $2"
}

require_non_negative_number() {
  [[ "$2" =~ ^[0-9]+([.][0-9]+)?$ ]] || die "$1 must be non-negative: $2"
}

require_number_at_most() {
  awk -v value="$2" -v ceiling="$3" 'BEGIN { exit !(value <= ceiling) }' ||
    die "$1 cannot exceed the acceptance ceiling of $3: $2"
}

require_number_at_least() {
  awk -v value="$2" -v floor="$3" 'BEGIN { exit !(value >= floor) }' ||
    die "$1 must be at least $3 for acceptance measurements: $2"
}

normalize_path() {
  python3 - "$1" <<'PY'
import sys
from pathlib import Path

print(Path(sys.argv[1]).resolve(strict=False))
PY
}

metadata_value() {
  local path="$1" key="$2"
  awk -F '\t' -v key="$key" '$1 == key { print $2; exit }' "$path"
}

fixture_log_has_settled_record() {
  local path="$1" expected_variant="$2" expected_messages="$3" expected_bytes="$4"
  local expected_total=$((expected_messages * expected_bytes))
  local expected_rows=$((expected_messages * 2))
  local shape_pattern
  case "$expected_variant" in
    baseline)
      shape_pattern="formatted_rows=0 blocks=0 actions=0 raw_bytes=[0-9]+ cache_bytes=0"
      ;;
    formatted)
      shape_pattern="formatted_rows=${expected_messages} blocks=[1-9][0-9]* actions=[1-9][0-9]* raw_bytes=[0-9]+ cache_bytes=[1-9][0-9]*"
      ;;
    *)
      die "unsupported fixture variant for shape validation: $expected_variant"
      ;;
  esac
  local pattern="\\[AI Review\\]\\[ResponseFixture\\] settled fixture=markdown-v1 variant=${expected_variant} messages=${expected_messages} bytes_per_response=${expected_bytes} assistant_bytes_total=${expected_total} rows=${expected_rows} assistant_rows=${expected_messages} ${shape_pattern} fixture_persistence=false([[:space:]]|$)"
  grep -a -E -q "$pattern" "$path"
}

fixture_log_has_consumed_scroll() {
  local path="$1" expected_variant="$2"
  grep -a -E -q \
    "\\[AI Review\\]\\[ResponseFixture\\] scroll-consumed fixture=markdown-v1 variant=${expected_variant} movement_px=-?[0-9]+([.][0-9]+)?([[:space:]]|$)" \
    "$path"
}

wait_for_consumed_scroll() {
  local pid="$1" run_log="$2"
  local timeout="${USHER_AI_REVIEW_RESPONSE_HOOK_TIMEOUT_SECONDS:-30}"
  local expected_variant="${USHER_AI_REVIEW_RESPONSE_VARIANT:?missing response variant}"
  local attempts=0

  while (( attempts < timeout * 10 )); do
    kill -0 "$pid" 2>/dev/null || die "AI Review fixture process $pid exited during scroll"
    if fixture_log_has_consumed_scroll "$run_log" "$expected_variant"; then
      return 0
    fi
    sleep 0.1
    ((attempts += 1))
  done
  die "AI Review response panel did not report consumed scroll input: $run_log"
}

wait_for_fixture_log() {
  local pid="$1" run="$2" scenario="$3"
  local output_root="${USHER_AI_REVIEW_RESPONSE_CELL_OUTPUT:?missing cell output root}"
  local timeout="${USHER_AI_REVIEW_RESPONSE_HOOK_TIMEOUT_SECONDS:-30}"
  local expected_variant="${USHER_AI_REVIEW_RESPONSE_VARIANT:?missing response variant}"
  local expected_messages="${USHER_AI_REVIEW_RESPONSE_FIXTURE_MESSAGES:?missing fixture messages}"
  local expected_bytes="${USHER_AI_REVIEW_RESPONSE_FIXTURE_BYTES:?missing fixture bytes}"
  local attempts=0 run_log=""

  while (( attempts < timeout * 10 )); do
    kill -0 "$pid" 2>/dev/null || die "AI Review fixture process $pid exited before settling"
    run_log="$(
      find "$output_root" -type f -path "*/runs/${scenario}-run-${run}.log" -print 2>/dev/null |
        sort |
        tail -n 1
    )"
    if [[ -n "$run_log" ]] &&
      fixture_log_has_settled_record \
        "$run_log" "$expected_variant" "$expected_messages" "$expected_bytes"; then
      printf '%s\n' "$run_log"
      return 0
    fi
    sleep 0.1
    ((attempts += 1))
  done

  if [[ -n "$run_log" ]]; then
    die "fixture did not log a settled, non-persistent state within ${timeout}s: $run_log"
  fi
  die "fixture run log was not created within ${timeout}s under $output_root"
}

run_internal_prepare_hook() {
  [[ $# -eq 3 ]] || die "internal prepare hook expects PID RUN SCENARIO"
  local pid="$1" run="$2" scenario="$3"
  local window_control="${USHER_MEMORY_WINDOW_CONTROL_BINARY:?missing window-control helper}"
  local run_log control_file
  local width=1280 height=860
  local scroll_events="${USHER_AI_REVIEW_RESPONSE_SCROLL_EVENTS:-24}"
  local scroll_delta="${USHER_AI_REVIEW_RESPONSE_SCROLL_DELTA:-120}"

  [[ "$scenario" == "ai-review-response" ]] ||
    die "unexpected internal hook scenario: $scenario"
  [[ -x "$window_control" ]] || die "window-control helper is not executable: $window_control"
  require_positive_integer "scroll events" "$scroll_events"
  require_positive_integer "scroll delta" "$scroll_delta"

  echo "Prepare phase 1/3: waiting for markdown-v1 fixture (variant=${USHER_AI_REVIEW_RESPONSE_VARIANT:-unknown}, run=$run)"
  run_log="$(wait_for_fixture_log "$pid" "$run" "$scenario")"
  control_file="${run_log%.log}.control"
  [[ -f "$control_file" ]] || die "benchmark control file is missing: $control_file"

  echo "Prepare phase 2/3: fixing native window at ${width}x${height}"
  printf 'resize %s %s\n' "$width" "$height" >"$control_file"
  kill -USR1 "$pid"
  "$window_control" wait-size "$pid" "$width" "$height" 10000 ||
    die "window did not reach ${width}x${height}; grant Accessibility permission and retry"

  echo "Prepare phase 3/3: injecting balanced AI Review response scroll input"
  "$window_control" scroll-cycle \
    "$pid" \
    "$((width - 140))" \
    "$((height / 2))" \
    "$scroll_delta" \
    "$scroll_events" \
    8 || die "scroll input failed; grant Accessibility permission and retry"
  wait_for_consumed_scroll "$pid" "$run_log"
  "$window_control" wait-size "$pid" "$width" "$height" 10000 ||
    die "window size changed during scroll input"
  echo "Prepare complete: fixture=markdown-v1 persistence=false window=${width}x${height} scroll=balanced"
}

if [[ "${USHER_AI_REVIEW_RESPONSE_INTERNAL_HOOK:-0}" == "1" ]]; then
  run_internal_prepare_hook "$@"
  exit 0
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
script_path="$repo_root/scripts/benchmark-ai-review-response.sh"
benchmark_memory="$repo_root/scripts/benchmark-memory.sh"
compare_memory="$repo_root/scripts/compare-memory-benchmarks.py"

runs=3
samples=5
interval=0.5
settle_seconds=2
startup_timeout=30
fixture_messages=12
fixture_bytes=4096
scroll_events=24
scroll_delta=120
max_percent=5
max_mib=8
output_root="$repo_root/memory-benchmark-results/ai-review-response-ab"
default_binary="$repo_root/target/release/usher"
binary="$default_binary"
build_binary=1
build_mode=built

while [[ $# -gt 0 ]]; do
  case "$1" in
    --runs) runs="${2:?missing value for --runs}"; shift 2 ;;
    --samples) samples="${2:?missing value for --samples}"; shift 2 ;;
    --interval) interval="${2:?missing value for --interval}"; shift 2 ;;
    --settle) settle_seconds="${2:?missing value for --settle}"; shift 2 ;;
    --startup-timeout) startup_timeout="${2:?missing value for --startup-timeout}"; shift 2 ;;
    --fixture-messages) fixture_messages="${2:?missing value for --fixture-messages}"; shift 2 ;;
    --fixture-bytes) fixture_bytes="${2:?missing value for --fixture-bytes}"; shift 2 ;;
    --scroll-events) scroll_events="${2:?missing value for --scroll-events}"; shift 2 ;;
    --scroll-delta) scroll_delta="${2:?missing value for --scroll-delta}"; shift 2 ;;
    --max-percent) max_percent="${2:?missing value for --max-percent}"; shift 2 ;;
    --max-mib) max_mib="${2:?missing value for --max-mib}"; shift 2 ;;
    --output) output_root="${2:?missing value for --output}"; shift 2 ;;
    --binary) binary="${2:?missing value for --binary}"; shift 2 ;;
    --no-build) build_binary=0; build_mode=no-build; shift ;;
    -h|--help) usage; exit 0 ;;
    *) die "unknown option: $1" ;;
  esac
done

[[ "$(uname -s)" == "Darwin" ]] || die "this benchmark requires macOS"
if (( build_binary == 1 )); then
  require_command cargo
  require_command rustc
fi
require_command cp
require_command chmod
require_command footprint
require_command git
require_command awk
require_command ps
require_command python3
require_command shasum
require_command swiftc
require_command tee
require_positive_integer "--runs" "$runs"
(( runs >= 3 )) || die "--runs must be at least 3 for an A/B acceptance decision"
require_positive_integer "--samples" "$samples"
(( samples >= 5 )) || die "--samples must be at least 5 for acceptance measurements"
require_positive_integer "--startup-timeout" "$startup_timeout"
require_positive_integer "--fixture-messages" "$fixture_messages"
(( fixture_messages <= 64 )) || die "--fixture-messages must be in the range 1..64"
require_positive_integer "--fixture-bytes" "$fixture_bytes"
(( fixture_bytes >= 512 && fixture_bytes <= 16384 )) ||
  die "--fixture-bytes must be in the range 512..16384"
require_positive_integer "--scroll-events" "$scroll_events"
require_positive_integer "--scroll-delta" "$scroll_delta"
require_non_negative_number "--interval" "$interval"
require_non_negative_number "--settle" "$settle_seconds"
require_number_at_least "--interval" "$interval" 0.5
require_number_at_least "--settle" "$settle_seconds" 2
require_non_negative_number "--max-percent" "$max_percent"
require_non_negative_number "--max-mib" "$max_mib"
require_number_at_most "--max-percent" "$max_percent" 5
require_number_at_most "--max-mib" "$max_mib" 8
[[ -x "$benchmark_memory" ]] || die "benchmark helper is not executable: $benchmark_memory"
[[ -f "$compare_memory" ]] || die "comparison helper is missing: $compare_memory"
[[ -x "$script_path" ]] || die "benchmark script must be executable: $script_path"

if [[ "$binary" != /* ]]; then
  binary="$repo_root/$binary"
fi
binary="$(normalize_path "$binary")"
[[ "$binary" == */release/* ]] || die "same-binary A/B requires a Release binary path: $binary"
if (( build_binary == 1 )) && [[ "$binary" != "$default_binary" ]]; then
  die "automatic build targets $default_binary; pass --no-build with a custom --binary"
fi
if [[ "$output_root" != /* ]]; then
  output_root="$repo_root/$output_root"
fi
output_root="$(normalize_path "$output_root")"

case "$output_root/" in
  "$repo_root/"*)
    output_relative="${output_root#"$repo_root"/}"
    git -C "$repo_root" check-ignore -q -- "$output_relative" ||
      die "--output inside the worktree must be Git-ignored to preserve A/B fingerprints: $output_root"
    ;;
esac

if (( build_binary == 0 )); then
  [[ -x "$binary" ]] || die "Release binary is not executable: $binary"
fi

cd "$repo_root"
timestamp="$(date -u +%Y%m%dT%H%M%SZ)"
result_root="$output_root/${timestamp}-$$"
baseline_output="$result_root/baseline"
formatted_output="$result_root/formatted"
comparison_output="$result_root/comparison"
staged_binary_dir="$result_root/release"
staged_binary="$staged_binary_dir/usher"
mkdir -p "$baseline_output" "$formatted_output" "$comparison_output" "$staged_binary_dir"

if (( build_binary == 1 )); then
  host_target="$(rustc -vV | sed -n 's/^host: //p')"
  [[ "$host_target" =~ ^[[:alnum:]_.-]+$ ]] ||
    die "could not resolve the Rust host target: $host_target"
  binary="$repo_root/target/$host_target/release/usher"
  echo "Building the shared Release binary once for target=$host_target ..."
  cargo build \
    --release \
    --locked \
    --target "$host_target" \
    --target-dir "$repo_root/target" 2>&1 | tee "$result_root/build.log"
fi
[[ -x "$binary" ]] || die "Release binary is not executable: $binary"

source_binary="$binary"
source_binary_sha="$(shasum -a 256 "$source_binary" | awk '{print $1}')"
[[ -n "$source_binary_sha" ]] || die "could not fingerprint Release binary: $source_binary"
cp -p "$source_binary" "$staged_binary"
chmod 0555 "$staged_binary"
binary="$staged_binary"

initial_binary_sha="$(shasum -a 256 "$binary" | awk '{print $1}')"
[[ -n "$initial_binary_sha" ]] || die "could not fingerprint Release binary: $binary"
[[ "$initial_binary_sha" == "$source_binary_sha" ]] ||
  die "staged Release binary SHA differs from its source"
instance_namespace="ai-review-response-${timestamp}-$$"
variant_result=""

run_variant() {
  local variant="$1" application_variant="$2" cell_output="$3"
  local driver_log="$result_root/${variant}-driver.log"
  variant_result=""

  echo
  echo "Running variant=$variant with $runs isolated Release processes ..."
  USHER_AI_REVIEW_RESPONSE_FIXTURE=markdown-v1 \
    USHER_AI_REVIEW_RESPONSE_VARIANT="$variant" \
    USHER_AI_REVIEW_RESPONSE_INTERNAL_HOOK=1 \
    USHER_AI_REVIEW_RESPONSE_CELL_OUTPUT="$cell_output" \
    USHER_AI_REVIEW_RESPONSE_HOOK_TIMEOUT_SECONDS="$startup_timeout" \
    USHER_AI_REVIEW_RESPONSE_FIXTURE_MESSAGES="$fixture_messages" \
    USHER_AI_REVIEW_RESPONSE_FIXTURE_BYTES="$fixture_bytes" \
    USHER_AI_REVIEW_RESPONSE_SCROLL_EVENTS="$scroll_events" \
    USHER_AI_REVIEW_RESPONSE_SCROLL_DELTA="$scroll_delta" \
    USHER_BENCHMARK_OFFLINE=1 \
    USHER_BENCHMARK_STATE_ROOT="$cell_output/state" \
    USHER_BENCHMARK_INSTANCE_NAMESPACE="$instance_namespace" \
    RUST_LOG=info \
    "$benchmark_memory" \
      --runs "$runs" \
      --scenarios ai-review-response \
      --renderer femtovg \
      --application-variant "$application_variant" \
      --protocol static \
      --route PrReview \
      --fixture markdown-v1 \
      --prepare-hook "$script_path" \
      --prepare-delay "$settle_seconds" \
      --samples "$samples" \
      --interval "$interval" \
      --startup-timeout "$startup_timeout" \
      --output "$cell_output" \
      --binary "$binary" \
      --no-build 2>&1 | tee "$driver_log"

  local result_dir
  result_dir="$(sed -n 's/^Benchmark complete: //p' "$driver_log" | tail -n 1)"
  [[ -n "$result_dir" && -d "$result_dir" ]] ||
    die "could not resolve benchmark result directory from $driver_log"
  variant_result="$result_dir"
}

assert_binary_unchanged() {
  local phase="$1" current_sha
  current_sha="$(shasum -a 256 "$binary" | awk '{print $1}')"
  [[ "$current_sha" == "$initial_binary_sha" ]] ||
    die "Release binary changed during $phase: initial=$initial_binary_sha current=$current_sha"
}

validate_variant_result() {
  local result_dir="$1" expected_runs="$2" expected_variant="$3" cell_output="$4"
  local required count=0 log

  for required in \
    metadata.tsv \
    raw-samples.tsv \
    run-summary.tsv \
    summary.tsv \
    summary.md; do
    [[ -s "$result_dir/$required" ]] ||
      die "benchmark result is missing required artifact: $result_dir/$required"
  done

  for log in "$result_dir"/runs/ai-review-response-run-*.log; do
    [[ -f "$log" ]] || continue
    ((count += 1))
    fixture_log_has_settled_record \
      "$log" "$expected_variant" "$fixture_messages" "$fixture_bytes" ||
      die "fixture marker does not match markdown-v1 variant=$expected_variant messages=$fixture_messages bytes_per_response=$fixture_bytes persistence=false: $log"
    fixture_log_has_consumed_scroll "$log" "$expected_variant" ||
      die "fixture did not report consumed response-panel scroll input: $log"
    if grep -a -q 'fixture_persistence=true' "$log"; then
      die "fixture unexpectedly reported persistence: $log"
    fi
  done
  [[ "$count" -eq "$expected_runs" ]] ||
    die "expected $expected_runs fixture logs in $result_dir, found $count"

  local run state_dir persisted_file
  for ((run = 1; run <= expected_runs; run++)); do
    state_dir="$cell_output/state/$expected_variant/run-$run"
    [[ -d "$state_dir" ]] ||
      die "isolated fixture state directory is missing: $state_dir"
    persisted_file="$(
      find "$state_dir" -type f \( \
        -name 'ai-review.json' -o \
        -path '*/ai-review/*' \
      \) -print -quit
    )"
    [[ -z "$persisted_file" ]] ||
      die "response fixture wrote AI Review persistence: $persisted_file"
  done
}

run_variant baseline ai-review-response-baseline "$baseline_output"
baseline_result="$variant_result"
validate_variant_result "$baseline_result" "$runs" baseline "$baseline_output"
assert_binary_unchanged "baseline cell"
baseline_sha="$(metadata_value "$baseline_result/metadata.tsv" binary_sha256)"
[[ "$baseline_sha" == "$initial_binary_sha" ]] ||
  die "baseline metadata SHA does not match the initially built binary"

run_variant formatted ai-review-response-formatted "$formatted_output"
formatted_result="$variant_result"
validate_variant_result "$formatted_result" "$runs" formatted "$formatted_output"
assert_binary_unchanged "formatted cell"
formatted_sha="$(metadata_value "$formatted_result/metadata.tsv" binary_sha256)"
[[ "$formatted_sha" == "$initial_binary_sha" ]] ||
  die "formatted metadata SHA does not match the initially built binary"
[[ "$baseline_sha" == "$formatted_sha" ]] ||
  die "A/B cells used different binaries: baseline=$baseline_sha formatted=$formatted_sha"

python3 "$compare_memory" \
  "$baseline_result" \
  "$formatted_result" \
  --ignore-metadata-key observed_routes \
  --ignore-metadata-key observed_ui_contexts \
  --output-dir "$comparison_output"

{
  printf 'key\tvalue\n'
  printf 'created_utc\t%s\n' "$timestamp"
  printf 'fixture\tmarkdown-v1\n'
  printf 'fixture_messages\t%s\n' "$fixture_messages"
  printf 'fixture_bytes\t%s\n' "$fixture_bytes"
  printf 'scenario\tai-review-response\n'
  printf 'window\t1280x860 settled phases\n'
  printf 'input\tbalanced scroll delta=%s events_per_direction=%s\n' "$scroll_delta" "$scroll_events"
  printf 'runs_per_variant\t%s\n' "$runs"
  printf 'samples_per_phase\t%s\n' "$samples"
  printf 'sample_interval_seconds\t%s\n' "$interval"
  printf 'settle_seconds\t%s\n' "$settle_seconds"
  printf 'build_mode\t%s\n' "$build_mode"
  printf 'source_binary\t%s\n' "$source_binary"
  printf 'source_binary_sha256\t%s\n' "$source_binary_sha"
  printf 'staged_binary\t%s\n' "$binary"
  printf 'binary_sha256\t%s\n' "$initial_binary_sha"
  printf 'instance_namespace\t%s\n' "$instance_namespace"
  printf 'benchmark_offline\ttrue\n'
  printf 'benchmark_state\tisolated-per-variant-and-run\n'
  printf 'fixture_persistence_validation\tno-ai-review-files-per-run\n'
  printf 'comparison_ignored_metadata\tobserved_routes,observed_ui_contexts (startup-only observations)\n'
  printf 'baseline_result\t%s\n' "$baseline_result"
  printf 'formatted_result\t%s\n' "$formatted_result"
  printf 'maximum_increase_percent\t%s\n' "$max_percent"
  printf 'maximum_increase_mib\t%s\n' "$max_mib"
} >"$result_root/metadata.tsv"

gate_status=0
python3 - \
  "$comparison_output/comparison.tsv" \
  "$comparison_output/acceptance.tsv" \
  "$comparison_output/acceptance.md" \
  "$max_percent" \
  "$max_mib" \
  "$initial_binary_sha" \
  "$runs" <<'PY' || gate_status=$?
import csv
import sys
from pathlib import Path

comparison_path = Path(sys.argv[1])
tsv_path = Path(sys.argv[2])
markdown_path = Path(sys.argv[3])
max_percent = float(sys.argv[4])
max_mib = float(sys.argv[5])
binary_sha = sys.argv[6]
runs = int(sys.argv[7])

with comparison_path.open(newline="", encoding="utf-8") as handle:
    comparisons = list(csv.DictReader(handle, delimiter="\t"))

settled = [row for row in comparisons if row["phase"] == "settled"]
if len(settled) != 2 or {int(row["cycle"]) for row in settled} != {1, 2}:
    raise SystemExit(
        "expected exactly two settled static cycles, found "
        + ", ".join(f"{row['phase']}:{row['cycle']}" for row in settled)
    )

limit_bytes = max_mib * 1024.0 * 1024.0
records = []
failures = []
for row in sorted(settled, key=lambda item: int(item["cycle"])):
    metrics = (
        (
            "RSS settled median",
            float(row["baseline_rss_settled_kib"]) * 1024.0,
            float(row["candidate_rss_settled_kib"]) * 1024.0,
            float(row["rss_settled_delta_kib"]) * 1024.0,
            float(row["rss_settled_delta_percent"]),
        ),
        (
            "Physical footprint settled median",
            float(row["baseline_footprint_settled_bytes"]),
            float(row["candidate_footprint_settled_bytes"]),
            float(row["footprint_settled_delta_bytes"]),
            float(row["footprint_settled_delta_percent"]),
        ),
    )
    for metric, baseline, candidate, delta, delta_percent in metrics:
        increase = max(delta, 0.0)
        increase_percent = max(delta_percent, 0.0)
        relative_pass = increase_percent <= max_percent
        absolute_pass = increase <= limit_bytes
        passed = relative_pass and absolute_pass
        record = {
            "scenario": row["scenario"],
            "phase": row["phase"],
            "cycle": row["cycle"],
            "metric": metric,
            "baseline_bytes": int(baseline),
            "candidate_bytes": int(candidate),
            "delta_bytes": int(delta),
            "delta_mib": f"{delta / (1024.0 * 1024.0):.4f}",
            "delta_percent": f"{delta_percent:.4f}",
            "relative_limit_percent": f"{max_percent:.4f}",
            "absolute_limit_mib": f"{max_mib:.4f}",
            "relative_pass": str(relative_pass).lower(),
            "absolute_pass": str(absolute_pass).lower(),
            "result": "pass" if passed else "fail",
        }
        records.append(record)
        if not passed:
            failures.append(
                f"cycle {row['cycle']} {metric}: {delta / (1024.0 * 1024.0):+.2f} MiB "
                f"({delta_percent:+.2f}%)"
            )

with tsv_path.open("w", newline="", encoding="utf-8") as handle:
    writer = csv.DictWriter(handle, fieldnames=list(records[0]), delimiter="\t")
    writer.writeheader()
    writer.writerows(records)

lines = [
    "# AI Review response memory acceptance",
    "",
    f"Same Release binary SHA: `{binary_sha}`. Each variant used {runs} isolated processes.",
    "The gate covers the two settled static periods after the markdown fixture, 1280x860 window, and balanced scroll input were prepared.",
    "Startup-visible samples remain in the raw benchmark but are excluded because benchmark-memory records them before its prepare hook.",
    "",
    f"Limits: candidate increase <= {max_percent:.2f}% and <= {max_mib:.2f} MiB; both must pass for RSS and physical footprint.",
    "",
    "| Cycle | Metric | Baseline | Formatted | Delta | Delta % | Relative | Absolute | Result |",
    "| ---: | --- | ---: | ---: | ---: | ---: | --- | --- | --- |",
]
for record in records:
    baseline_mib = int(record["baseline_bytes"]) / (1024.0 * 1024.0)
    candidate_mib = int(record["candidate_bytes"]) / (1024.0 * 1024.0)
    lines.append(
        f"| {record['cycle']} | {record['metric']} | {baseline_mib:.2f} MiB | "
        f"{candidate_mib:.2f} MiB | {float(record['delta_mib']):+.2f} MiB | "
        f"{float(record['delta_percent']):+.2f}% | {record['relative_pass']} | "
        f"{record['absolute_pass']} | **{record['result']}** |"
    )
lines.extend(["", "Overall: **PASS**" if not failures else "Overall: **FAIL**", ""])
markdown_path.write_text("\n".join(lines), encoding="utf-8")
print("\n".join(lines))

if failures:
    raise SystemExit("AI Review response memory acceptance failed:\n- " + "\n- ".join(failures))
PY

echo
echo "Results: $result_root"
echo "Raw baseline: $baseline_result/raw-samples.tsv"
echo "Raw formatted: $formatted_result/raw-samples.tsv"
echo "Comparison: $comparison_output/comparison.md"
echo "Acceptance: $comparison_output/acceptance.md"

if (( gate_status != 0 )); then
  exit "$gate_status"
fi

assert_binary_unchanged "acceptance reporting"
