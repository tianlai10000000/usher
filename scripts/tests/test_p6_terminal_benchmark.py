"""The Terminal performance gate must reject incomplete or regressed evidence."""
import importlib.util
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location(
    "p6_terminal", Path(__file__).resolve().parents[1] / "benchmark-p6-terminal.py")
BENCHMARK = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BENCHMARK)


def results():
    return {side: [dict(complete=True, clean_exit=True, framebuffer_errors=0,
                       peak_mib=100, lifetime_peak_mib=100, settled_mib=90, growth_mib=0, ui_p95_ms=52,
                       footprint_peak_mib=120, footprint_final_mib=100,
                       cycles_ms=[300, 301, 302])
                   for _ in range(3)] for side in "AB"}


class TerminalBudgetTests(unittest.TestCase):
    def test_equal_complete_evidence_passes(self):
        self.assertTrue(BENCHMARK.summarize(results())["passed"])

    def test_missing_samples_and_failed_runs_never_pass(self):
        for key, value in [("complete", False), ("clean_exit", False),
                           ("framebuffer_errors", 1), ("footprint_peak_mib", None),
                           ("cycles_ms", []), ("cycles_ms", [300, 0, 300])]:
            with self.subTest(key=key):
                rows = results()
                rows["B"][0][key] = value
                self.assertFalse(BENCHMARK.summarize(rows)["passed"])
        rows = results()
        rows["A"].pop()
        self.assertFalse(BENCHMARK.summarize(rows)["passed"])

    def test_rss_footprint_growth_and_responsiveness_are_independent_gates(self):
        for key in ["peak_mib", "lifetime_peak_mib", "settled_mib", "footprint_peak_mib",
                    "footprint_final_mib", "growth_mib", "ui_p95_ms"]:
            with self.subTest(key=key):
                rows = results()
                for row in rows["B"]:
                    row[key] += 9
                self.assertFalse(BENCHMARK.summarize(rows)["passed"])

    def test_throughput_regression_fails_even_with_good_memory_and_heartbeat(self):
        rows = results()
        for row in rows["B"]:
            row["cycles_ms"] = [500, 600, 700]
        self.assertFalse(BENCHMARK.summarize(rows)["passed"])


if __name__ == "__main__":
    unittest.main()
