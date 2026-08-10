#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/benchmark-memory.sh [options]

Run the two-cycle macOS memory benchmark against isolated release processes.

Options:
  --runs N                 Isolated processes per scenario (default: 3)
  --scenarios LIST         Comma-separated scenario labels (default: home)
  --renderer NAME          femtovg, femtovg-wgpu, or skia (default: femtovg)
  --application-variant ID Application ownership label (default: usher)
  --tokio-workers VALUE    default or a positive worker count (default: default)
  --workflow-lifecycle MODE normal, retain, or clear (default: normal)
  --protocol NAME          window-cycle or static (default: window-cycle)
  --route LABEL            Expected route label recorded in metadata (default: Home)
  --fixture LABEL          Fixture/version label recorded in metadata (default: local)
  --prepare-hook PATH      Executable called as: PATH PID RUN SCENARIO
  --prepare-delay SECONDS  Extra settling time after the hook (default: 2)
  --samples N              Samples retained per phase (default: 5)
  --interval SECONDS       Delay between samples (default: 0.5)
  --startup-timeout SEC    Window readiness timeout (default: 20)
  --output DIR             Result root (default: memory-benchmark-results)
  --binary PATH            Binary to run (default: target/release/usher)
  --no-build               Reuse the selected binary without building
  --allow-non-release      Permit a binary outside a release target directory
  -h, --help               Show this help

The window-cycle protocol is startup-visible, then two identical cycles of
800x600, 1000x700, 1280x860, fullscreen, and exit-fullscreen, followed by a
final 800x600 sample. The static protocol records two settled periods without
Accessibility automation. A preparation hook is required to make non-Home
fixtures truly repeatable; application-owned model counts and actual
route/scale are retained in each run's stdout log.
EOF
}

die() {
  echo "error: $*" >&2
  exit 1
}

require_positive_integer() {
  [[ "$2" =~ ^[1-9][0-9]*$ ]] || die "$1 must be a positive integer: $2"
}

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
runs=3
scenarios="home"
renderer="femtovg"
application_variant="usher"
tokio_workers="default"
workflow_lifecycle="normal"
protocol="window-cycle"
route_label="Home"
fixture_label="local"
prepare_hook=""
prepare_delay=2
samples_per_phase=5
sample_interval=0.5
startup_timeout=20
output_root="${repo_root}/memory-benchmark-results"
binary="${repo_root}/target/release/usher"
build_binary=1
allow_non_release=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --runs) runs="${2:?missing value for --runs}"; shift 2 ;;
    --scenarios) scenarios="${2:?missing value for --scenarios}"; shift 2 ;;
    --renderer) renderer="${2:?missing value for --renderer}"; shift 2 ;;
    --application-variant) application_variant="${2:?missing value for --application-variant}"; shift 2 ;;
    --tokio-workers) tokio_workers="${2:?missing value for --tokio-workers}"; shift 2 ;;
    --workflow-lifecycle) workflow_lifecycle="${2:?missing value for --workflow-lifecycle}"; shift 2 ;;
    --protocol) protocol="${2:?missing value for --protocol}"; shift 2 ;;
    --route) route_label="${2:?missing value for --route}"; shift 2 ;;
    --fixture) fixture_label="${2:?missing value for --fixture}"; shift 2 ;;
    --prepare-hook) prepare_hook="${2:?missing value for --prepare-hook}"; shift 2 ;;
    --prepare-delay) prepare_delay="${2:?missing value for --prepare-delay}"; shift 2 ;;
    --samples) samples_per_phase="${2:?missing value for --samples}"; shift 2 ;;
    --interval) sample_interval="${2:?missing value for --interval}"; shift 2 ;;
    --startup-timeout) startup_timeout="${2:?missing value for --startup-timeout}"; shift 2 ;;
    --output) output_root="${2:?missing value for --output}"; shift 2 ;;
    --binary) binary="${2:?missing value for --binary}"; shift 2 ;;
    --no-build) build_binary=0; shift ;;
    --allow-non-release) allow_non_release=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) die "unknown option: $1" ;;
  esac
done

