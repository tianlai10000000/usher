#!/usr/bin/env python3
"""Run serial Release hot-path fixtures and enforce absolute and paired budgets.

Without --baseline, A/B are repeatability cohorts of the same executable, not
evidence of an optimization. Raw logs, samples and failed runs are retained.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import re
import statistics
import subprocess
import sys


CASES = {
    "project": ("b", "57", 30, []),
    "job": ("b", "58", 30, ["--short-lines"]),
    "preview-code": ("c", "60", 40, ["--scenario", "code"]),
    "preview-paragraphs": ("c", "60", 40, ["--scenario", "paragraphs"]),
    "find": ("c", "61", 80, ["--scenario", "find"]),
}
LIMITS = dict(peak_mib=256, settled_mib=256, growth_mib=8,
              project_p95_ms=50, job_p95_ms=50, layout_p95_ms=25,
              created_rows=1024, created_lines=2000, find_latency_ms=2000)


def fingerprint(path):
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def read_json(path):
    return json.loads(path.read_text())


def require(condition, message):
    if not condition:
        raise ValueError(message)


def number(value, label, minimum=None, maximum=None):
    require(type(value) in (int, float) and math.isfinite(value), f"{label}: invalid number")
    require(minimum is None or value >= minimum, f"{label}: below {minimum}: {value}")
    require(maximum is None or value <= maximum, f"{label}: exceeds {maximum}: {value}")
    return value


def validate_case(directory, name, hashes):
    family, task, seconds, _ = CASES[name]
    metadata = read_json(directory / "metadata.json")
    require(metadata["task"] == task and metadata["seconds"] == seconds
            and metadata["pairs"] == 3 and metadata["order"] == "AB/BA/AB", "fixture protocol mismatch")
    for side in "AB":
        require(metadata["binaries"][side]["sha256"] == hashes[side], "binary hash mismatch")
    if family == "b":
        require(metadata["diagnostics"] is False, "diagnostics mode mismatch")
        require(metadata["short_lines"] is (name == "job"), "job mode mismatch")
        require(metadata["project_count"] == (128 if name == "project" else 0), "project count mismatch")
        if name == "job":
            require(metadata["fixture_adapter_sha256"] == fingerprint(directory / "fixture-python"),
                    "fixture adapter mismatch")
    else:
        require(metadata["scenario"] == name.removeprefix("preview-"), "scenario mismatch")
    require(read_json(directory / "summary.json")["passed"] is True, "paired runner failed")
    rows = {side: [] for side in "AB"}
    for side in "AB":
        for index in range(1, 4):
            run = directory / f"{side}-{index}"
            row = read_json(run / "result.json")
            log = (run / "app.log").read_text(errors="replace")
            require(row["clean_exit"] is True and row["fixture_complete"] is True, f"{run.name}: incomplete run")
            require("[Startup] Window revealed" in log and f"[P5][{task}] complete" in log,
                    f"{run.name}: missing app evidence")
            require("Invalid framebuffer" not in log, f"{run.name}: framebuffer failure")
            samples = read_json(run / "samples.json")
            require(len(samples) >= seconds * 2, f"{run.name}: insufficient RSS samples")
            previous = -1
            for sample in samples:
                timestamp = number(sample["seconds"], "sample time", 0, seconds + 2)
                require(timestamp > previous, "sample times must increase")
                previous = timestamp
                number(sample["rss_mib"], "sample RSS", 1, LIMITS["peak_mib"])
            require(samples[0]["seconds"] < seconds * .25 and previous >= seconds * .95,
                    "RSS observation does not cover the run")
            first = [s["rss_mib"] for s in samples if seconds * .25 <= s["seconds"] < seconds * .5]
            last = [s["rss_mib"] for s in samples if s["seconds"] >= seconds * .75]
            require(len(first) >= 3 and len(last) >= 3, "missing settled RSS windows")
            measured = dict(peak_mib=max(s["rss_mib"] for s in samples),
                            settled_mib=statistics.median(last),
                            growth_mib=statistics.median(last) - statistics.median(first))
            for key, value in measured.items():
                number(row[key], key, maximum=LIMITS[key])
                require(math.isclose(row[key], value, abs_tol=.000001), f"{key}: sample/result mismatch")
            if name in ("project", "job"):
                number(row["dispatch_to_ui_p95_ms"], "callback P95", 0, LIMITS[f"{name}_p95_ms"])
                if name == "project":
                    require(row["rounds"] == 24 and "complete rows=128 updated=64" in log, "project rounds incomplete")
                    number(row["ui_commits"], "project UI commits", 1, 64)
                else:
                    number(row["peak_queue"], "queue blocks", 1, 16)
                    number(row["tail_bytes"], "tail bytes", 1, 32768)
                    number(row["cancel_ms"], "cancellation", 0, 2000)
                    number(row["created_rows"], "job row creations", 1, LIMITS["created_rows"])
                    require("output_ok=true cancel_ok=true tail=true" in log, "job result incomplete")
            else:
                number(row["ui_p95_ms"], "layout P95", 0, LIMITS["layout_p95_ms"])
                require(row["framebuffer_errors"] == 0, "framebuffer errors")
                if name == "find":
                    matches = re.findall(r"\[P5\]\[61\] round=(\d+) count=(\d+) valid=true latency_ms=(\d+)", log)
                    require([int(m[0]) for m in matches] == list(range(12)), "Find rounds incomplete")
                    for round_id, count, latency in matches:
                        expected = 30000 if int(round_id) < 4 else 60000 if int(round_id) < 8 else 7000
                        require(int(count) == expected, "Find count mismatch")
                        number(int(latency), "Find delivery latency", 0, LIMITS["find_latency_ms"])
                else:
                    require("[P5][60] result tail=true" in log, "Preview tail not reached")
                    if name == "preview-code":
                        number(row["created_lines"], "Preview line creations", 1, LIMITS["created_lines"])
            rows[side].append(row)
    medians = {side: {key: statistics.median(row[key] for row in values)
                      for key in ("peak_mib", "settled_mib", "growth_mib")}
               for side, values in rows.items()}
    a, b = medians["A"], medians["B"]
    for key in ("peak_mib", "settled_mib"):
        require(b[key] - a[key] <= max(8, a[key] * .05), f"paired {key} regression")
    require(b["growth_mib"] - a["growth_mib"] <= 8, "paired growth regression")
    return dict(passed=True, medians=medians)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--baseline", type=Path, help="optional captured pre-change Release executable")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    binaries = dict(A=(args.baseline or args.binary).resolve(), B=args.binary.resolve())
    hashes = {side: fingerprint(path) for side, path in binaries.items()}
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    report = dict(protocol="p5-hot-paths-v1", mode="repeatability" if hashes["A"] == hashes["B"] else "before-after",
                  binaries={side: dict(path=str(path), sha256=hashes[side]) for side, path in binaries.items()},
                  limits=LIMITS, cases={}, passed=False)
    report_path = output / "hot-paths.json"
    report_path.write_text(json.dumps(report, indent=2))
    environment = {key: value for key, value in os.environ.items()
                   if not key.startswith(("USHER_P5_", "USHER_MEMORY_"))}
    for name, (family, task, seconds, flags) in CASES.items():
        command = [sys.executable, str(Path(__file__).with_name(f"benchmark-p5-{family}.py")),
                   "--a", str(binaries["A"]), "--b", str(binaries["B"]), "--task", task,
                   "--seconds", str(seconds), "--output", str(output / name), *flags]
        print(f"Running {name} ({report['mode']})", flush=True)
        try:
            with (output / f"{name}.log").open("w") as log:
                completed = subprocess.run(command, env=environment, stdout=log, stderr=subprocess.STDOUT, check=False)
            require(completed.returncode == 0, f"runner exited {completed.returncode}; see {name}.log")
            require(all(fingerprint(path) == hashes[side] for side, path in binaries.items()), "executable changed during benchmark")
            report["cases"][name] = validate_case(output / name, name, hashes)
        except (OSError, ValueError, KeyError, TypeError, IndexError) as error:
            report["cases"][name] = dict(passed=False, error=str(error))
        print(json.dumps({name: report["cases"][name]}), flush=True)
        report_path.write_text(json.dumps(report, indent=2))
    report["passed"] = all(report["cases"][name]["passed"] for name in CASES)
    report_path.write_text(json.dumps(report, indent=2))
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
