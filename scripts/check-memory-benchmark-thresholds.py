#!/usr/bin/env python3
"""Fail when one Usher memory benchmark exceeds its absolute or growth budgets."""

from __future__ import annotations

import argparse
import csv
import statistics
from collections import defaultdict
from pathlib import Path


MIB = 1024.0 * 1024.0


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("result_dir", type=Path)
    parser.add_argument("--minimum-runs", type=int, default=3)
    parser.add_argument("--max-rss-peak-mib", type=float, default=256.0)
    parser.add_argument("--max-footprint-peak-mib", type=float, default=256.0)
    parser.add_argument("--max-rss-cycle-growth-mib", type=float, default=16.0)
    parser.add_argument("--max-footprint-cycle-growth-mib", type=float, default=16.0)
    return parser.parse_args()


def read_tsv(path: Path) -> list[dict[str, str]]:
    if not path.is_file():
        raise SystemExit(f"missing benchmark result: {path}")
    with path.open(newline="", encoding="utf-8") as handle:
        return list(csv.DictReader(handle, delimiter="\t"))


def read_metadata(path: Path) -> dict[str, str]:
    return {row["key"]: row["value"] for row in read_tsv(path)}


def median_cycle_growth(
    rows: list[dict[str, str]], field: str
) -> dict[tuple[str, str], float]:
    indexed = {
        (row["scenario"], int(row["run"]), row["phase"], int(row["cycle"])): row
        for row in rows
    }
    samples: dict[tuple[str, str], list[int]] = defaultdict(list)
    for scenario, run, phase, cycle in indexed:
        if cycle != 1:
            continue
        first = indexed[(scenario, run, phase, 1)]
        second = indexed.get((scenario, run, phase, 2))
        if second is not None:
            samples[(scenario, phase)].append(int(second[field]) - int(first[field]))
    return {key: float(statistics.median(values)) for key, values in samples.items()}


def main() -> None:
    args = parse_args()
    if args.minimum_runs < 1:
        raise SystemExit("--minimum-runs must be positive")
    limits = (
        args.max_rss_peak_mib,
        args.max_footprint_peak_mib,
        args.max_rss_cycle_growth_mib,
        args.max_footprint_cycle_growth_mib,
    )
    if any(value < 0 for value in limits):
        raise SystemExit("memory thresholds must be non-negative")

    metadata = read_metadata(args.result_dir / "metadata.tsv")
    summary = read_tsv(args.result_dir / "summary.tsv")
    per_run = read_tsv(args.result_dir / "run-summary.tsv")
    if not summary or not per_run:
        raise SystemExit("benchmark summary is empty")

    errors: list[str] = []
    try:
        recorded_runs = int(metadata.get("runs", "0"))
    except ValueError:
        recorded_runs = 0
    if recorded_runs < args.minimum_runs:
        errors.append(
            f"isolated runs {recorded_runs} are below required {args.minimum_runs}"
        )
    if metadata.get("build_profile") != "release":
        errors.append("benchmark did not use the release profile")

    expected_route = metadata.get("expected_route", "")
    observed_routes = set(filter(None, metadata.get("observed_routes", "").split(",")))
    if expected_route and expected_route not in observed_routes:
        errors.append(
            f"expected route {expected_route!r} was not observed: {sorted(observed_routes)}"
        )

    rss_peak_mib = max(int(row["rss_peak_max_kib"]) for row in summary) / 1024.0
    footprint_peak_mib = (
        max(int(row["footprint_peak_max_bytes"]) for row in summary) / MIB
    )
    rss_growth = median_cycle_growth(per_run, "rss_settled_median_kib")
    footprint_growth = median_cycle_growth(per_run, "footprint_settled_median_bytes")
    if not rss_growth or rss_growth.keys() != footprint_growth.keys():
        errors.append("benchmark does not contain matching cycle 1/cycle 2 growth samples")
    rss_growth_mib = max(rss_growth.values(), default=0.0) / 1024.0
    footprint_growth_mib = max(footprint_growth.values(), default=0.0) / MIB

    checks = [
        ("RSS peak", rss_peak_mib, args.max_rss_peak_mib),
        ("Footprint peak", footprint_peak_mib, args.max_footprint_peak_mib),
        ("RSS cycle growth", rss_growth_mib, args.max_rss_cycle_growth_mib),
        (
            "Footprint cycle growth",
            footprint_growth_mib,
            args.max_footprint_cycle_growth_mib,
        ),
    ]
    for label, actual, limit in checks:
        if actual > limit:
            errors.append(f"{label} {actual:.2f} MiB exceeds {limit:.2f} MiB")

    report = [
        "# Memory benchmark thresholds",
        "",
        f"Result: **{'FAIL' if errors else 'PASS'}**",
        "",
        "| Check | Observed | Limit | Status |",
        "| --- | ---: | ---: | --- |",
    ]
    for label, actual, limit in checks:
        report.append(
            f"| {label} | {actual:.2f} MiB | {limit:.2f} MiB | "
            f"{'PASS' if actual <= limit else 'FAIL'} |"
        )
    if errors:
        report.extend(["", "## Failures", ""])
        report.extend(f"- {error}" for error in errors)
    report.append("")
    report_text = "\n".join(report)
    (args.result_dir / "thresholds.md").write_text(report_text, encoding="utf-8")
    print(report_text)
    if errors:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