[[ "$(uname -s)" == "Darwin" ]] || die "this benchmark currently requires macOS"
command -v footprint >/dev/null 2>&1 || die "footprint(1) is required"
command -v python3 >/dev/null 2>&1 || die "python3 is required for aggregation"
command -v shasum >/dev/null 2>&1 || die "shasum is required for source fingerprinting"
command -v swiftc >/dev/null 2>&1 || die "swiftc is required for AX window automation"
require_positive_integer "--runs" "$runs"
require_positive_integer "--samples" "$samples_per_phase"
require_positive_integer "--startup-timeout" "$startup_timeout"
[[ "$prepare_delay" =~ ^[0-9]+([.][0-9]+)?$ ]] || die "--prepare-delay must be non-negative"
[[ "$sample_interval" =~ ^[0-9]+([.][0-9]+)?$ ]] || die "--interval must be non-negative"
[[ -z "$prepare_hook" || -x "$prepare_hook" ]] || die "preparation hook is not executable: $prepare_hook"
[[ -n "$scenarios" ]] || die "--scenarios cannot be empty"
[[ "$application_variant" =~ ^[[:alnum:]_.:-]+$ ]] ||
  die "--application-variant may contain only letters, digits, dot, underscore, colon, and hyphen"
[[ "$tokio_workers" == "default" || "$tokio_workers" =~ ^[1-9][0-9]*$ ]] ||
  die "--tokio-workers must be default or a positive integer"
[[ "$workflow_lifecycle" == "normal" || "$workflow_lifecycle" == "retain" || "$workflow_lifecycle" == "clear" ]] ||
  die "--workflow-lifecycle must be normal, retain, or clear"
[[ "$protocol" == "window-cycle" || "$protocol" == "static" ]] ||
  die "--protocol must be window-cycle or static"

pr_review_fixture=""
if [[ "$workflow_lifecycle" != "normal" ]]; then
  pr_review_fixture="large-ranges-v1"
fi

case "$renderer" in
  femtovg) backend="winit-femtovg"; cargo_features=() ;;
  femtovg-wgpu) backend="winit-femtovg-wgpu"; cargo_features=(--no-default-features --features renderer-femtovg-wgpu) ;;
  skia) backend="winit-skia"; cargo_features=(--no-default-features --features renderer-skia) ;;
  *) die "unsupported renderer: $renderer" ;;
esac

