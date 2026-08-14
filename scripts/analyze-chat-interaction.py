#!/usr/bin/env python3
"""Aggregate the reproducible Chat resize/scroll A/B frame traces."""

from __future__ import annotations

import argparse
import csv
import math
import statistics
from dataclasses import dataclass
from pathlib import Path


PHASES = ("resize", "scroll")


def percentile(values: list[float], fraction: float) -> float:
    ordered = sorted(values)
    if not ordered:
        return math.nan
    position = (len(ordered) - 1) * fraction
    lower = math.floor(position)
    upper = math.ceil(position)
    if lower == upper:
        return ordered[lower]
    weight = position - lower
    return ordered[lower] * (1.0 - weight) + ordered[upper] * weight


@dataclass(frozen=True)
class PhaseMetrics:
    frames: int
    gap_p50_ms: float
    gap_p95_ms: float
    gap_max_ms: float
    render_p50_ms: float
    render_p95_ms: float
    render_max_ms: float
    over_16_67_percent: float
    over_33_33_percent: float
    mounted_median: float
    mounted_max: int
    model_commits: int
    virtual_blank_max_px: float
    commit_gap_p50_ms: float
    commit_gap_p95_ms: float
    commit_gap_max_ms: float
    mutation_p95_ms: float
    mutation_max_ms: float
    mutation_to_render_p95_ms: float
    mutation_to_render_max_ms: float
    inserted_rows_max: int
    removed_rows_max: int
    incremental_commit_percent: float


@dataclass(frozen=True)
class RunMetrics:
    variant: str
    run: str
    phases: dict[str, PhaseMetrics]
    startup_bottom_distance_max_px: float
    post_stop_commits_800ms: int
    drag_tracking_drift_max_px: float
    drag_reverse_jump_max_px: float
    drag_anchor_writes: int
    post_release_jump_px: float
    wheel_direction_samples: int
    wheel_reverse_jump_max_px: float


