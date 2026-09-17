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