if [[ "$binary" != /* ]]; then
  binary="${repo_root}/${binary}"
fi
if [[ "$allow_non_release" -ne 1 && "$binary" != */release/* ]]; then
  die "refusing non-release binary path (pass --allow-non-release to override): $binary"
fi
build_profile=release
if [[ "$binary" != */release/* ]]; then
  build_profile=non-release-override
fi

cd "$repo_root"
if [[ "$build_binary" -eq 1 ]]; then
  echo "Building release renderer=$renderer ..."
  cargo build --release --locked "${cargo_features[@]}"
fi
[[ -x "$binary" ]] || die "binary is not executable: $binary"

timestamp="$(date -u +%Y%m%dT%H%M%SZ)"
scenario_slug="$(printf '%s' "$scenarios" | tr -cs '[:alnum:]_.-' '_')"
result_dir="${output_root}/${timestamp}-${application_variant}-${renderer}-${workflow_lifecycle}-${scenario_slug}"
mkdir -p "$result_dir/runs"
window_control="$result_dir/macos-window-control"
swiftc -O "$repo_root/scripts/macos-window-control.swift" -o "$window_control"
raw_tsv="$result_dir/raw-samples.tsv"
metadata_tsv="$result_dir/metadata.tsv"
observed_fullscreen_file="$result_dir/observed-fullscreen-sizes.txt"
: >"$observed_fullscreen_file"
printf 'timestamp_utc\tscenario\trun\tphase\tcycle\tsample\telapsed_ms\trss_kib\tfootprint_bytes\n' >"$raw_tsv"

git_dirty=false
[[ -z "$(git status --porcelain)" ]] || git_dirty=true
worktree_fingerprint="$(
  {
    git diff --binary HEAD
    while IFS= read -r path; do
      printf 'untracked\t%s\t' "$path"
      shasum -a 256 "$path"
    done < <(git ls-files --others --exclude-standard)
  } | shasum -a 256 | awk '{print $1}'
)"
display_info="$(system_profiler SPDisplaysDataType 2>/dev/null | awk -F: '/Resolution:/{sub(/^[[:space:]]+/, "", $2); print $2; exit}')"
{
  printf 'key\tvalue\n'
  printf 'created_utc\t%s\n' "$timestamp"
  printf 'git_commit\t%s\n' "$(git rev-parse HEAD)"
  printf 'git_dirty\t%s\n' "$git_dirty"
  printf 'worktree_fingerprint\t%s\n' "$worktree_fingerprint"
  printf 'macos\t%s (%s)\n' "$(sw_vers -productVersion)" "$(sw_vers -buildVersion)"
  printf 'hardware\t%s\n' "$(sysctl -n hw.model 2>/dev/null || uname -m)"
  printf 'architecture\t%s\n' "$(uname -m)"
  printf 'display\t%s\n' "${display_info:-unknown}"
  printf 'display_scale_source\t[Memory][Diag] scale_factor in run stdout\n'
  printf 'renderer\t%s\n' "$renderer"
  printf 'slint_backend\t%s\n' "$backend"
  printf 'application_variant\t%s\n' "$application_variant"
  printf 'tokio_worker_threads\t%s\n' "$tokio_workers"
  printf 'workflow_lifecycle\t%s\n' "$workflow_lifecycle"
  printf 'protocol\t%s\n' "$protocol"
  printf 'build_profile\t%s\n' "$build_profile"
  printf 'binary\t%s\n' "$binary"
  printf 'binary_sha256\t%s\n' "$(shasum -a 256 "$binary" | awk '{print $1}')"
  printf 'scenarios\t%s\n' "$scenarios"
  printf 'expected_route\t%s\n' "$route_label"
  printf 'fixture\t%s\n' "$fixture_label"
  printf 'model_counts_source\t[Memory][UI Models] in run stdout\n'
  printf 'runs\t%s\n' "$runs"
  printf 'samples_per_phase\t%s\n' "$samples_per_phase"
  printf 'sample_interval_seconds\t%s\n' "$sample_interval"
  printf 'startup_timeout_seconds\t%s\n' "$startup_timeout"
  printf 'prepare_hook\t%s\n' "${prepare_hook:-none}"
  printf 'prepare_delay_seconds\t%s\n' "$prepare_delay"
  printf 'window_control\t%s\n' 'signal-file-v1+macos-cg-state-v1'
} >"$metadata_tsv"

active_pid=""
active_control_file=""
cleanup() {
  if [[ -n "$active_pid" ]] && kill -0 "$active_pid" 2>/dev/null; then
    kill -TERM "$active_pid" 2>/dev/null || true
  fi
}
trap cleanup EXIT INT TERM

window_count() {
  "$window_control" count "$1" 2>/dev/null
}

wait_for_ready() {
  local pid="$1" run_log="$2" waited=0
  while (( waited < startup_timeout * 10 )); do
    kill -0 "$pid" 2>/dev/null || return 1
    if grep -a -q '\[Startup\] Window revealed' "$run_log"; then
      if [[ "$protocol" == "static" ]] ||
        [[ "$(window_count "$pid" || true)" =~ ^[1-9][0-9]*$ ]]; then
        return 0
      fi
    fi
    sleep 0.1
    ((waited += 1))
  done
  return 1
}

resize_window() {
  local pid="$1" width="$2" height="$3"
  printf 'resize %s %s\n' "$width" "$height" >"$active_control_file"
  kill -USR1 "$pid"
  "$window_control" wait-size "$pid" "$width" "$height" 10000
}

set_fullscreen() {
  local pid="$1" enabled="$2" attempt state
  for attempt in 1 2 3; do
    state="$("$window_control" fullscreen-state "$pid")"
    if [[ "$state" == "$enabled" ]]; then
      if [[ "$enabled" == "true" ]]; then
        "$window_control" window-size "$pid" >>"$observed_fullscreen_file"
      fi
      return
    fi
    printf 'toggle-fullscreen\n' >"$active_control_file"
    kill -USR1 "$pid"
    if "$window_control" wait-fullscreen "$pid" "$enabled" 10000; then
      if [[ "$enabled" == "true" ]]; then
        "$window_control" window-size "$pid" >>"$observed_fullscreen_file"
      fi
      return
    fi
    sleep 1
  done
  die "window did not reach fullscreen=$enabled after 3 attempts"
}

footprint_bytes() {
  local output value
  output="$(footprint -f bytes --noCategories "$1" 2>/dev/null)" || return 1
  value="$(printf '%s\n' "$output" | awk '
    match($0, /Footprint:[[:space:]]*[0-9,]+/) {
      value = substr($0, RSTART, RLENGTH)
      sub(/^Footprint:[[:space:]]*/, "", value)
      gsub(/,/, "", value)
      print value
      exit
    }
  ')"
  [[ "$value" =~ ^[0-9]+$ ]] || return 1
  printf '%s\n' "$value"
}

