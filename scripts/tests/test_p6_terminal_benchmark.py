"""The Terminal performance gate must reject incomplete or regressed evidence."""
import importlib.util
from pathlib import Path
import unittest
import copy
import json
import sys
import tempfile
from unittest.mock import patch, MagicMock

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


def regression_results(batch=69, n=3, soak=False):
    rows = results()
    for side in rows.values():
        for row in side:
            row.update(batch=batch, cycles_ms=[300] * n, cycle_ids=list(range(n)),
                       shutdown_ms=100, soak=soak, requested_seconds=900,
                       progress=[(i + 1, (i + 1) * 30000) for i in range(n)],
                       rss_slope_mib_min=0, footprint_slope_mib_min=0,
                       resources={k: [20] * 20 for k in ("threads", "mach_ports", "regions")},
                       output_writes=[0] * n, shared_snapshots=[100] * (2 * n),
                       prepared=[10] * n, replay_successes=n, replay_failures=0,
                       terminal_replay_cycles=[True] * n,
                       retained_layouts=[dict(cycle=c, source=s, source_bytes=100, measurement_bytes=0,
                                             wrapped_bytes=50) for c in range(n) for s in range(16)])
    return rows


class RegressionModeTests(unittest.TestCase):
    def test_suite_retains_failure_and_continues_independent_cases(self):
        spec = importlib.util.spec_from_file_location(
            "p6_suite", Path(__file__).resolve().parents[1] / "benchmark-p6-regression.py")
        suite = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(suite)
        with tempfile.TemporaryDirectory() as temporary:
            binary = Path(temporary) / "binary"
            binary.write_bytes(b"fixture")
            output = Path(temporary) / "results"
            process = MagicMock()
            process.__enter__.return_value = process
            process.wait.return_value = 7
            row = regression_results(70, n=30, soak=True)["A"][0]
            with patch.object(sys, "argv", ["suite", "--binary", str(binary), "--output", str(output),
                                           "--batches", "65", "69", "--soak-batches", "70"]), \
                    patch.object(suite.subprocess, "Popen", return_value=process) as popen, \
                    patch.object(suite.P6, "run", return_value=row), \
                    patch.object(suite.signal, "signal"):
                self.assertEqual(suite.main(), 1)
            summary = json.loads((output / "summary.json").read_text())
            self.assertEqual(popen.call_count, 2)
            self.assertFalse(summary["passed"])
            self.assertTrue(summary["soaks"]["70"]["passed"])
            self.assertEqual(summary["status"], "complete")

    def test_current_current_passes_without_legacy_costs(self):
        for batch in (69, 70):
            rows = regression_results(batch)
            self.assertFalse(BENCHMARK.summarize(rows)["passed"])
            self.assertTrue(BENCHMARK.summarize(rows, "regression")["passed"])

    def test_live_hook_replay_need_not_contain_a_future_terminal_event(self):
        rows = regression_results(69)
        for side in rows.values():
            for row in side:
                row["replay_failures"] = 3
        self.assertTrue(BENCHMARK.summarize(rows, "regression")["passed"])
        rows["B"][0]["terminal_replay_cycles"][-1] = False
        self.assertFalse(BENCHMARK.summarize(rows, "regression")["passed"])

    def test_rejects_fault_on_either_side(self):
        mutations = [("cycle_ids", [0, 1, 1]), ("output_writes", [0, 1, 0]),
                     ("terminal_replay_cycles", [True, False, True]),
                     ("complete", False), ("shutdown_ms", 9000), ("shared_snapshots", [0] * 6)]
        for side in "AB":
            for key, value in mutations:
                rows = regression_results()
                rows[side][0][key] = value
                with self.subTest(side=side, key=key):
                    self.assertFalse(BENCHMARK.summarize(rows, "regression")["passed"])

    def test_sustained_activity_resources_and_slope(self):
        row = regression_results(n=30, soak=True)["A"][0]
        self.assertEqual(BENCHMARK.regression_errors(row), [])
        mutations = [("progress", [(i + 1, (i + 1) * 1000) for i in range(30)]),
                     ("progress", []), ("rss_slope_mib_min", .6),
                     ("resources", {"threads": [20] * 10 + [30] * 10}),
                     ("growth_mib", 9)]
        for key, value in mutations:
            bad = copy.deepcopy(row)
            bad[key] = value
            self.assertTrue(BENCHMARK.regression_errors(bad), key)
        stalled = copy.deepcopy(row)
        stalled["progress"][10] = (11, 410000)
        self.assertTrue(BENCHMARK.regression_errors(stalled))

    def test_slope_uses_elapsed_minutes(self):
        self.assertAlmostEqual(BENCHMARK.linear_slope([(0, 5), (1, 5.5), (2, 6)]), .5)
        self.assertIsNone(BENCHMARK.linear_slope([]))


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

    def test_stale_highlight_loading_is_a_candidate_failure(self):
        rows = results()
        rows["A"][0]["premature_loading_clears"] = 5
        self.assertTrue(BENCHMARK.summarize(rows)["passed"])
        rows["B"][0]["premature_loading_clears"] = 1
        self.assertFalse(BENCHMARK.summarize(rows)["passed"])

    def test_response_budget_requires_rejection_and_successful_followups(self):
        rows = results()
        for side in "AB":
            for row in rows[side]:
                row.update(batch=67, small_responses=6, oversized_rejections=3 if side == "B" else 0)
        self.assertTrue(BENCHMARK.summarize(rows)["passed"])
        rows["B"][0]["oversized_rejections"] = 2
        self.assertFalse(BENCHMARK.summarize(rows)["passed"])
        rows["B"][0]["oversized_rejections"] = 3
        rows["B"][1]["small_responses"] = 5
        self.assertFalse(BENCHMARK.summarize(rows)["passed"])

    def test_hook_journal_gate_requires_bounded_files_and_terminal_replay(self):
        rows = results()
        for side in "AB":
            for row in rows[side]:
                row.update(batch=68, journal_bytes=[2_000_000] * 3,
                           replay_successes=3, replay_failures=0, terminal_replay_cycles=[True] * 3)
        self.assertTrue(BENCHMARK.summarize(rows)["passed"])
        rows["B"][0]["journal_bytes"][0] = 3_000_000
        self.assertFalse(BENCHMARK.summarize(rows)["passed"])
        rows["B"][0]["journal_bytes"][0] = 2_000_000
        rows["B"][1]["replay_failures"] = 1
        self.assertFalse(BENCHMARK.summarize(rows)["passed"])

    def test_hook_state_gate_requires_no_output_writes_and_shared_configuration(self):
        rows = results()
        for side in "AB":
            for row in rows[side]:
                row.update(batch=69, output_writes=[0 if side == "B" else 10] * 3,
                           shared_snapshots=[100 if side == "B" else 0] * 6,
                           replay_successes=3, prepared=[10] * 3, terminal_replay_cycles=[True] * 3)
        self.assertTrue(BENCHMARK.summarize(rows)["passed"])
        rows["B"][0]["output_writes"][0] = 1
        self.assertFalse(BENCHMARK.summarize(rows)["passed"])
        rows["B"][0]["output_writes"][0] = 0
        rows["B"][1]["shared_snapshots"][0] = 99
        self.assertFalse(BENCHMARK.summarize(rows)["passed"])

    def test_duplicate_terminal_replays_cannot_hide_a_later_failed_cycle(self):
        log = ("[P6][69] replay_applied terminal=true\n" * 3
               + "[P6][69] cycle=0 completed_ms=100\n"
               + "[P6][69] replay_applied terminal=false\n"
               + "[P6][69] cycle=1 completed_ms=100\n"
               + "[P6][69] cycle=2 completed_ms=100\n")
        self.assertEqual(BENCHMARK.terminal_replay_cycles(log, 69), [True, False, False])

    def test_job_retention_requires_every_source_in_every_cycle(self):
        rows = results()
        for side in "AB":
            for row in rows[side]:
                row.update(batch=70, retained_layouts=[dict(cycle=c, source=s, source_bytes=100,
                           measurement_bytes=0 if side == "B" else 100, wrapped_bytes=50)
                           for c in range(3) for s in range(16)])
        self.assertTrue(BENCHMARK.summarize(rows)["passed"])
        rows["B"][0]["retained_layouts"][0]["measurement_bytes"] = 1
        self.assertFalse(BENCHMARK.summarize(rows)["passed"])
        rows["B"][0]["retained_layouts"][0]["measurement_bytes"] = 0
        rows["B"][0]["retained_layouts"][-1]["source"] = 14
        self.assertFalse(BENCHMARK.summarize(rows)["passed"])
        rows["B"][0]["retained_layouts"].pop()
        self.assertFalse(BENCHMARK.summarize(rows)["passed"])


if __name__ == "__main__":
    unittest.main()
