from __future__ import annotations

import csv
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
CHECKER = REPOSITORY_ROOT / "scripts" / "check-memory-benchmark-thresholds.py"


def write_tsv(path: Path, rows: list[dict[str, object]]) -> None:
    with path.open("w", newline="", encoding="utf-8") as handle:
        writer = csv.DictWriter(handle, fieldnames=list(rows[0]), delimiter="\t")
        writer.writeheader()
        writer.writerows(rows)


def fixture(result_dir: Path, rss_peak_kib: int = 120 * 1024) -> None:
    write_tsv(
        result_dir / "metadata.tsv",
        [
            {"key": "runs", "value": "3"},
            {"key": "build_profile", "value": "release"},
            {"key": "expected_route", "value": "Home"},
            {"key": "observed_routes", "value": "Home"},
        ],
    )
    write_tsv(
        result_dir / "summary.tsv",
        [
            {
                "scenario": "home",
                "phase": "settled",
                "cycle": cycle,
                "runs": 3,
                "rss_settled_median_kib": (110 + cycle) * 1024,
                "rss_peak_max_kib": rss_peak_kib,
                "footprint_settled_median_bytes": (90 + cycle) * 1024 * 1024,
                "footprint_peak_max_bytes": 100 * 1024 * 1024,
            }
            for cycle in (1, 2)
        ],
    )
    write_tsv(
        result_dir / "run-summary.tsv",
        [
            {
                "scenario": "home",
                "run": run,
                "phase": "settled",
                "cycle": cycle,
                "rss_peak_kib": rss_peak_kib,
                "rss_settled_median_kib": (110 + cycle) * 1024,
                "footprint_peak_bytes": 100 * 1024 * 1024,
                "footprint_settled_median_bytes": (90 + cycle) * 1024 * 1024,
            }
            for run in (1, 2, 3)
            for cycle in (1, 2)
        ],
    )


class MemoryThresholdTests(unittest.TestCase):
    def test_accepts_release_results_with_three_runs_inside_every_budget(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            result_dir = Path(directory)
            fixture(result_dir)

            completed = subprocess.run(
                [sys.executable, str(CHECKER), str(result_dir)],
                check=False,
                capture_output=True,
                text=True,
            )

            self.assertEqual(completed.returncode, 0, completed.stdout + completed.stderr)
            self.assertIn("Result: **PASS**", (result_dir / "thresholds.md").read_text())

    def test_rejects_a_peak_over_the_configured_budget(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            result_dir = Path(directory)
            fixture(result_dir, rss_peak_kib=120 * 1024)

            completed = subprocess.run(
                [
                    sys.executable,
                    str(CHECKER),
                    str(result_dir),
                    "--max-rss-peak-mib",
                    "100",
                ],
                check=False,
                capture_output=True,
                text=True,
            )

            self.assertEqual(completed.returncode, 1)
            report = (result_dir / "thresholds.md").read_text()
            self.assertIn("Result: **FAIL**", report)
            self.assertIn("RSS peak 120.00 MiB exceeds 100.00 MiB", report)


if __name__ == "__main__":
    unittest.main()