sample_phase() {
  local pid="$1" scenario="$2" run="$3" phase="$4" cycle="$5"
  local sample rss footprint now elapsed
  for ((sample = 1; sample <= samples_per_phase; sample++)); do
    kill -0 "$pid" 2>/dev/null || die "process $pid exited during $scenario run $run"
    rss="$(ps -p "$pid" -o rss= | tr -d ' ')"
    [[ "$rss" =~ ^[0-9]+$ ]] || die "failed to sample RSS for PID $pid"
    footprint="$(footprint_bytes "$pid")" ||
      die "footprint could not inspect PID $pid; grant the invoking terminal process-inspection permission"
    now="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    elapsed="$(( $(date +%s) - process_started_epoch ))000"
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
      "$now" "$scenario" "$run" "$phase" "$cycle" "$sample" "$elapsed" "$rss" "$footprint" >>"$raw_tsv"
    sleep "$sample_interval"
  done
}

settle_and_sample() {
  sleep "$prepare_delay"
  sample_phase "$@"
}

stop_process() {
  local pid="$1" attempts=0
  kill -TERM "$pid" 2>/dev/null || true
  while kill -0 "$pid" 2>/dev/null && (( attempts < 100 )); do
    sleep 0.1
    ((attempts += 1))
  done
  if kill -0 "$pid" 2>/dev/null; then
    kill -KILL "$pid" 2>/dev/null || true
  fi
  wait "$pid" 2>/dev/null || true
  active_pid=""
}

IFS=',' read -r -a scenario_items <<<"$scenarios"
for scenario in "${scenario_items[@]}"; do
  [[ -n "$scenario" ]] || die "empty scenario in --scenarios"
  [[ "$scenario" =~ ^[[:alnum:]_.:-]+$ ]] ||
    die "scenario labels may contain only letters, digits, dot, underscore, colon, and hyphen: $scenario"
  for ((run = 1; run <= runs; run++)); do
    run_log="$result_dir/runs/${scenario}-run-${run}.log"
    active_control_file="$result_dir/runs/${scenario}-run-${run}.control"
    : >"$active_control_file"
    echo "Running scenario=$scenario run=$run/$runs ..."
    process_started_epoch="$(date +%s)"
    USHER_MEMORY_DIAGNOSTICS=1 \
      USHER_MEMORY_SCENARIO="$scenario" \
      USHER_MEMORY_BENCHMARK_RUN="$run" \
      USHER_MEMORY_BUILD_PROFILE="$build_profile" \
      USHER_MEMORY_WINDOW_CONTROL=signal-file-v1 \
      USHER_MEMORY_WINDOW_CONTROL_FILE="$active_control_file" \
      USHER_TOKIO_WORKER_THREADS="$tokio_workers" \
      USHER_MEMORY_WORKFLOW_LIFECYCLE="$workflow_lifecycle" \
      USHER_MEMORY_PR_REVIEW_FIXTURE="$pr_review_fixture" \
      SLINT_BACKEND="$backend" \
      "$binary" >"$run_log" 2>&1 &
    active_pid=$!

    if ! wait_for_ready "$active_pid" "$run_log"; then
      sed -n '1,120p' "$run_log" >&2
      die "Usher window did not become accessible; close any existing instance and grant Accessibility permission"
    fi
    sample_phase "$active_pid" "$scenario" "$run" startup_visible 0

    if [[ -n "$prepare_hook" ]]; then
      "$prepare_hook" "$active_pid" "$run" "$scenario"
    elif [[ "$scenario" != "home" && "$workflow_lifecycle" == "normal" ]]; then
      echo "warning: scenario=$scenario has no preparation hook; fixture setup is not reproducible" >&2
    fi
    sleep "$prepare_delay"

    if [[ "$protocol" == "static" ]]; then
      settle_and_sample "$active_pid" "$scenario" "$run" settled 1
      settle_and_sample "$active_pid" "$scenario" "$run" settled 2
      stop_process "$active_pid"
      continue
    fi

    for cycle in 1 2; do
      resize_window "$active_pid" 800 600
      settle_and_sample "$active_pid" "$scenario" "$run" small "$cycle"
      resize_window "$active_pid" 1000 700
      settle_and_sample "$active_pid" "$scenario" "$run" medium "$cycle"
      resize_window "$active_pid" 1280 860
      settle_and_sample "$active_pid" "$scenario" "$run" large "$cycle"
      set_fullscreen "$active_pid" true
      settle_and_sample "$active_pid" "$scenario" "$run" fullscreen "$cycle"
      set_fullscreen "$active_pid" false
      settle_and_sample "$active_pid" "$scenario" "$run" exit_fullscreen "$cycle"
    done
    resize_window "$active_pid" 800 600
    settle_and_sample "$active_pid" "$scenario" "$run" final_small 0
    stop_process "$active_pid"
  done
