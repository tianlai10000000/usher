#!/usr/bin/env python3
"""Compare two complete Usher memory benchmark result directories."""

from __future__ import annotations

import argparse
import csv
import statistics
from collections import defaultdict
from pathlib import Path


MATCHING_METADATA = (
    "git_commit",
    "git_dirty",
    "worktree_fingerprint",
    "macos",
    "hardware",
    "architecture",
    "display",
    "protocol",
    "scenarios",
    "expected_route",
    "fixture",
    "runs",
    "samples_per_phase",
    "sample_interval_seconds",
    "startup_timeout_seconds",
    "prepare_hook",
    "prepare_delay_seconds",
    "window_control",
    "observed_routes",
    "observed_scale_factors",
    "observed_ui_contexts",
    "observed_fullscreen_sizes",
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Compare candidate minus baseline RSS/footprint results. "
            "Negative deltas mean the candidate used less memory."
        )
    )
    parser.add_argument("baseline", type=Path, help="baseline result directory")
    parser.add_argument("candidate", type=Path, help="candidate result directory")
    parser.add_argument(
        "--output-dir",
        type=Path,
        help="comparison output directory (default: candidate/ab-vs-BASELINE)",
    )
    return parser.parse_args()


def read_metadata(result_dir: Path) -> dict[str, str]:
    path = result_dir / "metadata.tsv"
    with path.open(newline="", encoding="utf-8") as handle:
        return {
            row["key"]: row["value"]
            for row in csv.DictReader(handle, delimiter="\t")
        }


def read_rows(path: Path) -> list[dict[str, str]]:
    with path.open(newline="", encoding="utf-8") as handle:
        return list(csv.DictReader(handle, delimiter="\t"))


def validate_metadata(
    baseline: dict[str, str], candidate: dict[str, str]
) -> None:
    errors = []
    for key in MATCHING_METADATA:
        if baseline.get(key) != candidate.get(key):
            errors.append(
                f"metadata mismatch for {key}: "
                f"{baseline.get(key)!r} != {candidate.get(key)!r}"
            )
    for label, metadata in (("baseline", baseline), ("candidate", candidate)):
        if metadata.get("build_profile") != "release":
            errors.append(f"{label} build_profile is not release")
        try:
            if int(metadata.get("runs", "0")) < 3:
                errors.append(f"{label} has fewer than three isolated runs")
        except ValueError:
            errors.append(f"{label} has an invalid run count")
    changed_dimensions = []
    if (
        baseline.get("renderer"),
        baseline.get("slint_backend"),
    ) != (
        candidate.get("renderer"),
        candidate.get("slint_backend"),
    ):
        changed_dimensions.append("renderer")
    if baseline.get("application_variant", "usher") != candidate.get(
        "application_variant", "usher"
    ):
        changed_dimensions.append("application_variant")
    if baseline.get("tokio_worker_threads", "default") != candidate.get(
        "tokio_worker_threads", "default"
    ):
        changed_dimensions.append("tokio_worker_threads")
    if baseline.get("workflow_lifecycle", "normal") != candidate.get(
        "workflow_lifecycle", "normal"
    ):
        changed_dimensions.append("workflow_lifecycle")
    if len(changed_dimensions) != 1:
        errors.append(
            "expected exactly one candidate variable; changed dimensions are "
            + (", ".join(changed_dimensions) if changed_dimensions else "none")
        )
    if errors:
        raise SystemExit("incomparable benchmark results:\n- " + "\n- ".join(errors))


def indexed_summary(rows: list[dict[str, str]]) -> dict[tuple[str, str, int], dict[str, str]]:
    return {
        (row["scenario"], row["phase"], int(row["cycle"])): row
        for row in rows
    }


def percent(delta: float, baseline: float) -> float:
    return delta * 100.0 / baseline if baseline else 0.0