def analyze_trace(path: Path) -> RunMetrics:
    rows: list[dict[str, str]] = []
    with path.open(newline="", encoding="utf-8") as trace_file:
        rows.extend(csv.DictReader(trace_file, delimiter="\t"))
    if not rows:
        raise ValueError(f"empty trace: {path}")
    variants = {row["variant"] for row in rows}
    if len(variants) != 1:
        raise ValueError(f"mixed variants in {path}: {sorted(variants)}")

    previous_commits: int | None = None
    for row in rows:
        commits_now = int(row["model_commits"])
        row["_commit_changed"] = str(
            previous_commits is not None and commits_now > previous_commits
        )
        previous_commits = commits_now

    phases: dict[str, PhaseMetrics] = {}
    first_resize_index = next(
        (index for index, row in enumerate(rows) if row["phase"] == "resize"),
        len(rows),
    )
    last_resize_index = max(
        (index for index, row in enumerate(rows) if row["phase"] == "resize"),
        default=-1,
    )
    for phase in PHASES:
        selected = [
            row
            for index, row in enumerate(rows)
            if row["phase"] == phase and (phase != "scroll" or index > last_resize_index)
        ]
        gaps = [float(row["frame_gap_us"]) / 1000.0 for row in selected if int(row["frame_gap_us"]) > 0]
        renders = [float(row["render_us"]) / 1000.0 for row in selected if int(row["render_us"]) > 0]
        mounted = [int(row["mounted_rows"]) for row in selected]
        commits = [int(row["model_commits"]) for row in selected]
        commit_rows = [row for row in selected if row["_commit_changed"] == "True"]
        commit_gaps = [
            float(row["frame_gap_us"]) / 1000.0
            for row in commit_rows
            if int(row["frame_gap_us"]) > 0
        ]
        mutation_durations = [float(row["model_mutation_us"]) / 1000.0 for row in commit_rows]
        mutation_to_render = [float(row["mutation_to_render_us"]) / 1000.0 for row in commit_rows]
        inserted_rows = [int(row["inserted_rows"]) for row in commit_rows]
        removed_rows = [int(row["removed_rows"]) for row in commit_rows]
        incremental_commits = [int(row["incremental_mutation"]) for row in commit_rows]
        virtual_blanks = []
        previous_commits: int | None = None
        for row in selected:
            commits_now = int(row["model_commits"])
            if previous_commits == commits_now:
                virtual_blanks.append(float(row["virtual_blank_px"]))
            previous_commits = commits_now
        if len(gaps) < 5 or len(renders) < 5:
            raise ValueError(f"insufficient {phase} frames in {path}: gaps={len(gaps)} renders={len(renders)}")
        phases[phase] = PhaseMetrics(
            frames=len(gaps),
            gap_p50_ms=statistics.median(gaps),
            gap_p95_ms=percentile(gaps, 0.95),
            gap_max_ms=max(gaps),
            render_p50_ms=statistics.median(renders),
            render_p95_ms=percentile(renders, 0.95),
            render_max_ms=max(renders),
            over_16_67_percent=sum(value > 16.67 for value in gaps) / len(gaps) * 100.0,
            over_33_33_percent=sum(value > 33.33 for value in gaps) / len(gaps) * 100.0,
            mounted_median=statistics.median(mounted),
            mounted_max=max(mounted),
            model_commits=max(commits) - min(commits),
            virtual_blank_max_px=max(virtual_blanks, default=0.0),
            commit_gap_p50_ms=statistics.median(commit_gaps) if commit_gaps else math.nan,
            commit_gap_p95_ms=percentile(commit_gaps, 0.95),
            commit_gap_max_ms=max(commit_gaps, default=math.nan),
            mutation_p95_ms=percentile(mutation_durations, 0.95),
            mutation_max_ms=max(mutation_durations, default=math.nan),
            mutation_to_render_p95_ms=percentile(mutation_to_render, 0.95),
            mutation_to_render_max_ms=max(mutation_to_render, default=math.nan),
            inserted_rows_max=max(inserted_rows, default=0),
            removed_rows_max=max(removed_rows, default=0),
            incremental_commit_percent=(
                sum(incremental_commits) / len(incremental_commits) * 100.0
                if incremental_commits
                else math.nan
            ),
        )
    startup_rows = rows[:first_resize_index]
    initial_position_index = next(
        (
            index
            for index in range(1, len(startup_rows))
            if abs(
                float(startup_rows[index]["scroll_y"])
                - float(startup_rows[index - 1]["scroll_y"])
            )
            > 1.0
        ),
        len(startup_rows),
    )
    startup_bottom_distance_max_px = max(
        (
            float(startup_rows[index]["bottom_distance_px"])
            for index in range(initial_position_index, len(startup_rows))
        ),
        default=0.0,
    )

    last_motion = 0
    for index in range(1, len(rows)):
        if abs(float(rows[index]["scroll_y"]) - float(rows[index - 1]["scroll_y"])) > 0.25:
            last_motion = index
    stopped_at_us = int(rows[last_motion]["elapsed_us"])
    commits_at_stop = int(rows[last_motion]["model_commits"])
    commits_after_stop = max(
        int(row["model_commits"])
        for row in rows[last_motion:]
        if int(row["elapsed_us"]) <= stopped_at_us + 800_000
    )
    drag_indexes = [
        index for index, row in enumerate(rows) if int(row["scrollbar_dragging"]) == 1
    ]
    if len(drag_indexes) < 3:
        raise ValueError(f"insufficient scrollbar-drag frames in {path}: {len(drag_indexes)}")
    drag_rows = [rows[index] for index in drag_indexes]
    pointer_offsets = [
        float(row["scrollbar_pointer_y"]) - float(row["scrollbar_thumb_y"])
        for row in drag_rows
    ]
    drag_tracking_drift_max_px = max(pointer_offsets) - min(pointer_offsets)
    drag_reverse_jump_max_px = max(
        (
            float(drag_rows[index]["scrollbar_thumb_y"])
            - float(drag_rows[index - 1]["scrollbar_thumb_y"])
            for index in range(1, len(drag_rows))
        ),
        default=0.0,
    )
    drag_anchor_writes = int(drag_rows[-1]["anchor_scroll_writes"]) - int(
        drag_rows[0]["anchor_scroll_writes"]
    )
    last_drag_index = drag_indexes[-1]
    release_row_index = next(
        (
            index
            for index in range(last_drag_index + 1, len(rows))
            if int(rows[index]["scrollbar_release_revision"])
            > int(rows[last_drag_index]["scrollbar_release_revision"])
        ),
        last_drag_index,
    )
    release_position = (
        float(rows[release_row_index]["scrollbar_release_position"])
        if release_row_index != last_drag_index
        else float(rows[last_drag_index]["scroll_y"])
    )
    release_wheel_revision = int(rows[release_row_index]["wheel_input_revision"])
    post_release_rows = []
    for row in rows[release_row_index:]:
        if int(row["wheel_input_revision"]) != release_wheel_revision:
            break
        post_release_rows.append(row)
    post_release_jump_px = max(
        (abs(float(row["scroll_y"]) - release_position) for row in post_release_rows),
        default=0.0,
    )
    wheel_reverse_jumps: list[float] = []
    previous_row = rows[release_row_index]
    for row in rows[release_row_index + 1 :]:
        revision = int(row["wheel_input_revision"])
        previous_revision = int(previous_row["wheel_input_revision"])
        if revision == release_wheel_revision:
            previous_row = row
            continue
        movement = float(row["scroll_y"]) - float(previous_row["scroll_y"])
        direction = float(row["wheel_input_delta"])
        if direction > 0.0 and movement < 0.0:
            wheel_reverse_jumps.append(-movement)
        elif direction < 0.0 and movement > 0.0:
            wheel_reverse_jumps.append(movement)
        previous_row = row
    wheel_reverse_jump_max_px = max(wheel_reverse_jumps, default=0.0)
    wheel_direction_samples = max(
        int(row["wheel_input_revision"]) for row in rows
    ) - release_wheel_revision
    return RunMetrics(
        variant=variants.pop(),
        run=path.stem,
        phases=phases,
        startup_bottom_distance_max_px=startup_bottom_distance_max_px,
        post_stop_commits_800ms=commits_after_stop - commits_at_stop,
        drag_tracking_drift_max_px=drag_tracking_drift_max_px,
        drag_reverse_jump_max_px=max(0.0, drag_reverse_jump_max_px),
        drag_anchor_writes=drag_anchor_writes,
        post_release_jump_px=post_release_jump_px,
        wheel_direction_samples=wheel_direction_samples,
        wheel_reverse_jump_max_px=wheel_reverse_jump_max_px,
    )