done

python3 - "$result_dir/runs" "$protocol" "$observed_fullscreen_file" >>"$metadata_tsv" <<'PY'
import re
import sys
from pathlib import Path

runs_dir = Path(sys.argv[1])
protocol = sys.argv[2]
fullscreen_path = Path(sys.argv[3])
pattern = re.compile(
    r"\[Memory\]\[UI\] route=(\S+) logical_width=(\d+) logical_height=(\d+) "
    r"scale_factor=([0-9.]+)"
)
contexts = set()
routes = set()
scales = set()
fullscreen_sizes = set()
for path in sorted(runs_dir.glob("*.log")):
    text = path.read_text(encoding="utf-8", errors="replace")
    for route, width_text, height_text, scale_text in pattern.findall(text):
        width = int(width_text)
        height = int(height_text)
        scale = f"{float(scale_text):.2f}"
        routes.add(route)
        scales.add(scale)
        contexts.add(f"{route}:{width}x{height}@{scale}")
        if width > 1280 or height > 860:
            fullscreen_sizes.add(f"{width}x{height}@{scale}")

native_fullscreen_sizes = {
    line.strip()
    for line in fullscreen_path.read_text(encoding="utf-8").splitlines()
    if line.strip()
}
for size in native_fullscreen_sizes:
    for scale in scales:
        fullscreen_sizes.add(f"{size}@{scale}")
        for route in routes:
            contexts.add(f"{route}:{size}@{scale}")

if not contexts:
    raise SystemExit("no [Memory][UI] contexts found in benchmark run logs")
if protocol == "window-cycle" and not fullscreen_sizes:
    raise SystemExit("window-cycle did not record a verified fullscreen size")

print("observed_routes\t" + ",".join(sorted(routes)))
print("observed_scale_factors\t" + ",".join(sorted(scales)))
print("observed_ui_contexts\t" + ",".join(sorted(contexts)))
print("observed_fullscreen_sizes\t" + (",".join(sorted(fullscreen_sizes)) or "none"))
PY

python3 - "$raw_tsv" "$result_dir/run-summary.tsv" "$result_dir/summary.tsv" "$result_dir/summary.md" <<'PY'
import csv
import statistics
import sys
from collections import defaultdict

raw_path, run_path, summary_path, markdown_path = sys.argv[1:]
with open(raw_path, newline="", encoding="utf-8") as handle:
    rows = list(csv.DictReader(handle, delimiter="\t"))

groups = defaultdict(list)
for row in rows:
    key = (row["scenario"], int(row["run"]), row["phase"], int(row["cycle"]))
    groups[key].append(row)

per_run = []
for (scenario, run, phase, cycle), samples in sorted(groups.items()):
    rss = [int(row["rss_kib"]) for row in samples]
    footprint = [int(row["footprint_bytes"]) for row in samples]
    tail = min(3, len(samples))
    per_run.append({
        "scenario": scenario,
        "run": run,
        "phase": phase,
        "cycle": cycle,
        "rss_peak_kib": max(rss),
        "rss_settled_median_kib": int(statistics.median(rss[-tail:])),
        "footprint_peak_bytes": max(footprint),
        "footprint_settled_median_bytes": int(statistics.median(footprint[-tail:])),
    })

