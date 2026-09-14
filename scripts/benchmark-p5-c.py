#!/usr/bin/env python3
"""P5-C paired Release app fixtures with the existing RSS acceptance budget."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import re
import statistics

spec = importlib.util.spec_from_file_location("p5a", Path(__file__).with_name("benchmark-p5-a.py"))
p5a = importlib.util.module_from_spec(spec)
spec.loader.exec_module(p5a)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--a", type=Path, required=True)
    parser.add_argument("--b", type=Path, required=True)
    parser.add_argument("--task", choices=["60", "61"], required=True)
    parser.add_argument("--scenario", default="code", choices=["code", "paragraphs", "find"])
    parser.add_argument("--seconds", type=float, default=40)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.seconds < 30:
        parser.error("Use at least 30 seconds")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    def seed(directory, scenario, port):
        directory.mkdir(parents=True)
        (directory / "settings.toml").write_text('theme_mode = 0\nlanguage = "en"\n[diagnostics]\nterminal_verbose_logging = false\n')
    p5a.seed = seed
    os.environ["USHER_P5_BENCHMARK"] = args.task
    os.environ["USHER_P5_PREVIEW_SCENARIO"] = args.scenario
    binaries = {"A": args.a.resolve(), "B": args.b.resolve()}
    metadata = dict(task=args.task, scenario=args.scenario, seconds=args.seconds, pairs=3, order="AB/BA/AB",
                    budget="peak/stable delta <= max(8 MiB, 5% A); B growth and growth delta <= 8 MiB",
                    binaries={s: dict(path=str(p), sha256=p5a.fingerprint(p)) for s, p in binaries.items()})
    (output / "metadata.json").write_text(json.dumps(metadata, indent=2))
    results = {"A": [], "B": []}
    for index in range(3):
        for side in ("AB" if index % 2 == 0 else "BA"):
            directory = output / f"{side}-{index + 1}"
            directory.mkdir()
            row = p5a.run(binaries[side], "p5-c", directory, args.seconds)
            log = (directory / "app.log").read_text(errors="replace")
            row["fixture_complete"] = f"[P5][{args.task}] complete" in log
            row["framebuffer_errors"] = log.count("Invalid framebuffer")
            timings = sorted(int(x) / 1000 for x in re.findall(r"\[P5\]\[" + args.task + r"\] layout_us=(\d+)", log))
            row["ui_p95_ms"] = timings[int((len(timings)-1)*.95)] if timings else None
            row["ui_max_ms"] = max(timings) if timings else None
            mounts = re.findall(r"created=(\d+)", log)
            row["created_lines"] = int(mounts[-1]) if mounts else 0
            (directory / "result.json").write_text(json.dumps(row, indent=2))
            results[side].append(row)
            print(side, index+1, json.dumps(row), flush=True)
    medians = {s: {k: statistics.median(row[k] for row in rows) for k in ["peak_mib", "settled_mib", "growth_mib"]} for s, rows in results.items()}
    a, b = medians["A"], medians["B"]
    passed = all(b[k]-a[k] <= max(8, a[k]*.05) for k in ["peak_mib", "settled_mib"])
    passed &= b["growth_mib"] <= 8 and b["growth_mib"]-a["growth_mib"] <= 8
    passed &= all(row["clean_exit"] and row["fixture_complete"] for rows in results.values() for row in rows)
    passed &= all(row["framebuffer_errors"] == 0 for row in results["B"])
    summary = dict(medians=medians, passed=passed)
    (output / "summary.json").write_text(json.dumps(summary, indent=2))
    print(json.dumps(summary, indent=2), flush=True)
    raise SystemExit(0 if passed else 1)


if __name__ == "__main__":
    main()
