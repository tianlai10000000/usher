#!/usr/bin/env python3
"""Controlled macOS Release A/B through opt-in offline production fixtures."""
import argparse
import hashlib
import json
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


def run(binary, directory, seconds, batch=65):
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
    samples = []
    start = time.monotonic()
    log_path = directory / "app.log"
    with log_path.open("w") as log:
        process = subprocess.Popen([str(binary)], env=env, stdout=log, stderr=log)
        try:
            while time.monotonic() - start < seconds and process.poll() is None:
                raw = subprocess.check_output(
                    ["ps", "-p", str(process.pid), "-o", "rss=,%cpu="], text=True).split()
                samples.append(dict(seconds=time.monotonic() - start,
                                    rss_mib=int(raw[0]) / 1024, cpu_percent=float(raw[1])))
                time.sleep(.1)
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
    complete = f"[P6][{batch}] complete" in log and len(cycles) == 3
    result = dict(batch=batch, complete=complete, clean_exit=process.returncode == 0,
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
                  oversized_rejections=log.count(f"[P6][{batch}] response large=true rejected"),
                  small_responses=len(re.findall(marker + r" response large=false accepted_bytes=\d+", log)))
    (directory / "result.json").write_text(json.dumps(result, indent=2))
    return result


def summarize(results):
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
    return dict(valid=valid, medians=medians, passed=passed)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--a", type=Path, required=True)
    parser.add_argument("--b", type=Path, required=True)
    parser.add_argument("--seconds", type=float, default=30)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--batch", type=int, choices=[65, 66, 67], default=65)
    args = parser.parse_args()
    if args.seconds < 30:
        parser.error("At least 30 seconds per fresh process is required")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    binaries = {side: getattr(args, side.lower()).resolve() for side in "AB"}
    metadata = dict(task=args.batch, seconds=args.seconds, pairs=3, order="AB/BA/AB",
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
            result = run(binaries[side], directory, args.seconds, args.batch)
            results[side].append(result)
            print(side, index + 1, json.dumps(result), flush=True)
    summary = summarize(results)
    (output / "summary.json").write_text(json.dumps(summary, indent=2))
    print(json.dumps(summary, indent=2), flush=True)
    raise SystemExit(0 if summary["passed"] else 1)


if __name__ == "__main__":
    main()