run_fields = list(per_run[0]) if per_run else []
with open(run_path, "w", newline="", encoding="utf-8") as handle:
    writer = csv.DictWriter(handle, fieldnames=run_fields, delimiter="\t")
    writer.writeheader()
    writer.writerows(per_run)

cross_groups = defaultdict(list)
for row in per_run:
    cross_groups[(row["scenario"], row["phase"], row["cycle"])].append(row)

summary = []
for (scenario, phase, cycle), values in sorted(cross_groups.items()):
    summary.append({
        "scenario": scenario,
        "phase": phase,
        "cycle": cycle,
        "runs": len(values),
        "rss_settled_median_kib": int(statistics.median(v["rss_settled_median_kib"] for v in values)),
        "rss_peak_max_kib": max(v["rss_peak_kib"] for v in values),
        "footprint_settled_median_bytes": int(statistics.median(v["footprint_settled_median_bytes"] for v in values)),
        "footprint_peak_max_bytes": max(v["footprint_peak_bytes"] for v in values),
    })

summary_fields = list(summary[0]) if summary else []
with open(summary_path, "w", newline="", encoding="utf-8") as handle:
    writer = csv.DictWriter(handle, fieldnames=summary_fields, delimiter="\t")
    writer.writeheader()
    writer.writerows(summary)

by_run = {(r["scenario"], r["run"], r["phase"], r["cycle"]): r for r in per_run}
slopes = []
for scenario in sorted({r["scenario"] for r in per_run}):
    phases = sorted(
        {
            r["phase"]
            for r in per_run
            if r["scenario"] == scenario and r["cycle"] in (1, 2)
        }
    )
    for phase in phases:
        rss_values, footprint_values = [], []
        for run in sorted({r["run"] for r in per_run if r["scenario"] == scenario}):
            first = by_run.get((scenario, run, phase, 1))
            second = by_run.get((scenario, run, phase, 2))
            if first and second:
                rss_values.append(second["rss_settled_median_kib"] - first["rss_settled_median_kib"])
                footprint_values.append(second["footprint_settled_median_bytes"] - first["footprint_settled_median_bytes"])
        if rss_values:
            slopes.append((scenario, phase, statistics.median(rss_values), statistics.median(footprint_values)))

def mib_from_kib(value):
    return value / 1024

def mib_from_bytes(value):
    return value / (1024 * 1024)

with open(markdown_path, "w", encoding="utf-8") as handle:
    handle.write("# Usher memory benchmark\n\n")
    handle.write("Settled values are the median of the final three samples in each phase. ")
    handle.write("Peak is the maximum observed across isolated processes.\n\n")
    handle.write("| Scenario | Phase | Cycle | Runs | RSS settled median | RSS peak max | Footprint settled median | Footprint peak max |\n")
    handle.write("| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |\n")
    for row in summary:
        handle.write(
            f"| {row['scenario']} | {row['phase']} | {row['cycle']} | {row['runs']} | "
            f"{mib_from_kib(row['rss_settled_median_kib']):.2f} MiB | "
            f"{mib_from_kib(row['rss_peak_max_kib']):.2f} MiB | "
            f"{mib_from_bytes(row['footprint_settled_median_bytes']):.2f} MiB | "
            f"{mib_from_bytes(row['footprint_peak_max_bytes']):.2f} MiB |\n"
        )
    handle.write("\n## Cycle 2 minus cycle 1 settled slope\n\n")
    handle.write("| Scenario | Phase | RSS median slope | Footprint median slope |\n")
    handle.write("| --- | --- | ---: | ---: |\n")
    for scenario, phase, rss, footprint in slopes:
        handle.write(
            f"| {scenario} | {phase} | {mib_from_kib(rss):+.2f} MiB | "
            f"{mib_from_bytes(footprint):+.2f} MiB |\n"
        )
PY

echo "Benchmark complete: $result_dir"
echo "Summary: $result_dir/summary.md"
