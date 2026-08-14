#!/usr/bin/env python3
from __future__ import annotations

import csv
import math
import statistics
import sys
from pathlib import Path


def percentile(values: list[float], fraction: float) -> float:
    ordered = sorted(values)
    position = (len(ordered) - 1) * fraction
    lower = math.floor(position)
    upper = math.ceil(position)
    if lower == upper:
        return ordered[lower]
    return ordered[lower] * (upper - position) + ordered[upper] * (position - lower)


def samples(path: Path) -> dict[str, list[float] | float]:
    with path.open(newline="", encoding="utf-8") as source:
        rows = list(csv.DictReader(source, delimiter="\t"))
    rows = [row for row in rows if row["host"] == "docked-divider"]
    gaps = [
        int(row["frame_gap_us"]) / 1000.0
        for row in rows
        if 0 < int(row["frame_gap_us"]) < 600_000
    ]
    renders = [int(row["render_us"]) / 1000.0 for row in rows if int(row["render_us"]) > 0]
    blocks = [int(row["block_count"]) for row in rows]
    if not rows or not renders:
        raise ValueError(f"insufficient transition frames in {path}")
    return {
        "gaps": gaps,
        "renders": renders,
        "frame_count": float(len(rows)),
        "block_min": min(blocks),
        "block_max": max(blocks),
    }


def summarize(runs: list[dict[str, list[float] | float]]) -> dict[str, float]:
    gaps = [value for run in runs for value in run["gaps"]]
    renders = [value for run in runs for value in run["renders"]]
    if not gaps:
        raise ValueError("no measurable frame gaps across transition runs")
    return {
        "frames_per_run": statistics.median(float(run["frame_count"]) for run in runs),
        "gap_p50": statistics.median(gaps),
        "gap_p95": percentile(gaps, 0.95),
        "gap_max": max(gaps),
        "render_p50": statistics.median(renders),
        "render_p95": percentile(renders, 0.95),
        "render_max": max(renders),
        "over_33": sum(value > 33.33 for value in gaps) / len(gaps) * 100.0,
    }


def change(candidate: float, baseline: float) -> str:
    if baseline == 0:
        return f"{candidate - baseline:+.2f}"
    return f"{(candidate - baseline) / baseline * 100:+.1f}%"


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("usage: analyze-artifact-preview-transition.py RESULT_DIR")
    root = Path(sys.argv[1])
    grouped: dict[str, list[dict[str, list[float] | float]]] = {
        "baseline": [],
        "optimized": [],
    }
    for path in sorted((root / "runs").glob("*.tsv")):
        variant = path.name.split("-", 1)[0]
        if variant in grouped:
            grouped[variant].append(samples(path))
    if any(len(runs) < 3 for runs in grouped.values()):
        raise SystemExit("expected at least three isolated runs per variant")

    fields = (
        ("Recorded frames / run (median)", "frames_per_run"),
        ("Frame gap p50 (ms)", "gap_p50"),
        ("Frame gap p95 (ms)", "gap_p95"),
        ("Frame gap max (ms)", "gap_max"),
        ("Render p50 (ms)", "render_p50"),
        ("Render p95 (ms)", "render_p95"),
        ("Render max (ms)", "render_max"),
        (">33.33 ms frames (%)", "over_33"),
    )
    summaries = {variant: summarize(runs) for variant, runs in grouped.items()}
    lines = [
        "# Artifact Preview docked transition A/B",
        "",
        "Same-binary Release measurements for one close and one open transition with a 256 KiB, 371-block Markdown fixture.",
        "",
        "| Metric | Baseline | Optimized | Change |",
        "| --- | ---: | ---: | ---: |",
    ]
    for label, key in fields:
        baseline = summaries["baseline"][key]
        optimized = summaries["optimized"][key]
        lines.append(
            f"| {label} | {baseline:.2f} | {optimized:.2f} | {change(optimized, baseline)} |"
        )
    for variant, runs in grouped.items():
        if any(run["block_min"] != 371 or run["block_max"] != 371 for run in runs):
            raise SystemExit(f"{variant} transition replaced the 371-block model")
    lines.extend(["", "All traces retained exactly 371 blocks during both transitions.", ""])
    summary = "\n".join(lines)
    (root / "summary.md").write_text(summary, encoding="utf-8")
    print(summary)


if __name__ == "__main__":
    main()
