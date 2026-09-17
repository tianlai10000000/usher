#!/usr/bin/env python3
"""Controlled macOS Release A/B through opt-in offline production fixtures."""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import re
import signal
import statistics
import subprocess
import time


def percentile(values, fraction):
    return sorted(values)[int((len(values) - 1) * fraction)] if values else None


def linear_slope(points):
    if len(points) < 2:
        return None
    x = statistics.mean(p[0] for p in points)
    y = statistics.mean(p[1] for p in points)
    variance = sum((p[0] - x) ** 2 for p in points)
    return sum((a - x) * (b - y) for a, b in points) / variance if variance else None


def terminal_replay_cycles(log, batch):
    marker = re.escape(f"[P6][{batch}]")
    pattern = re.compile(marker + r" (?:replay_applied terminal=(true|false)|cycle=\d+ completed_ms=\d+)")
    last_replay = None
    cycles = []
    for match in pattern.finditer(log):
        if match.group(1) is not None:
            last_replay = match.group(1) == "true"
        else:
            cycles.append(last_replay is True)
            last_replay = None
    return cycles


def run(binary, directory, seconds, batch=65, soak=False):
    state = directory / "state/default/run-0"
    state.mkdir(parents=True)
    (state / "settings.toml").write_text(
        'theme_mode = 0\nlanguage = "en"\n[diagnostics]\nterminal_verbose_logging = false\n'
    )
    env = os.environ.copy()
    env.update(USHER_BENCHMARK_OFFLINE="1", USHER_P6_BENCHMARK=str(batch),
               USHER_BENCHMARK_STATE_ROOT=str(directory / "state"),
               USHER_AI_REVIEW_RESPONSE_VARIANT="default", USHER_MEMORY_BENCHMARK_RUN="0",
               USHER_MEMORY_DIAGNOSTICS="1", USHER_MEMORY_BUILD_PROFILE="release",
               USHER_MEMORY_SCENARIO=f"p6-{batch}", USHER_TOKIO_WORKER_THREADS="2",
               SLINT_BACKEND="winit-femtovg", SHELL="/bin/sh", ENV="/dev/null")
    env.pop("USHER_P6_SOAK_SECONDS", None)
    if soak:
        env["USHER_P6_SOAK_SECONDS"] = str(int(seconds))
    samples = []
    interrupted = False
    log_offset = 0
    last_log_check = 0
    start = time.monotonic()
    log_path = directory / "app.log"
    with log_path.open("w") as log:
        process = subprocess.Popen([str(binary)], env=env, stdout=log, stderr=log)
        try:
            while time.monotonic() - start < seconds + (60 if soak else 0) and process.poll() is None:
                if soak and time.monotonic() - last_log_check >= 1:
                    with log_path.open(errors="replace") as reader:
                        reader.seek(log_offset)
                        new_log = reader.read()
                        log_offset = reader.tell()
                    last_log_check = time.monotonic()
                    if f"[P6][{batch}] complete" in new_log:
                        break
                raw = subprocess.run(
                    ["ps", "-p", str(process.pid), "-o", "rss=,%cpu="], text=True,
                    capture_output=True, check=False).stdout.split()
                if len(raw) != 2:
                    break
                samples.append(dict(seconds=time.monotonic() - start,
                                    rss_mib=int(raw[0]) / 1024, cpu_percent=float(raw[1])))
                time.sleep(.1)
        except KeyboardInterrupt:
            interrupted = True
        finally:
            stop = time.monotonic()
            if process.poll() is None:
                process.send_signal(signal.SIGTERM)
            try:
                process.wait(timeout=8)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
            shutdown_ms = (time.monotonic() - stop) * 1000
    (directory / "samples.json").write_text(json.dumps(samples, indent=2))
    log = log_path.read_text(errors="replace")
    first = [s["rss_mib"] for s in samples if seconds * .25 <= s["seconds"] < seconds * .5]
    last = [s["rss_mib"] for s in samples if s["seconds"] >= seconds * .75]
    marker = re.escape(f"[P6][{batch}]")
    heartbeats = [int(v) / 1000 for v in re.findall(marker + r" heartbeat_us=(\d+)", log)]
    cycles = [int(v) for v in re.findall(marker + r" cycle=\d+ completed_ms=(\d+)", log)]
    footprints = [float(v) for v in re.findall(r" phys_footprint_mib=([\d.]+)", log)]
    lifetime_peaks = [float(v) for v in re.findall(r" rss_lifetime_peak_mib=([\d.]+)", log)]
    progress = [(int(n), int(ms)) for n, ms in re.findall(
        marker + r" progress cycles=(\d+) elapsed_ms=(\d+)", log)]
    complete = not interrupted and f"[P6][{batch}] complete" in log and (len(cycles) >= 3 if soak else len(cycles) == 3)
    warm = [s for s in samples if s["seconds"] >= seconds * .25]
    slope = linear_slope([(s["seconds"] / 60, s["rss_mib"]) for s in warm])
    footprint_points = [(int(ms) / 60000, float(value)) for ms, value in re.findall(
        r"\[Memory\]\[Diag\][^\n]*? elapsed_ms=(\d+)[^\n]*? phys_footprint_mib=([\d.]+)", log)
        if int(ms) >= seconds * 250]
    result = dict(batch=batch, complete=complete, interrupted=interrupted, clean_exit=process.returncode == 0,
                  soak=soak, requested_seconds=seconds, progress=progress,
                  cycle_ids=[int(v) for v in re.findall(marker + r" cycle=(\d+) completed_ms=", log)],
                  rss_slope_mib_min=slope,
                  footprint_slope_mib_min=linear_slope(footprint_points),
                  resources={key: [int(v) for v in re.findall(r" " + key + r"=(\d+)", log)]
                             for key in ("threads", "mach_ports", "regions")},
                  peak_mib=max(s["rss_mib"] for s in samples) if samples else None,
                  lifetime_peak_mib=max(lifetime_peaks) if lifetime_peaks else None,
                  settled_mib=statistics.median(last) if last else None,
                  growth_mib=statistics.median(last) - statistics.median(first) if first and last else None,
                  footprint_peak_mib=max(footprints) if footprints else None,
                  footprint_final_mib=footprints[-1] if footprints else None,
                  ui_p95_ms=percentile(heartbeats, .95), ui_max_ms=max(heartbeats) if heartbeats else None,
                  cycles_ms=cycles, shutdown_ms=shutdown_ms,
                  cpu_p95=percentile([s["cpu_percent"] for s in samples], .95),
                  framebuffer_errors=log.count("Invalid framebuffer"),
                  premature_loading_clears=log.count(f"[P6][{batch}] premature_loading_clear"),
                  journal_bytes=[int(v) for v in re.findall(marker + r" journal_bytes=(\d+)", log)],
                  replay_successes=log.count(f"[P6][{batch}] replay_applied terminal=true"),
                  replay_failures=log.count(f"[P6][{batch}] replay_applied terminal=false"),
                  terminal_replay_cycles=terminal_replay_cycles(log, batch),
                  output_writes=[int(v) for v in re.findall(marker + r" output_writes=(\d+)", log)],
                  prepared=[int(v) for v in re.findall(marker + r" output_writes=\d+ prepared=(\d+)", log)],
                  shared_snapshots=[int(v) for v in re.findall(marker + r" shared_snapshots=(\d+)", log)],
                  retained_layouts=[dict(cycle=int(c), source=int(s), source_bytes=int(raw),
                                         measurement_bytes=int(measured), wrapped_bytes=int(wrapped))
                                    for c, s, raw, measured, wrapped in re.findall(
                                        marker + r" layout_ready cycle=(\d+) source=(\d+) source_bytes=(\d+) measurement_bytes=(\d+) wrapped_bytes=(\d+)", log)],
                  oversized_rejections=log.count(f"[P6][{batch}] response large=true rejected"),
                  small_responses=len(re.findall(marker + r" response large=false accepted_bytes=\d+", log)))
    (directory / "result.json").write_text(json.dumps(result, indent=2))
    return result


