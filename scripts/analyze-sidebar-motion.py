#!/usr/bin/env python3
"""Summarize render-frame traces from the sidebar rapid-toggle A/B benchmark."""

from __future__ import annotations

import argparse
import csv
import math
import statistics
from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class Sample:
    elapsed_ms: float
    progress: float
    target_expanded: bool
    active: bool


@dataclass(frozen=True)
class RunMetrics:
    mode: str
    run: str
    target_changes: int
    active_frames: int
    interval_median_ms: float
    interval_p95_ms: float
    interval_max_ms: float
    repeated_active_frames_percent: float
    motion_update_median_ms: float
    motion_update_p95_ms: float
    progress_step_p95: float
    progress_step_max: float
    settle_after_last_click_ms: float


def percentile(values: list[float], percentage: float) -> float:
    ordered = sorted(values)
    if not ordered:
        return math.nan
    position = (len(ordered) - 1) * percentage
    lower = math.floor(position)
    upper = math.ceil(position)
    if lower == upper:
        return ordered[lower]
    weight = position - lower
    return ordered[lower] * (1.0 - weight) + ordered[upper] * weight


def load_trace(path: Path) -> tuple[str, list[Sample]]:
    samples: list[Sample] = []
    mode = "unknown"
    with path.open(newline="", encoding="utf-8") as trace_file:
        for row in csv.DictReader(trace_file, delimiter="\t"):
            mode = row["mode"]
            samples.append(
                Sample(
                    elapsed_ms=float(row["elapsed_us"]) / 1_000.0,
                    progress=float(row["progress"]),
                    target_expanded=row["target_expanded"] == "true",
                    active=row["active"] == "true",
                )
            )
    return mode, samples


def analyze(path: Path) -> RunMetrics:
    mode, samples = load_trace(path)
    target_change_indices = [
        index
        for index in range(1, len(samples))
        if samples[index].target_expanded != samples[index - 1].target_expanded
    ]
    active_indices = [index for index, sample in enumerate(samples) if sample.active]
    intervals: list[float] = []
    progress_steps: list[float] = []
    for previous_index, current_index in zip(active_indices, active_indices[1:]):
        if current_index != previous_index + 1:
            continue
        intervals.append(
            samples[current_index].elapsed_ms - samples[previous_index].elapsed_ms
        )
        progress_steps.append(
            abs(samples[current_index].progress - samples[previous_index].progress)
        )

    moving_sample_indices = [active_indices[0]]
    for previous_index, current_index in zip(active_indices, active_indices[1:]):
        if current_index != previous_index + 1:
            moving_sample_indices.append(current_index)
            continue
        if abs(samples[current_index].progress - samples[previous_index].progress) > 1.0e-6:
            moving_sample_indices.append(current_index)
    motion_update_intervals = [
        samples[current].elapsed_ms - samples[previous].elapsed_ms
        for previous, current in zip(moving_sample_indices, moving_sample_indices[1:])
        if current == previous + 1
        or all(samples[index].active for index in range(previous, current + 1))
    ]
    repeated_active_frames_percent = (
        sum(step <= 1.0e-6 for step in progress_steps) / len(progress_steps) * 100.0
    )

    settle_ms = math.nan
    if target_change_indices:
        last_change = target_change_indices[-1]
        for sample in samples[last_change:]:
            if not sample.active:
                settle_ms = sample.elapsed_ms - samples[last_change].elapsed_ms
                break

    if not intervals:
        raise ValueError(f"trace contains no consecutive active render frames: {path}")

    return RunMetrics(
        mode=mode,
        run=path.stem,
        target_changes=len(target_change_indices),
        active_frames=len(active_indices),
        interval_median_ms=statistics.median(intervals),
        interval_p95_ms=percentile(intervals, 0.95),
        interval_max_ms=max(intervals),
        repeated_active_frames_percent=repeated_active_frames_percent,
        motion_update_median_ms=statistics.median(motion_update_intervals),
        motion_update_p95_ms=percentile(motion_update_intervals, 0.95),
        progress_step_p95=percentile(progress_steps, 0.95),
        progress_step_max=max(progress_steps),
        settle_after_last_click_ms=settle_ms,
    )