def median_metric(runs: list[RunMetrics], phase: str, field: str) -> float:
    return statistics.median(float(getattr(run.phases[phase], field)) for run in runs)


def fmt(value: float) -> str:
    return "n/a" if math.isnan(value) else f"{value:.2f}"


def comparison(candidate: float, baseline: float) -> str:
    if baseline == 0:
        return f"{candidate - baseline:+.2f}"
    change = (candidate - baseline) / baseline * 100.0
    return f"{candidate - baseline:+.2f} ({change:+.1f}%)"


def render_summary(runs: list[RunMetrics]) -> str:
    grouped: dict[str, list[RunMetrics]] = {}
    for run in runs:
        grouped.setdefault(run.variant, []).append(run)
    if set(grouped) != {"baseline", "optimized"}:
        raise ValueError(f"expected baseline and optimized traces, got {sorted(grouped)}")
    if len(grouped["baseline"]) != len(grouped["optimized"]):
        raise ValueError("baseline and optimized run counts differ")

    lines = [
        "# Chat interaction A/B",
        "",
        "Each comparison cell is the median of isolated release-process runs. Frame gap is the interval between consecutive rendered frames; render time is measured from Slint BeforeRendering to AfterRendering.",
        "",
        "## Per run",
        "",
        "| Variant/run | Phase | Frames | Gap p50/p95/max (ms) | Render p50/p95/max (ms) | >16.67 ms | >33.33 ms | Mounted median/max | Blank max (px) | Model commits | Post-stop commits (800 ms) |",
        "| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |",
    ]
    for run in sorted(runs, key=lambda item: (item.variant, item.run)):
        for phase in PHASES:
            metric = run.phases[phase]
            lines.append(
                f"| {run.variant}/{run.run} | {phase} | {metric.frames} | "
                f"{fmt(metric.gap_p50_ms)}/{fmt(metric.gap_p95_ms)}/{fmt(metric.gap_max_ms)} | "
                f"{fmt(metric.render_p50_ms)}/{fmt(metric.render_p95_ms)}/{fmt(metric.render_max_ms)} | "
                f"{fmt(metric.over_16_67_percent)}% | {fmt(metric.over_33_33_percent)}% | "
                f"{fmt(metric.mounted_median)}/{metric.mounted_max} | {fmt(metric.virtual_blank_max_px)} | {metric.model_commits} | "
                f"{run.post_stop_commits_800ms if phase == 'scroll' else 'n/a'} |"
            )

    lines.extend([
        "",
        "## Direct-input continuity",
        "",
        "Tracking drift is the range of the pointer-to-thumb grab offset during one upward drag. Reverse jump is motion against the active one-direction drag or wheel input.",
        "",
        "| Variant/run | Drag tracking drift max (px) | Drag reverse jump max (px) | Anchor writes during drag | Post-release jump (px) | Wheel direction samples | Wheel reverse jump max (px) |",
        "| --- | ---: | ---: | ---: | ---: | ---: | ---: |",
    ])
    for run in sorted(runs, key=lambda item: (item.variant, item.run)):
        lines.append(
            f"| {run.variant}/{run.run} | {fmt(run.drag_tracking_drift_max_px)} | "
            f"{fmt(run.drag_reverse_jump_max_px)} | {run.drag_anchor_writes} | "
            f"{fmt(run.post_release_jump_px)} | {run.wheel_direction_samples} | "
            f"{fmt(run.wheel_reverse_jump_max_px)} |"
        )

    lines.extend([
        "",
        "## Lifecycle stability",
        "",
        "The startup bottom distance starts at the first automatic history-bottom positioning and includes the initial window-size settle before resize input begins.",
        "",
        "| Variant/run | Startup bottom distance max (px) |",
        "| --- | ---: |",
    ])
    for run in sorted(runs, key=lambda item: (item.variant, item.run)):
        lines.append(
            f"| {run.variant}/{run.run} | {fmt(run.startup_bottom_distance_max_px)} |"
        )

    lines.extend([
        "",
        "## Virtual-window commit latency",
        "",
        "A commit-adjacent frame is the first completed Slint frame after a virtual-window model mutation.",
        "",
        "| Variant/run | Commit gap p50/p95/max (ms) | Mutation p95/max (ms) | Mutation-to-render p95/max (ms) | Inserted max | Removed max | Incremental commits |",
        "| --- | ---: | ---: | ---: | ---: | ---: | ---: |",
    ])
    for run in sorted(runs, key=lambda item: (item.variant, item.run)):
        metric = run.phases["scroll"]
        lines.append(
            f"| {run.variant}/{run.run} | "
            f"{fmt(metric.commit_gap_p50_ms)}/{fmt(metric.commit_gap_p95_ms)}/{fmt(metric.commit_gap_max_ms)} | "
            f"{fmt(metric.mutation_p95_ms)}/{fmt(metric.mutation_max_ms)} | "
            f"{fmt(metric.mutation_to_render_p95_ms)}/{fmt(metric.mutation_to_render_max_ms)} | "
            f"{metric.inserted_rows_max} | {metric.removed_rows_max} | "
            f"{fmt(metric.incremental_commit_percent)}% |"
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
        ("Virtual blank max (px)", "virtual_blank_max_px"),
        ("Model commits", "model_commits"),
        ("Commit-adjacent gap p95 (ms)", "commit_gap_p95_ms"),
        ("Commit-adjacent gap max (ms)", "commit_gap_max_ms"),
        ("Mutation-to-render p95 (ms)", "mutation_to_render_p95_ms"),
        ("Inserted rows max", "inserted_rows_max"),
    )
    lines.extend([
        "",
        "## Median comparison",
        "",
        "Lower is better for every metric below. Delta is optimized minus baseline.",
        "",
        "| Phase | Metric | Baseline | Optimized | Delta |",
        "| --- | --- | ---: | ---: | ---: |",
    ])
    for phase in PHASES:
        for label, field in fields:
            baseline = median_metric(grouped["baseline"], phase, field)
            optimized = median_metric(grouped["optimized"], phase, field)
            lines.append(
                f"| {phase} | {label} | {fmt(baseline)} | {fmt(optimized)} | {comparison(optimized, baseline)} |"
            )
        if phase == "scroll":
            baseline = statistics.median(
                run.post_stop_commits_800ms for run in grouped["baseline"]
            )
            optimized = statistics.median(
                run.post_stop_commits_800ms for run in grouped["optimized"]
            )
            lines.append(
                "| scroll | Post-stop commits (800 ms) | "
                f"{fmt(float(baseline))} | {fmt(float(optimized))} | "
                f"{comparison(float(optimized), float(baseline))} |"
            )
    lines.append("")
    return "\n".join(lines)