def migration_summary(results):
    keys = ["peak_mib", "lifetime_peak_mib", "settled_mib", "growth_mib", "ui_p95_ms",
            "footprint_peak_mib", "footprint_final_mib"]
    valid = set(results) == {"A", "B"} and all(len(rows) >= 3 for rows in results.values())
    valid &= all(row["complete"] and row["clean_exit"] and row["framebuffer_errors"] == 0
                 and all(row[key] is not None for key in keys)
                 and len(row.get("cycles_ms", [])) == 3
                 and all(value > 0 for value in row["cycles_ms"])
                 for rows in results.values() for row in rows)
    medians = {}
    passed = False
    if valid:
        medians = {s: {key: statistics.median(r[key] for r in rows) for key in keys}
                   for s, rows in results.items()}
        for side, rows in results.items():
            medians[side]["cycle_ms"] = statistics.median(
                statistics.median(row["cycles_ms"]) for row in rows)
        a, b = medians["A"], medians["B"]
        passed = all(b[k] - a[k] <= max(8, a[k] * .05)
                     for k in ["peak_mib", "lifetime_peak_mib", "settled_mib",
                               "footprint_peak_mib", "footprint_final_mib"])
        passed &= b["growth_mib"] <= 8 and b["growth_mib"] - a["growth_mib"] <= 8
        passed &= b["ui_p95_ms"] - a["ui_p95_ms"] <= max(8, a["ui_p95_ms"] * .1)
        # Polling fixtures observe completion at 50-100 ms intervals.
        passed &= b["cycle_ms"] - a["cycle_ms"] <= max(100, a["cycle_ms"] * .2)
        passed &= all(row.get("premature_loading_clears", 0) == 0 for row in results["B"])
        passed &= all(row.get("oversized_rejections") == 3 and row.get("small_responses") == 6
                      for row in results["B"] if row.get("batch") == 67)
        passed &= all(row.get("small_responses") == 6
                      for row in results["A"] if row.get("batch") == 67)
        passed &= all(row.get("replay_successes") == 3 and row.get("replay_failures") == 0
                      and len(row.get("journal_bytes", [])) == 3
                      for rows in results.values() for row in rows if row.get("batch") == 68)
        passed &= all(row.get("terminal_replay_cycles") == [True] * 3
                      for rows in results.values() for row in rows if row.get("batch") in (68, 69))
        passed &= all(max(row["journal_bytes"], default=0) <= 2 * 1024 * 1024 + 64 * 1024
                      for row in results["B"] if row.get("batch") == 68)
        passed &= all(row.get("output_writes") == [0, 0, 0]
                      and row.get("shared_snapshots") == [100] * 6
                      and row.get("replay_successes", 0) >= 3
                      and len(row.get("prepared", [])) == 3
                      and all(0 < count <= 200 for count in row["prepared"])
                      for row in results["B"] if row.get("batch") == 69)
        passed &= all(len(row.get("output_writes", [])) == 3
                      and all(count > 0 for count in row["output_writes"])
                      and row.get("shared_snapshots") == [0] * 6
                      for row in results["A"] if row.get("batch") == 69)
        passed &= all([(entry["cycle"], entry["source"]) for entry in row.get("retained_layouts", [])]
                      == [(cycle, source) for cycle in range(3) for source in range(16)]
                      for rows in results.values() for row in rows if row.get("batch") == 70)
        passed &= all(entry.get("source_bytes", 0) > 0 and entry.get("measurement_bytes") == 0
                      and entry.get("wrapped_bytes", 0) > 0
                      for row in results["B"] if row.get("batch") == 70
                      for entry in row.get("retained_layouts", []))
        passed &= all(entry.get("measurement_bytes", 0) > 0 and entry.get("wrapped_bytes", 0) > 0
                      for row in results["A"] if row.get("batch") == 70
                      for entry in row.get("retained_layouts", []))
    return dict(valid=valid, medians=medians, passed=passed)