def median_metric(runs: list[RunMetrics], field: str) -> float:
    values = [float(getattr(run, field)) for run in runs]
    return statistics.median(values)


def fmt(value: float, digits: int = 2) -> str:
    return "n/a" if math.isnan(value) else f"{value:.{digits}f}"


def delta(candidate: float, baseline: float, lower_is_better: bool) -> str:
    absolute = candidate - baseline
    percentage = absolute / baseline * 100.0 if baseline else math.nan
    improvement = -percentage if lower_is_better else percentage
    return f"{absolute:+.2f} ({percentage:+.1f}%; improvement {improvement:+.1f}%)"


def render_summary(runs: list[RunMetrics]) -> str:
    grouped: dict[str, list[RunMetrics]] = {}
    for run in runs:
        grouped.setdefault(run.mode, []).append(run)
    if set(grouped) != {"timer", "display-link"}:
        raise ValueError(f"expected timer and display-link traces, got {sorted(grouped)}")

    lines = [
        "# Sidebar rapid-toggle A/B",
        "",
        "Each cell uses the median of isolated release-process runs. The input burst is 24 clicks at 50 ms intervals.",
        "",
        "## Per run",
        "",
        "| Mode/run | Target changes | Active render frames | Repeated active frames | Motion update median (ms) | Motion update p95 (ms) | Progress step p95 | Max progress step | Final settle (ms) |",
        "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |",
    ]
    for run in sorted(runs, key=lambda item: (item.mode, item.run)):
        lines.append(
            f"| {run.mode}/{run.run} | {run.target_changes} | {run.active_frames} | "
            f"{fmt(run.repeated_active_frames_percent)}% | "
            f"{fmt(run.motion_update_median_ms)} | {fmt(run.motion_update_p95_ms)} | "
            f"{fmt(run.progress_step_p95, 4)} | "
            f"{fmt(run.progress_step_max, 4)} | {fmt(run.settle_after_last_click_ms)} |"
        )

    baseline = grouped["timer"]
    candidate = grouped["display-link"]
    metric_rows = [
        ("Active render frames", "active_frames", False, 0),
        ("Frame interval median (ms)", "interval_median_ms", True, 2),
        ("Frame interval p95 (ms)", "interval_p95_ms", True, 2),
        ("Max interval (ms)", "interval_max_ms", True, 2),
        ("Repeated active frames (%)", "repeated_active_frames_percent", True, 2),
        ("Motion update median (ms)", "motion_update_median_ms", True, 2),
        ("Motion update p95 (ms)", "motion_update_p95_ms", True, 2),
        ("Progress step p95", "progress_step_p95", True, 4),
        ("Max progress step", "progress_step_max", True, 4),
        ("Final settle (ms)", "settle_after_last_click_ms", True, 2),
    ]
    lines.extend(
        [
            "",
            "## Median comparison",
            "",
            "| Metric | Fixed 16 ms timer | Display-linked closed-form spring | Candidate - baseline |",
            "| --- | ---: | ---: | ---: |",
        ]
    )
    for label, field, lower_is_better, digits in metric_rows:
        baseline_value = median_metric(baseline, field)
        candidate_value = median_metric(candidate, field)
        lines.append(
            f"| {label} | {fmt(baseline_value, digits)} | {fmt(candidate_value, digits)} | "
            f"{delta(candidate_value, baseline_value, lower_is_better)} |"
        )
    lines.append("")
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    traces = sorted(args.directory.glob("*-run-*.tsv"))
    if len(traces) < 6:
        raise SystemExit(f"expected at least six traces, found {len(traces)}")
    summary = render_summary([analyze(path) for path in traces])
    summary_path = args.directory / "summary.md"
    summary_path.write_text(summary, encoding="utf-8")
    print(summary)


if __name__ == "__main__":
    main()
