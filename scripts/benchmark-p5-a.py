#!/usr/bin/env python3
"""Paired macOS Release RSS checks for P5-A, using isolated app state.

Run with --a/--b binaries captured before/after ONE task. This measures the real
app (including Slint); task shutdown race tests are a separate functional gate.
Logo fixtures use only a local HTTP server and never contact a Git provider.
"""
import argparse
import hashlib
import http.server
import json
import os
from pathlib import Path
import signal
import statistics
import subprocess
import threading
import time


def fingerprint(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def seed(directory, scenario, port):
    directory.mkdir(parents=True)
    (directory / "settings.toml").write_text(
        'theme_mode = 0\nlanguage = "en"\nterminal_verbose_logging = false\n'
    )
    if scenario == "tasks":
        tasks = {}
        for index in range(128):
            key = f"p5-task-{index}"
            tasks[key] = dict(
                id=key,
                target=dict(project_key="p5", project_name="P5 fixture",
                            pull_request_number=str(index),
                            pull_request_title="Persistence fixture " + "x" * 256,
                            base_sha="a" * 40, head_sha="b" * 40),
                rule_name="fixture", model_name="fixture", status="running",
                progress_percent=50, completed_chunks=1, total_chunks=2,
                finding_count=0, error="", created_at_unix_ms=1, updated_at_unix_ms=1,
            )
        (directory / "ai-review").mkdir()
        (directory / "ai-review/tasks.json").write_text(
            json.dumps(dict(tasks=tasks, order=list(tasks)))
        )
    else:
        (directory / "project").mkdir()
        (directory / "project/projects.json").write_text(json.dumps([
            dict(project_name=f"P5 project {i}", project_alias=f"p5-{i}",
                 local_repo="/tmp", remote_repo="", repo_type="github",
                 repo_project_id=str(i), project_tags=[], is_pr_auto_update=False,
                 logo=f"http://127.0.0.1:{port}/missing-{i}.png")
            for i in range(32)
        ]))


def run(binary, scenario, directory, duration):
    metrics = dict(requests=0, active=0, peak_active=0, per_url={})
    lock = threading.Lock()
    # Both sides get the same startup grace period. Immediate failures can
    # starve the old UI before window reveal, making visible-window RSS unequal.
    responses_start = time.monotonic() + 3

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            with lock:
                metrics["requests"] += 1
                metrics["per_url"][self.path] = metrics["per_url"].get(self.path, 0) + 1
                metrics["active"] += 1
                metrics["peak_active"] = max(metrics["active"], metrics["peak_active"])
            try:
                time.sleep(max(0, responses_start - time.monotonic()))
                time.sleep(0.05)
                self.send_response(404)
                self.send_header("Content-Length", "0")
                self.end_headers()
            finally:
                with lock:
                    metrics["active"] -= 1

        def log_message(self, *_):
            pass

    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    state_root = directory / "state"
    seed(state_root / "default/run-0", scenario, server.server_port)
    env = os.environ.copy()
    env.update(USHER_BENCHMARK_OFFLINE="1", USHER_BENCHMARK_STATE_ROOT=str(state_root),
               USHER_AI_REVIEW_RESPONSE_VARIANT="default", USHER_MEMORY_BENCHMARK_RUN="0",
               USHER_MEMORY_DIAGNOSTICS="1", USHER_MEMORY_BUILD_PROFILE="release",
               USHER_MEMORY_SCENARIO=f"p5-{scenario}", USHER_TOKIO_WORKER_THREADS="2",
               SLINT_BACKEND="winit-femtovg")
    log_path = directory / "app.log"
    samples = []
    ready = False
    start = time.monotonic()
    with log_path.open("w") as log:
        process = subprocess.Popen([str(binary)], env=env, stdout=log, stderr=log)
        try:
            while time.monotonic() - start < duration:
                if process.poll() is not None:
                    raise RuntimeError(f"App exited early: {log_path}")
                rss = int(subprocess.check_output(
                    ["ps", "-p", str(process.pid), "-o", "rss="], text=True).strip())
                elapsed = time.monotonic() - start
                samples.append(dict(seconds=elapsed, rss_mib=rss / 1024))
                ready |= "[Startup] Window revealed" in log_path.read_text(errors="replace")
                time.sleep(0.25)
        finally:
            with lock:
                measured_requests = {**metrics, "per_url": metrics["per_url"].copy()}
            stop_start = time.monotonic()
            process.send_signal(signal.SIGTERM)
            try:
                process.wait(timeout=8)
                clean_exit = process.returncode == 0
            except subprocess.TimeoutExpired:
                clean_exit = False
                process.kill()
                process.wait()
            shutdown_ms = (time.monotonic() - stop_start) * 1000
            server.shutdown()
            server.server_close()
    (directory / "samples.json").write_text(json.dumps(samples, indent=2))
    if not ready:
        raise RuntimeError(f"Window never became ready: {log_path}")
    if scenario == "logos" and metrics["requests"] == 0:
        raise RuntimeError("Logo fixture did not execute")
    if scenario == "tasks":
        persisted = json.loads((state_root / "default/run-0/ai-review/tasks.json").read_text())
        if len(persisted["tasks"]) != 128 or any(
                task["status"] != "failed" for task in persisted["tasks"].values()):
            raise RuntimeError("Task fixture did not hydrate and persist all 128 interrupted tasks")
    first = [s["rss_mib"] for s in samples if duration * 0.25 <= s["seconds"] < duration * 0.5]
    last = [s["rss_mib"] for s in samples if s["seconds"] >= duration * 0.75]
    result = dict(peak_mib=max(s["rss_mib"] for s in samples),
                  settled_mib=statistics.median(last),
                  growth_mib=statistics.median(last) - statistics.median(first),
                  shutdown_ms=shutdown_ms, clean_exit=clean_exit, **measured_requests)
    (directory / "result.json").write_text(json.dumps(result, indent=2))
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--a", type=Path, required=True)
    parser.add_argument("--b", type=Path, required=True)
    parser.add_argument("--scenario", choices=["tasks", "logos"], required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--seconds", type=float, default=24)
    args = parser.parse_args()
    if args.runs < 3 or args.seconds < 16:
        parser.error("Use at least 3 pairs and 16 seconds per fresh process")
    args.output = args.output.resolve()
    args.output.mkdir(parents=True, exist_ok=False)
    binaries = {side: getattr(args, side.lower()).resolve() for side in ("A", "B")}
    results = {side: [] for side in binaries}
    metadata = dict(scenario=args.scenario, runs=args.runs, seconds=args.seconds,
                    logo_response_startup_grace_seconds=3, logo_response_delay_seconds=0.05,
                    binaries={s: dict(path=str(p), sha256=fingerprint(p)) for s, p in binaries.items()},
                    peak_budget="max(8 MiB, 5% of A median peak)", growth_budget_mib=8)
    (args.output / "metadata.json").write_text(json.dumps(metadata, indent=2))
    for index in range(args.runs):
        for side in ("AB" if index % 2 == 0 else "BA"):
            directory = args.output / f"{side}-{index + 1}"
            directory.mkdir()
            result = run(binaries[side], args.scenario, directory, args.seconds)
            results[side].append(result)
            print(side, index + 1, json.dumps(result), flush=True)
    medians = {side: {key: statistics.median(r[key] for r in rows)
                      for key in ("peak_mib", "settled_mib", "growth_mib", "requests", "peak_active")}
               for side, rows in results.items()}
    a, b = medians["A"], medians["B"]
    passed = (b["peak_mib"] - a["peak_mib"] <= max(8, a["peak_mib"] * 0.05)
              and b["settled_mib"] - a["settled_mib"] <= max(8, a["settled_mib"] * 0.05)
              and b["growth_mib"] <= 8 and b["growth_mib"] - a["growth_mib"] <= 8
              and all(r["clean_exit"] for r in results["B"]))
    if args.scenario == "logos":
        passed &= all(r["requests"] == 32 * 3 and r["peak_active"] <= 4
                      and len(r["per_url"]) == 32
                      and all(count == 3 for count in r["per_url"].values())
                      for r in results["B"])
    summary = dict(medians=medians, passed=passed)
    (args.output / "summary.json").write_text(json.dumps(summary, indent=2))
    print(json.dumps(summary, indent=2))
    raise SystemExit(0 if passed else 1)


if __name__ == "__main__":
    main()