def compare_summary(
    baseline_rows: list[dict[str, str]], candidate_rows: list[dict[str, str]]
) -> list[dict[str, object]]:
    baseline = indexed_summary(baseline_rows)
    candidate = indexed_summary(candidate_rows)
    if baseline.keys() != candidate.keys():
        missing_candidate = sorted(baseline.keys() - candidate.keys())
        missing_baseline = sorted(candidate.keys() - baseline.keys())
        raise SystemExit(
            "incomparable phase matrix: "
            f"missing candidate={missing_candidate}, missing baseline={missing_baseline}"
        )

    compared = []
    for scenario, phase, cycle in sorted(baseline):
        before = baseline[(scenario, phase, cycle)]
        after = candidate[(scenario, phase, cycle)]
        before_rss_settled = int(before["rss_settled_median_kib"])
        after_rss_settled = int(after["rss_settled_median_kib"])
        before_rss_peak = int(before["rss_peak_max_kib"])
        after_rss_peak = int(after["rss_peak_max_kib"])
        before_footprint_settled = int(before["footprint_settled_median_bytes"])
        after_footprint_settled = int(after["footprint_settled_median_bytes"])
        before_footprint_peak = int(before["footprint_peak_max_bytes"])
        after_footprint_peak = int(after["footprint_peak_max_bytes"])
        compared.append(
            {
                "scenario": scenario,
                "phase": phase,
                "cycle": cycle,
                "baseline_rss_settled_kib": before_rss_settled,
                "candidate_rss_settled_kib": after_rss_settled,
                "rss_settled_delta_kib": after_rss_settled - before_rss_settled,
                "rss_settled_delta_percent": percent(
                    after_rss_settled - before_rss_settled, before_rss_settled
                ),
                "baseline_rss_peak_kib": before_rss_peak,
                "candidate_rss_peak_kib": after_rss_peak,
                "rss_peak_delta_kib": after_rss_peak - before_rss_peak,
                "rss_peak_delta_percent": percent(
                    after_rss_peak - before_rss_peak, before_rss_peak
                ),
                "baseline_footprint_settled_bytes": before_footprint_settled,
                "candidate_footprint_settled_bytes": after_footprint_settled,
                "footprint_settled_delta_bytes": (
                    after_footprint_settled - before_footprint_settled
                ),
                "footprint_settled_delta_percent": percent(
                    after_footprint_settled - before_footprint_settled,
                    before_footprint_settled,
                ),
                "baseline_footprint_peak_bytes": before_footprint_peak,
                "candidate_footprint_peak_bytes": after_footprint_peak,
                "footprint_peak_delta_bytes": after_footprint_peak - before_footprint_peak,
                "footprint_peak_delta_percent": percent(
                    after_footprint_peak - before_footprint_peak,
                    before_footprint_peak,
                ),
            }
        )
    return compared


def cycle_slopes(rows: list[dict[str, str]]) -> dict[tuple[str, str], tuple[float, float]]:
    indexed = {
        (row["scenario"], int(row["run"]), row["phase"], int(row["cycle"])): row
        for row in rows
    }
    grouped: dict[tuple[str, str], list[tuple[int, int]]] = defaultdict(list)
    keys = {(key[0], key[1], key[2]) for key in indexed}
    for scenario, run, phase in keys:
        first = indexed.get((scenario, run, phase, 1))
        second = indexed.get((scenario, run, phase, 2))
        if not first or not second:
            continue
        grouped[(scenario, phase)].append(
            (
                int(second["rss_settled_median_kib"])
                - int(first["rss_settled_median_kib"]),
                int(second["footprint_settled_median_bytes"])
                - int(first["footprint_settled_median_bytes"]),
            )
        )
    return {
        key: (
            statistics.median(value[0] for value in values),
            statistics.median(value[1] for value in values),
        )
        for key, values in grouped.items()
    }


def compare_slopes(
    baseline_rows: list[dict[str, str]], candidate_rows: list[dict[str, str]]
) -> list[dict[str, object]]:
    baseline = cycle_slopes(baseline_rows)
    candidate = cycle_slopes(candidate_rows)
    if baseline.keys() != candidate.keys():
        raise SystemExit("incomparable cycle-slope phase matrix")
    return [
        {
            "scenario": scenario,
            "phase": phase,
            "baseline_rss_slope_kib": baseline[(scenario, phase)][0],
            "candidate_rss_slope_kib": candidate[(scenario, phase)][0],
            "rss_slope_delta_kib": (
                candidate[(scenario, phase)][0] - baseline[(scenario, phase)][0]
            ),
            "baseline_footprint_slope_bytes": baseline[(scenario, phase)][1],
            "candidate_footprint_slope_bytes": candidate[(scenario, phase)][1],
            "footprint_slope_delta_bytes": (
                candidate[(scenario, phase)][1] - baseline[(scenario, phase)][1]
            ),
        }
        for scenario, phase in sorted(baseline)
    ]


def write_tsv(path: Path, rows: list[dict[str, object]]) -> None:
    with path.open("w", newline="", encoding="utf-8") as handle:
        writer = csv.DictWriter(handle, fieldnames=list(rows[0]), delimiter="\t")
        writer.writeheader()
        writer.writerows(rows)


def mib_kib(value: float) -> float:
    return value / 1024.0


def mib_bytes(value: float) -> float:
    return value / (1024.0 * 1024.0)