def regression_errors(row):
    """Current implementations must satisfy the same invariants on both sides."""
    errors = []
    n = len(row.get("cycles_ms", []))
    def require(ok, reason):
        if not ok:
            errors.append(reason)
    def bounded(key, limit, minimum=0):
        value = row.get(key)
        return type(value) in (int, float) and math.isfinite(value) and minimum <= value <= limit
    require(n >= 3 and row.get("cycle_ids") == list(range(n)), "missing/duplicate/out-of-order cycles")
    require(row.get("complete") and row.get("clean_exit"), "incomplete workload or shutdown")
    require(bounded("shutdown_ms", 7999.999), "shutdown exceeded 8 seconds")
    require(row.get("framebuffer_errors") == 0, "framebuffer error")
    require(row.get("premature_loading_clears", 0) == 0, "stale highlight completion")
    for key in ("peak_mib", "lifetime_peak_mib", "settled_mib", "footprint_peak_mib", "footprint_final_mib"):
        require(bounded(key, 512), f"{key} exceeds 512 MiB or missing")
    require(bounded("ui_p95_ms", 100), "heartbeat p95 exceeds 100 ms")
    require(bounded("growth_mib", 8, -512), "RSS growth exceeds 8 MiB")
    require(all(v > 0 for v in row.get("cycles_ms", [])), "invalid cycle duration")
    batch = row.get("batch")
    if batch == 67:
        require(row.get("oversized_rejections") == n and row.get("small_responses") == 2 * n,
                "HTTP budget/followup failure")
    if batch in (68, 69):
        require(row.get("terminal_replay_cycles") == [True] * n,
                "missing terminal replay")
    if batch == 68:
        require(row.get("replay_failures") == 0, "incomplete finished-journal replay")
        sizes = row.get("journal_bytes", [])
        require(len(sizes) == n and all(0 < v <= 2 * 1024 * 1024 + 64 * 1024 for v in sizes),
                "journal budget failure")
    if batch == 69:
        require(row.get("output_writes") == [0] * n, "output-only persistence")
        require(row.get("shared_snapshots") == [100] * (2 * n), "configuration snapshot copies")
        require(len(row.get("prepared", [])) == n and all(0 < v <= 200 for v in row["prepared"]),
                "missing/unbounded Job projections")
    if batch == 70:
        entries = row.get("retained_layouts", [])
        require([(e["cycle"], e["source"]) for e in entries] ==
                [(c, s) for c in range(n) for s in range(16)], "missing/duplicate layout source")
        require(all(e["source_bytes"] > 0 and e["measurement_bytes"] == 0 and e["wrapped_bytes"] > 0
                    for e in entries), "unshared/empty layout")
    if row.get("soak"):
        progress = row.get("progress", [])
        require([p[0] for p in progress] == list(range(1, n + 1)), "missing progress evidence")
        elapsed = [0] + [p[1] for p in progress]
        require(elapsed[-1] >= row["requested_seconds"] * 1000, "workload stopped before duration")
        require(all(0 < b - a <= 60000 for a, b in zip(elapsed, elapsed[1:])), "workload stalled over 60 seconds")
        require(bounded("rss_slope_mib_min", .5, -512),
                "RSS slope exceeds 0.5 MiB/min")
        require(bounded("footprint_slope_mib_min", .5, -512),
                "footprint slope exceeds 0.5 MiB/min")
        for key, budget in (("threads", 4), ("mach_ports", 32), ("regions", 256)):
            values = row.get("resources", {}).get(key, [])
            warm = values[len(values) // 4:]
            width = max(1, len(warm) // 4)
            require(len(values) >= 6 and statistics.median(warm[-width:]) - statistics.median(warm[:width]) <= budget,
                    f"{key} growth exceeds {budget} or missing")
    return errors


def summarize(results, mode="migration"):
    if mode == "migration":
        return migration_summary(results)
    # Reuse the established relative memory/latency budgets, without old-A implementation assumptions.
    normalized = {side: [dict(row, batch=0, cycles_ms=[statistics.median(row["cycles_ms"])] * 3)
                         if row.get("cycles_ms") else row for row in rows]
                  for side, rows in results.items()}
    summary = migration_summary(normalized)
    summary["errors"] = {side: [regression_errors(row) for row in rows] for side, rows in results.items()}
    summary["passed"] &= not any(e for rows in summary["errors"].values() for e in rows)
    return summary


def main():
    def interrupt(signum, frame):
        raise KeyboardInterrupt
    signal.signal(signal.SIGTERM, interrupt)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--a", type=Path, required=True)
    parser.add_argument("--b", type=Path, required=True)
    parser.add_argument("--seconds", type=float, default=30)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--batch", type=int, choices=[65, 66, 67, 68, 69, 70], default=65)
    parser.add_argument("--mode", choices=["migration", "regression"], default="migration")
    parser.add_argument("--soak", action="store_true", help="Repeat real workload for --seconds, then finish its last cycle")
    args = parser.parse_args()
    if args.seconds < 30:
        parser.error("At least 30 seconds per fresh process is required")
    if args.soak and (args.mode != "regression" or args.seconds > 3600 or int(args.seconds) != args.seconds):
        parser.error("Soak requires regression mode and integral seconds in 30..3600")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    binaries = {side: getattr(args, side.lower()).resolve() for side in "AB"}
    metadata = dict(task=args.batch, mode=args.mode, soak=args.soak, seconds=args.seconds, pairs=3, order="AB/BA/AB",
                    platform=platform.platform(), machine=platform.machine(),
                    profile="release", renderer="winit-femtovg", window="1100x700 logical",
                    memory_budget="peak/stable <= A + max(8 MiB, 5% A); B growth and growth delta <= 8 MiB",
                    responsiveness_budget="p95 heartbeat <= A + max(8 ms, 10% A)",
                    throughput_budget="median cycle <= A + max(100 ms, 20% A)",
                    binaries={s: dict(path=str(p), sha256=hashlib.sha256(p.read_bytes()).hexdigest())
                              for s, p in binaries.items()})
    (output / "metadata.json").write_text(json.dumps(metadata, indent=2))
    results = {side: [] for side in "AB"}
    for index in range(3):
        for side in ("AB" if index % 2 == 0 else "BA"):
            directory = output / f"{side}-{index + 1}"
            directory.mkdir()
            result = run(binaries[side], directory, args.seconds, args.batch, args.soak)
            results[side].append(result)
            print(side, index + 1, json.dumps(result), flush=True)
            if result.get("interrupted"):
                (output / "summary.json").write_text(json.dumps(dict(passed=False, status="cancelled")))
                raise SystemExit(130)
    summary = summarize(results, args.mode)
    (output / "summary.json").write_text(json.dumps(summary, indent=2))
    print(json.dumps(summary, indent=2), flush=True)
    raise SystemExit(0 if summary["passed"] else 1)


if __name__ == "__main__":
    main()
