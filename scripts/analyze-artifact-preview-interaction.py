#!/usr/bin/env python3
from __future__ import annotations

import argparse
import csv
import math
import statistics
from dataclasses import dataclass
from pathlib import Path


HOSTS = ("docked-divider", "detached-window")


def percentile(values: list[float], fraction: float) -> float:
    ordered = sorted(values)
    position = (len(ordered) - 1) * fraction
    lower = math.floor(position)
    upper = math.ceil(position)
    if lower == upper:
        return ordered[lower]
    return ordered[lower] * (upper - position) + ordered[upper] * (position - lower)


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
    block_min: int
    block_max: int


@dataclass(frozen=True)
class Run:
    variant: str
    host: str
    name: str
    metrics: Metrics


def analyze(path: Path) -> Run:
    with path.open(newline="", encoding="utf-8") as trace_file:
        rows = list(csv.DictReader(trace_file, delimiter="\t"))
    if not rows:
        raise ValueError(f"empty trace: {path}")
    variants = {row["variant"] for row in rows}
    hosts = {row["host"] for row in rows}
    if len(variants) != 1 or len(hosts) != 1:
        raise ValueError(f"mixed variant/host in {path}: {variants=} {hosts=}")
    # Window creation can emit an earlier resize episode. Split on a real idle
    # second instead of the runtime's 300 ms trace tail: the unoptimized
    # document tree can itself produce >300 ms frame intervals during one
    # continuous gesture.
    episode_start = 0
    for index in range(1, len(rows)):
        previous = int(rows[index - 1]["elapsed_us"])
        current = int(rows[index]["elapsed_us"])
        if current - previous > 1_000_000:
            episode_start = index
    rows = rows[episode_start:]
    gaps = [
        (int(current["elapsed_us"]) - int(previous["elapsed_us"])) / 1000.0
        for previous, current in zip(rows, rows[1:])
    ]
    renders = [float(row["render_us"]) / 1000.0 for row in rows if int(row["render_us"]) > 0]
    blocks = [int(row["block_count"]) for row in rows]
    if len(gaps) < 5 or len(renders) < 5:
        raise ValueError(f"insufficient frames in {path}: gaps={len(gaps)} renders={len(renders)}")
    return Run(
        variant=variants.pop(),
        host=hosts.pop(),
        name=path.stem,
        metrics=Metrics(
            frames=len(gaps),
            gap_p50_ms=statistics.median(gaps),
            gap_p95_ms=percentile(gaps, 0.95),
            gap_max_ms=max(gaps),
            render_p50_ms=statistics.median(renders),
            render_p95_ms=percentile(renders, 0.95),
            render_max_ms=max(renders),
            over_16_67_percent=sum(value > 16.67 for value in gaps) / len(gaps) * 100.0,
            over_33_33_percent=sum(value > 33.33 for value in gaps) / len(gaps) * 100.0,
            block_min=min(blocks),
            block_max=max(blocks),
        ),
    )


def fmt(value: float) -> str:
    return f"{value:.2f}"


def comparison(candidate: float, baseline: float) -> str:
    if baseline == 0:
        return f"{candidate - baseline:+.2f}"
    change = (candidate - baseline) / baseline * 100.0
    return f"{candidate - baseline:+.2f} ({change:+.1f}%)"


def median_metric(runs: list[Run], field: str) -> float:
    return statistics.median(float(getattr(run.metrics, field)) for run in runs)


def render(runs: list[Run]) -> str:
    lines = [
        "# Artifact Preview resize A/B",
        "",
        "Each result is measured from Slint render callbacks in an isolated Release process. Frame gap is the interval between consecutive rendered frames during the resize gesture and its 300 ms settle tail.",
        "",
        "## Per run",
        "",
        "| Variant/run | Host | Frames | Gap p50/p95/max (ms) | Render p50/p95/max (ms) | >16.67 ms | >33.33 ms | Blocks min/max |",
        "| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |",
    ]
    for run in sorted(runs, key=lambda item: (item.host, item.variant, item.name)):
        metric = run.metrics
        lines.append(
            f"| {run.variant}/{run.name} | {run.host} | {metric.frames} | "
            f"{fmt(metric.gap_p50_ms)}/{fmt(metric.gap_p95_ms)}/{fmt(metric.gap_max_ms)} | "
            f"{fmt(metric.render_p50_ms)}/{fmt(metric.render_p95_ms)}/{fmt(metric.render_max_ms)} | "
            f"{fmt(metric.over_16_67_percent)}% | {fmt(metric.over_33_33_percent)}% | "
            f"{metric.block_min}/{metric.block_max} |"
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
    )
    lines.extend([
        "",
        "## Median comparison",
        "",
        "Lower is better. Each value is the median of isolated process runs; delta is optimized minus baseline.",
        "",
        "| Host | Metric | Baseline | Optimized | Delta |",
        "| --- | --- | ---: | ---: | ---: |",
    ])
    for host in HOSTS:
        baseline = [run for run in runs if run.host == host and run.variant == "baseline"]
        optimized = [run for run in runs if run.host == host and run.variant == "optimized"]
        if len(baseline) < 3 or len(optimized) < 3:
            raise ValueError(f"expected at least three runs per variant for {host}")
        for label, field in fields:
            baseline_value = median_metric(baseline, field)
            optimized_value = median_metric(optimized, field)
            lines.append(
                f"| {host} | {label} | {fmt(baseline_value)} | {fmt(optimized_value)} | "
                f"{comparison(optimized_value, baseline_value)} |"
            )
    lines.append("")
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    traces = sorted((args.directory / "runs").glob("*.tsv"))
    if len(traces) < 12:
        raise SystemExit(f"expected at least twelve traces, found {len(traces)}")
    try:
        summary = render([analyze(path) for path in traces])
    except ValueError as error:
        raise SystemExit(str(error)) from error
    (args.directory / "summary.md").write_text(summary, encoding="utf-8")
    print(summary)


if __name__ == "__main__":
    main()
