#!/usr/bin/env python3
"""On-demand P6 Release regression suite; no hosted schedule is enabled."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import signal
import subprocess
import sys

SPEC = importlib.util.spec_from_file_location("p6", Path(__file__).with_name("benchmark-p6-terminal.py"))
P6 = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(P6)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--baseline", type=Path, help="Defaults to repeatability comparison of the same binary")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--batches", type=int, choices=range(65, 71), nargs="+", default=list(range(65, 71)))
    parser.add_argument("--seconds", type=int, default=90)
    parser.add_argument("--soak-seconds", type=int, default=900)
    parser.add_argument("--soak-batches", type=int, choices=range(65, 71), nargs="+", default=[65, 69, 70])
    args = parser.parse_args()
    if args.seconds < 30 or not 900 <= args.soak_seconds <= 3600:
        parser.error("Paired runs require >=30 seconds and sustained workloads require 900..3600 seconds")
    root = args.output.resolve()
    root.mkdir(parents=True, exist_ok=False)
    binary = args.binary.resolve(strict=True)
    baseline = (args.baseline or binary).resolve(strict=True)
    metadata = dict(binary=str(binary), sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),
                    baseline=str(baseline), baseline_sha256=hashlib.sha256(baseline.read_bytes()).hexdigest(),
                    batches=args.batches, seconds=args.seconds, soak_seconds=args.soak_seconds,
                    soak_batches=args.soak_batches, platform=sys.platform,
                    protocol="3 fresh AB/BA/AB pairs per batch; one sustained process per selected fixture",
                    absolute_budgets="512 MiB RSS/footprint, 8 MiB RSS growth, 100 ms heartbeat p95",
                    soak_budgets="0.5 MiB/min RSS slope; threads +4, ports +32, regions +256 after warmup")
    (root / "metadata.json").write_text(json.dumps(metadata, indent=2))
    summary = dict(passed=False, status="running", comparisons={}, soaks={})
    def save():
        (root / "summary.json").write_text(json.dumps(summary, indent=2))
    def interrupt(signum, frame):
        raise KeyboardInterrupt
    signal.signal(signal.SIGTERM, interrupt)
    save()
    try:
        for batch in args.batches:
            command = [sys.executable, str(Path(__file__).with_name("benchmark-p6-terminal.py")),
                       "--a", str(baseline), "--b", str(binary), "--batch", str(batch),
                       "--seconds", str(args.seconds), "--mode", "regression", "--output", str(root / str(batch))]
            with (root / f"{batch}.log").open("w") as log:
                with subprocess.Popen(command, stdout=log, stderr=log, start_new_session=True) as process:
                    try:
                        code = process.wait()
                    except BaseException:
                        process.send_signal(signal.SIGINT)
                        try:
                            process.wait(timeout=15)
                        except subprocess.TimeoutExpired:
                            os.killpg(process.pid, signal.SIGKILL)
                            process.wait()
                        raise
            summary["comparisons"][str(batch)] = code == 0
            print(f"batch={batch} paired_exit={code}", flush=True)
            save()
        for batch in args.soak_batches:
            directory = root / f"soak-{batch}"
            directory.mkdir()
            try:
                row = P6.run(binary, directory, args.soak_seconds, batch, soak=True)
                if row.get("interrupted"):
                    raise KeyboardInterrupt
                errors = P6.regression_errors(row)
                summary["soaks"][str(batch)] = dict(passed=not errors, errors=errors, cycles=len(row["cycles_ms"]))
            except Exception as error:
                errors = [str(error)]
                summary["soaks"][str(batch)] = dict(passed=False, errors=errors)
            print(f"batch={batch} soak_errors={errors}", flush=True)
            save()
        summary["status"] = "complete"
        summary["passed"] = all(summary["comparisons"].values()) and all(r["passed"] for r in summary["soaks"].values())
    except KeyboardInterrupt:
        summary["status"] = "cancelled"
    except Exception as error:
        summary.update(status="failed", error=str(error))
    finally:
        save()
    return 0 if summary["passed"] else 1


if __name__ == "__main__":
    sys.exit(main())