def write_markdown(
    path: Path,
    baseline_metadata: dict[str, str],
    candidate_metadata: dict[str, str],
    comparisons: list[dict[str, object]],
    slopes: list[dict[str, object]],
) -> None:
    with path.open("w", encoding="utf-8") as handle:
        handle.write("# Usher memory A/B comparison\n\n")
        handle.write(
            f"Baseline: `{baseline_metadata.get('application_variant', 'usher')}` / "
            f"`{baseline_metadata['renderer']}` / `{baseline_metadata['slint_backend']}`. "
            f"Candidate: `{candidate_metadata.get('application_variant', 'usher')}` / "
            f"`{candidate_metadata['renderer']}` / "
            f"`{candidate_metadata['slint_backend']}`. Tokio workers: "
            f"`{baseline_metadata.get('tokio_worker_threads', 'default')}` -> "
            f"`{candidate_metadata.get('tokio_worker_threads', 'default')}`. "
            f"Workflow lifecycle: "
            f"`{baseline_metadata.get('workflow_lifecycle', 'normal')}` -> "
            f"`{candidate_metadata.get('workflow_lifecycle', 'normal')}`.\n\n"
        )
        handle.write(
            "All deltas are candidate minus baseline. Negative memory deltas mean "
            "the candidate used less memory.\n\n"
        )
        handle.write("## Settled median\n\n")
        handle.write(
            "| Scenario | Phase | Cycle | Baseline RSS | Candidate RSS | RSS delta | "
            "Baseline footprint | Candidate footprint | Footprint delta |\n"
        )
        handle.write("| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n")
        for row in comparisons:
            handle.write(
                f"| {row['scenario']} | {row['phase']} | {row['cycle']} | "
                f"{mib_kib(row['baseline_rss_settled_kib']):.2f} MiB | "
                f"{mib_kib(row['candidate_rss_settled_kib']):.2f} MiB | "
                f"{mib_kib(row['rss_settled_delta_kib']):+.2f} MiB "
                f"({row['rss_settled_delta_percent']:+.1f}%) | "
                f"{mib_bytes(row['baseline_footprint_settled_bytes']):.2f} MiB | "
                f"{mib_bytes(row['candidate_footprint_settled_bytes']):.2f} MiB | "
                f"{mib_bytes(row['footprint_settled_delta_bytes']):+.2f} MiB "
                f"({row['footprint_settled_delta_percent']:+.1f}%) |\n"
            )
        handle.write("\n## Maximum observed peak\n\n")
        handle.write(
            "| Scenario | Phase | Cycle | RSS delta | RSS delta % | "
            "Footprint delta | Footprint delta % |\n"
        )
        handle.write("| --- | --- | ---: | ---: | ---: | ---: | ---: |\n")
        for row in comparisons:
            handle.write(
                f"| {row['scenario']} | {row['phase']} | {row['cycle']} | "
                f"{mib_kib(row['rss_peak_delta_kib']):+.2f} MiB | "
                f"{row['rss_peak_delta_percent']:+.1f}% | "
                f"{mib_bytes(row['footprint_peak_delta_bytes']):+.2f} MiB | "
                f"{row['footprint_peak_delta_percent']:+.1f}% |\n"
            )
        handle.write("\n## Cycle 2 minus cycle 1 slope\n\n")
        handle.write(
            "| Scenario | Phase | Baseline RSS slope | Candidate RSS slope | "
            "Baseline footprint slope | Candidate footprint slope |\n"
        )
        handle.write("| --- | --- | ---: | ---: | ---: | ---: |\n")
        for row in slopes:
            handle.write(
                f"| {row['scenario']} | {row['phase']} | "
                f"{mib_kib(row['baseline_rss_slope_kib']):+.2f} MiB | "
                f"{mib_kib(row['candidate_rss_slope_kib']):+.2f} MiB | "
                f"{mib_bytes(row['baseline_footprint_slope_bytes']):+.2f} MiB | "
                f"{mib_bytes(row['candidate_footprint_slope_bytes']):+.2f} MiB |\n"
            )


def main() -> None:
    args = parse_args()
    baseline_metadata = read_metadata(args.baseline)
    candidate_metadata = read_metadata(args.candidate)
    validate_metadata(baseline_metadata, candidate_metadata)
    comparisons = compare_summary(
        read_rows(args.baseline / "summary.tsv"),
        read_rows(args.candidate / "summary.tsv"),
    )
    slopes = compare_slopes(
        read_rows(args.baseline / "run-summary.tsv"),
        read_rows(args.candidate / "run-summary.tsv"),
    )
    output_dir = args.output_dir or (
        args.candidate / f"ab-vs-{args.baseline.name}"
    )
    output_dir.mkdir(parents=True, exist_ok=True)
    write_tsv(output_dir / "comparison.tsv", comparisons)
    write_tsv(output_dir / "cycle-slopes.tsv", slopes)
    write_markdown(
        output_dir / "comparison.md",
        baseline_metadata,
        candidate_metadata,
        comparisons,
        slopes,
    )
    print(output_dir / "comparison.md")


if __name__ == "__main__":
    main()
