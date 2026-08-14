#!/usr/bin/env python3
"""Aggregate Artifact Preview wheel-scroll A/B frame traces."""

from __future__ import annotations

import argparse
import csv
import math
import statistics
from dataclasses import dataclass
from pathlib import Path


def percentile(values: list[float], fraction: float) -> float:
    ordered = sorted(values)
    position = (len(ordered) - 1) * fraction
    lower = math.floor(position)
    upper = math.ceil(position)
    if lower == upper:
        return ordered[lower]
    weight = position - lower
    return ordered[lower] * (1.0 - weight) + ordered[upper] * weight


@dataclass(frozen=True)
class Metrics:
    frames: int
    gap_p50_ms: float
    gap_p95_ms: float
    gap_max_ms: float
    render_p50_ms: float
    render_p95_ms: float
    render_max_ms: float
    over_16_67_percent: float
    over_33_33_percent: float
    mounted_max: int
    projection_commits: int


@dataclass(frozen=True)
class Run:
    variant: str
    name: str
    metrics: Metrics
    resize_metrics: Metrics


def metrics_for(rows: list[dict[str, str]], path: Path, phase: str) -> Metrics:
    if phase == "resize":
        episode_start = 0
        for index in range(1, len(rows)):
            if int(rows[index]["elapsed_us"]) - int(rows[index - 1]["elapsed_us"]) > 1_000_000:
                episode_start = index
        rows = rows[episode_start:]
    if not rows:
        raise ValueError(f"no docked {phase} samples in {path}")
    gaps = [int(row["frame_gap_us"]) / 1000.0 for row in rows if int(row["frame_gap_us"]) > 0]
    renders = [int(row["render_us"]) / 1000.0 for row in rows if int(row["render_us"]) > 0]
    mounted = [int(row["mounted_rows"]) for row in rows]
    commits = [int(row["projection_commits"]) for row in rows]
    if len(gaps) < 5 or len(renders) < 5:
        raise ValueError(
            f"insufficient {phase} frames in {path}: gaps={len(gaps)} renders={len(renders)}"
        )
    return Metrics(
        frames=len(gaps),
        gap_p50_ms=statistics.median(gaps),
        gap_p95_ms=percentile(gaps, 0.95),
        gap_max_ms=max(gaps),
        render_p50_ms=statistics.median(renders),
        render_p95_ms=percentile(renders, 0.95),
        render_max_ms=max(renders),
        over_16_67_percent=sum(value > 16.67 for value in gaps) / len(gaps) * 100.0,
        over_33_33_percent=sum(value > 33.33 for value in gaps) / len(gaps) * 100.0,
        mounted_max=max(mounted),
        projection_commits=max(commits) - min(commits),
    )


def analyze(path: Path) -> Run:
    with path.open(newline="", encoding="utf-8") as trace_file:
        all_rows = list(csv.DictReader(trace_file, delimiter="\t"))
    rows = [row for row in all_rows if row["host"] == "docked-divider"]
    variants = {row["variant"] for row in rows}
    if len(variants) != 1:
        raise ValueError(f"mixed variants in {path}: {sorted(variants)}")
    return Run(
        variant=variants.pop(),
        name=path.stem,
        metrics=metrics_for([row for row in rows if row["phase"] == "scroll"], path, "scroll"),
        resize_metrics=metrics_for(
            [row for row in rows if row["phase"] == "resize"], path, "resize"
        ),
    )


def fmt(value: float) -> str:
    return f"{value:.2f}"


def change(candidate: float, baseline: float) -> str:
    if baseline == 0:
        return f"{candidate - baseline:+.2f}"
    return f"{candidate - baseline:+.2f} ({(candidate - baseline) / baseline * 100.0:+.1f}%)"


def median(runs: list[Run], field: str) -> float:
    return statistics.median(float(getattr(run.metrics, field)) for run in runs)


def resize_median(runs: list[Run], field: str) -> float:
    return statistics.median(float(getattr(run.resize_metrics, field)) for run in runs)


def render(runs: list[Run]) -> str:
    grouped = {
        variant: [run for run in runs if run.variant == variant]
        for variant in ("baseline", "optimized")
    }
    if any(len(grouped[variant]) < 3 for variant in grouped):
        raise ValueError("expected at least three isolated runs per variant")
    lines = [
        "# Artifact Preview wheel-scroll A/B",
        "",
        "Same-binary Release measurements for the docked Preview with the fixed 256 KiB, 371-block Markdown fixture.",
        "",
        "| Variant/run | Frames | Gap p50/p95/max (ms) | Render p50/p95/max (ms) | >16.67 ms | >33.33 ms | Mounted max | Projection commits |",
        "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |",
    ]
    for run in sorted(runs, key=lambda item: (item.variant, item.name)):
        metric = run.metrics
        lines.append(
            f"| {run.variant}/{run.name} | {metric.frames} | "
            f"{fmt(metric.gap_p50_ms)}/{fmt(metric.gap_p95_ms)}/{fmt(metric.gap_max_ms)} | "
            f"{fmt(metric.render_p50_ms)}/{fmt(metric.render_p95_ms)}/{fmt(metric.render_max_ms)} | "
            f"{fmt(metric.over_16_67_percent)}% | {fmt(metric.over_33_33_percent)}% | "
            f"{metric.mounted_max} | {metric.projection_commits} |"
        )
    fields = (
        ("Frame gap p50 (ms)", "gap_p50_ms"),
        ("Frame gap p95 (ms)", "gap_p95_ms"),
        ("Frame gap max (ms)", "gap_max_ms"),
        ("Render p50 (ms)", "render_p50_ms"),
        ("Render p95 (ms)", "render_p95_ms"),
        ("Render max (ms)", "render_max_ms"),
        (">16.67 ms frames (%)", "over_16_67_percent"),
        (">33.33 ms frames (%)", "over_33_33_percent"),
        ("Mounted rows max", "mounted_max"),
        ("Projection commits", "projection_commits"),
    )
    lines.extend([
        "",
        "## Median comparison",
        "",
        "| Metric | Baseline | Optimized | Change |",
        "| --- | ---: | ---: | ---: |",
    ])
    for label, field in fields:
        baseline = median(grouped["baseline"], field)
        optimized = median(grouped["optimized"], field)
        lines.append(
            f"| {label} | {fmt(baseline)} | {fmt(optimized)} | {change(optimized, baseline)} |"
        )
    lines.extend([
        "",
        "## Same optimized resize path",
        "",
        "Both variants use the optimized docked Preview and differ only in the scroll projection policy.",
        "",
        "| Metric | Per-event model | Incremental model | Change |",
        "| --- | ---: | ---: | ---: |",
    ])
    for label, field in fields[:-2]:
        baseline = resize_median(grouped["baseline"], field)
        optimized = resize_median(grouped["optimized"], field)
        lines.append(
            f"| {label} | {fmt(baseline)} | {fmt(optimized)} | {change(optimized, baseline)} |"
        )
    lines.append("")
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    traces = sorted((args.directory / "runs").glob("*-run-*.tsv"))
    if len(traces) < 6:
        raise SystemExit(f"expected at least six traces, found {len(traces)}")
    try:
        summary = render([analyze(path) for path in traces])
    except ValueError as error:
        raise SystemExit(str(error)) from error
    (args.directory / "summary.md").write_text(summary, encoding="utf-8")
    print(summary)


if __name__ == "__main__":
    main()
