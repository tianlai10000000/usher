#!/usr/bin/env bash
set -euo pipefail

runs="${1:-5}"
if ! [[ "${runs}" =~ ^[1-9][0-9]*$ ]]; then
  echo "usage: $0 [positive-run-count]" >&2
  exit 2
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
results_file="$(mktemp "${TMPDIR:-/tmp}/usher-pr-memory-ab.XXXXXX")"
trap 'rm -f "${results_file}"' EXIT

cd "${repo_root}"

echo "PR Review selected-file memory A/B (${runs} isolated processes per cell)"
echo "Fixture: 206 files, 27,369 total lines, 112 content bytes per line"

for selected_lines in 2932 24843; do
  for mode in baseline candidate; do
    for ((run = 1; run <= runs; run++)); do
      output="$(
        USHER_PR_MEMORY_AB_MODE="${mode}" \
          USHER_PR_MEMORY_AB_SELECTED_LINES="${selected_lines}" \
          cargo test --lib --locked selected_file_memory_ab_sample \
            -- --ignored --nocapture --test-threads=1 2>&1
      )"
      row="$(printf '%s\n' "${output}" | sed -n 's/^.*PR_MEMORY_AB /PR_MEMORY_AB /p' | tail -n 1)"
      if [[ -z "${row}" ]]; then
        printf '%s\n' "${output}" >&2
        echo "missing PR_MEMORY_AB result for mode=${mode} selected_lines=${selected_lines}" >&2
        exit 1
      fi
      printf '%s run=%s\n' "${row}" "${run}" | tee -a "${results_file}"
    done
  done
done

python3 - "${results_file}" <<'PY'
import statistics
import sys

path = sys.argv[1]
rows = []
with open(path, encoding="utf-8") as handle:
    for raw in handle:
        fields = {}
        for item in raw.strip().split()[1:]:
            key, value = item.split("=", 1)
            fields[key] = value
        rows.append(fields)

metrics = ("rss_delta_bytes", "footprint_delta_bytes", "owned_heap_bytes")
groups = {}
for row in rows:
    key = (int(row["selected_lines"]), row["mode"])
    groups.setdefault(key, {metric: [] for metric in metrics})
    for metric in metrics:
        groups[key][metric].append(int(row[metric]))

def median(key, metric):
    return int(statistics.median(groups[key][metric]))

def mib(value):
    return value / (1024 * 1024)

def reduction(baseline, candidate):
    saved = baseline - candidate
    percent = saved * 100 / baseline if baseline else 0
    return saved, percent

print()
print("| Selected file | Variant | Median RSS Δ | Median footprint Δ | Owned heap |")
print("| ---: | --- | ---: | ---: | ---: |")
for selected_lines in sorted({key[0] for key in groups}):
    for mode in ("baseline", "candidate"):
        key = (selected_lines, mode)
        print(
            f"| {selected_lines:,} lines | {mode} | "
            f"{mib(median(key, 'rss_delta_bytes')):.2f} MiB | "
            f"{mib(median(key, 'footprint_delta_bytes')):.2f} MiB | "
            f"{mib(median(key, 'owned_heap_bytes')):.2f} MiB |"
        )
    baseline_key = (selected_lines, "baseline")
    candidate_key = (selected_lines, "candidate")
    rss_saved, rss_percent = reduction(
        median(baseline_key, "rss_delta_bytes"),
        median(candidate_key, "rss_delta_bytes"),
    )
    footprint_saved, footprint_percent = reduction(
        median(baseline_key, "footprint_delta_bytes"),
        median(candidate_key, "footprint_delta_bytes"),
    )
    heap_saved, heap_percent = reduction(
        median(baseline_key, "owned_heap_bytes"),
        median(candidate_key, "owned_heap_bytes"),
    )
    print(
        f"| {selected_lines:,} lines | **saved** | "
        f"**{mib(rss_saved):.2f} MiB ({rss_percent:.1f}%)** | "
        f"**{mib(footprint_saved):.2f} MiB ({footprint_percent:.1f}%)** | "
        f"**{mib(heap_saved):.2f} MiB ({heap_percent:.1f}%)** |"
    )
PY
