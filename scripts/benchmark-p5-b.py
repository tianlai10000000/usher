#!/usr/bin/env python3
"""Paired P5-B app RSS / effect fixtures; production Release binaries only."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import re
import statistics
import struct
import sys
import zlib

spec = importlib.util.spec_from_file_location("p5a", Path(__file__).with_name("benchmark-p5-a.py"))
p5a = importlib.util.module_from_spec(spec)
spec.loader.exec_module(p5a)


def png(width, height, color):
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))
    pixels = b"".join(b"\0" + bytes(color) * width for _ in range(height))
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">2I5B", width, height, 8, 6, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(pixels)) + chunk(b"IEND", b"")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--a", type=Path, required=True)
    parser.add_argument("--b", type=Path, required=True)
    parser.add_argument("--task", choices=["57", "58", "59"], required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--diagnostics", action="store_true")
    parser.add_argument("--short-lines", action="store_true", help="58: fill the retained tail with short lines via a common interpreter adapter")
    parser.add_argument("--seconds", type=float, default=30)
    args = parser.parse_args()
    if args.seconds < 24:
        parser.error("At least 24 seconds are required")
    args.output = args.output.resolve()
    args.output.mkdir(parents=True, exist_ok=False)
    count = 16 if args.diagnostics else 128

    def seed(directory, scenario, port):
        directory.mkdir(parents=True)
        (directory / "settings.toml").write_text('theme_mode = 0\nlanguage = "en"\nterminal_verbose_logging = ' + str(args.diagnostics).lower() + '\n')
        if args.task == "57":
            images = directory / "images"
            images.mkdir()
            for index in range(16):
                (images / f"{index}.png").write_bytes(png(512, 256, [index * 13, 80, 180, 255]))
            (images / "shape.svg").write_text('<svg xmlns="http://www.w3.org/2000/svg" width="128" height="128"><rect width="128" height="128" fill="#4f86ff"/><text x="10" y="70" font-size="36">项目</text></svg>')
            (directory / "project").mkdir()
            (directory / "project/projects.json").write_text(json.dumps([
                dict(project_name=f"P5 project {i}", project_alias=f"p5-{i}", local_repo="/tmp", remote_repo="", repo_type="github", repo_project_id=str(i), project_tags=["Web", "Fixture"], is_pr_auto_update=False,
                     logo=str(images / ("shape.svg" if i % 17 == 0 else f"{i % 16}.png")))
                for i in range(count)
            ]))

    p5a.seed = seed
    os.environ["USHER_P5_BENCHMARK"] = args.task
    os.environ["USHER_P5_PYTHON"] = sys.executable
    adapter = None
    if args.short_lines:
        if args.task != "58":
            parser.error("--short-lines requires task 58")
        adapter = args.output / "fixture-python"
        adapter.write_text("#!" + sys.executable + "\n" + r'''import sys
source = sys.argv[2]
if 'TAIL-' in source:
    old = r"b'x\n'*256"
    assert source.count(old) == 1
    source = source.replace(old, r"b'x x x x\n'*512")
exec(compile(source, '<p5-output-fixture>', 'exec'))
''')
        adapter.chmod(0o755)
        os.environ["USHER_P5_PYTHON"] = str(adapter)
    binaries = {"A": args.a.resolve(), "B": args.b.resolve()}
    metadata = dict(task=args.task, seconds=args.seconds, pairs=3, order="AB/BA/AB",
                    diagnostics=args.diagnostics, project_count=count if args.task == "57" else 0, short_lines=args.short_lines,
                    fixture_adapter_sha256=p5a.fingerprint(adapter) if adapter else None,
                    budget="peak/stable increase <= max(8 MiB, 5% A); B growth and growth delta <= 8 MiB",
                    binaries={side: dict(path=str(path), sha256=p5a.fingerprint(path)) for side, path in binaries.items()})
    (args.output / "metadata.json").write_text(json.dumps(metadata, indent=2))
    results = {"A": [], "B": []}
    for index in range(3):
        for side in ("AB" if index % 2 == 0 else "BA"):
            directory = args.output / f"{side}-{index + 1}"
            directory.mkdir()
            result = p5a.run(binaries[side], "p5-b", directory, args.seconds)
            log = (directory / "app.log").read_text(errors="replace")
            if args.task == "57":
                result["fixture_complete"] = f"[P5][57] complete rows={count} updated={min(count, 64)}" in log
                latencies = [int(value) / 1000 for value in re.findall(r"dispatch_to_ui_us=(\d+)", log)]
                result["rounds"] = len(latencies)
                result["dispatch_to_ui_p95_ms"] = sorted(latencies)[int((len(latencies) - 1) * .95)] if latencies else None
                result["ui_commits"] = len(re.findall(r"\[P5\]\[57\] ui_commit", log))
            else:
                result["fixture_complete"] = f"[P5][{args.task}] complete" in log
                if args.task == "59":
                    result["settings_writes"] = len(re.findall(r"\[Settings\] Saved settings to", log))
                    result["inbox_ui_updates"] = len(re.findall(r"\[P5\]\[59\] inbox_ui", log))
                    result["persisted_revisions"] = [int(value) for value in re.findall(r"persisted=Some\((\d+)\)", log)]
                if args.task == "58":
                    final = re.search(r"cancel_ms=(\d+) rows=(\d+) created=(\d+)", log)
                    if final:
                        result.update(cancel_ms=int(final[1]), rows=int(final[2]), created_rows=int(final[3]))
                    latencies = [int(value) / 1000 for value in re.findall(r"ui_commit_us=(\d+)", log)]
                    result["dispatch_to_ui_p95_ms"] = sorted(latencies)[int((len(latencies) - 1) * .95)] if latencies else None
                    queue = [int(value) for value in re.findall(r"peak_queue=(\d+)", log)]
                    result["peak_queue"] = max(queue) if queue else None
                    tails = [int(value) for value in re.findall(r"tail_bytes=(\d+)", log)]
                    result["tail_bytes"] = max(tails) if tails else None
            (directory / "result.json").write_text(json.dumps(result, indent=2))
            results[side].append(result)
            print(side, index + 1, json.dumps(result), flush=True)
    medians = {side: {key: statistics.median(row[key] for row in rows) for key in ["peak_mib", "settled_mib", "growth_mib"]} for side, rows in results.items()}
    a, b = medians["A"], medians["B"]
    passed = all(b[k] - a[k] <= max(8, a[k] * .05) for k in ["peak_mib", "settled_mib"])
    passed &= b["growth_mib"] <= 8 and b["growth_mib"] - a["growth_mib"] <= 8
    passed &= all(row["clean_exit"] and row["fixture_complete"] for rows in results.values() for row in rows)
    if args.task == "58":
        passed &= all(row.get("peak_queue") is not None and row["peak_queue"] <= 16
                      and row.get("tail_bytes") is not None and row["tail_bytes"] <= 32768
                      and row.get("cancel_ms", 99999) <= 2000 for row in results["B"])
    if args.task == "59":
        passed &= all(row.get("settings_writes", 99999) <= 8 for row in results["B"])
        passed &= all(row.get("persisted_revisions") == [501, 1002, 1503, 2004]
                      for rows in results.values() for row in rows)
    summary = dict(medians=medians, passed=passed)
    (args.output / "summary.json").write_text(json.dumps(summary, indent=2))
    print(json.dumps(summary, indent=2), flush=True)
    raise SystemExit(0 if passed else 1)


if __name__ == "__main__":
    main()