def validate_acceptance(runs: list[RunMetrics]) -> list[str]:
    failures: list[str] = []
    for run in runs:
        if run.variant != "optimized":
            continue
        scroll = run.phases["scroll"]
        if run.startup_bottom_distance_max_px > 1.0:
            failures.append(
                f"{run.run}: startup bottom distance {fmt(run.startup_bottom_distance_max_px)} px exceeds 1.00 px"
            )
        if math.isnan(scroll.commit_gap_max_ms) or scroll.commit_gap_max_ms > 50.0:
            failures.append(
                f"{run.run}: commit-adjacent frame gap max {fmt(scroll.commit_gap_max_ms)} ms exceeds 50.00 ms"
            )
        if scroll.virtual_blank_max_px > 1.0:
            failures.append(
                f"{run.run}: stable virtual blank {fmt(scroll.virtual_blank_max_px)} px exceeds 1.00 px"
            )
        if run.post_stop_commits_800ms != 0:
            failures.append(
                f"{run.run}: {run.post_stop_commits_800ms} model commits occurred within 800 ms after stop"
            )
        if scroll.mounted_max > 32:
            failures.append(
                f"{run.run}: mounted row maximum {scroll.mounted_max} exceeds 32"
            )
        if run.drag_tracking_drift_max_px > 1.0:
            failures.append(
                f"{run.run}: drag tracking drift {fmt(run.drag_tracking_drift_max_px)} px exceeds 1.00 px"
            )
        if run.drag_reverse_jump_max_px > 1.0:
            failures.append(
                f"{run.run}: drag reverse jump {fmt(run.drag_reverse_jump_max_px)} px exceeds 1.00 px"
            )
        if run.drag_anchor_writes != 0:
            failures.append(
                f"{run.run}: {run.drag_anchor_writes} anchor writes occurred during scrollbar drag"
            )
        if run.post_release_jump_px > 1.0:
            failures.append(
                f"{run.run}: post-release jump {fmt(run.post_release_jump_px)} px exceeds 1.00 px"
            )
        if run.wheel_direction_samples < 10:
            failures.append(
                f"{run.run}: only {run.wheel_direction_samples} Flickable wheel-direction samples were recorded"
            )
        if run.wheel_reverse_jump_max_px > 1.0:
            failures.append(
                f"{run.run}: wheel reverse jump {fmt(run.wheel_reverse_jump_max_px)} px exceeds 1.00 px"
            )
    return failures


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    traces = sorted((args.directory / "runs").glob("*-run-*.tsv"))
    if len(traces) < 6:
        raise SystemExit(f"expected at least six traces, found {len(traces)}")
    try:
        runs = [analyze_trace(path) for path in traces]
        summary = render_summary(runs)
    except ValueError as error:
        raise SystemExit(str(error)) from error
    summary_path = args.directory / "summary.md"
    summary_path.write_text(summary, encoding="utf-8")
    print(summary)
    failures = validate_acceptance(runs)
    if failures:
        raise SystemExit("Chat interaction acceptance failed:\n- " + "\n- ".join(failures))


if __name__ == "__main__":
    main()
