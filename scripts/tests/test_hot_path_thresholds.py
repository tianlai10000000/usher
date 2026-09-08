import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import subprocess
import sys


SPEC = importlib.util.spec_from_file_location(
    "hot_paths", Path(__file__).resolve().parents[1] / "benchmark-hot-paths.py")
POLICY = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(POLICY)
HASHES = dict(A="a" * 64, B="b" * 64)


def write_json(path, value):
    path.write_text(json.dumps(value))


def fixture(directory, name):
    family, task, seconds, _ = POLICY.CASES[name]
    metadata = dict(task=task, seconds=seconds, pairs=3, order="AB/BA/AB",
                    binaries={side: dict(sha256=value) for side, value in HASHES.items()})
    if family == "b":
        metadata.update(diagnostics=False, short_lines=name == "job",
                        project_count=128 if name == "project" else 0)
        if name == "job":
            (directory / "fixture-python").write_text("test adapter")
            metadata["fixture_adapter_sha256"] = POLICY.fingerprint(directory / "fixture-python")
    else:
        metadata["scenario"] = name.removeprefix("preview-")
    write_json(directory / "metadata.json", metadata)
    write_json(directory / "summary.json", dict(passed=True))
    for side in "AB":
        for index in range(1, 4):
            run = directory / f"{side}-{index}"
            run.mkdir()
            samples = [dict(seconds=i / 4, rss_mib=120) for i in range(seconds * 4)]
            write_json(run / "samples.json", samples)
            row = dict(peak_mib=120, settled_mib=120, growth_mib=0,
                       clean_exit=True, fixture_complete=True, dispatch_to_ui_p95_ms=5,
                       rounds=24, ui_commits=25, peak_queue=16, tail_bytes=32768,
                       cancel_ms=300, created_rows=264, ui_p95_ms=2,
                       framebuffer_errors=0, created_lines=500)
            write_json(run / "result.json", row)
            log = f"[Startup] Window revealed\n[P5][{task}] complete rows=128 updated=64\n"
            log += "output_ok=true cancel_ok=true tail=true\n[P5][60] result tail=true\n"
            for round_id in range(12):
                count = 30000 if round_id < 4 else 60000 if round_id < 8 else 7000
                log += f"[P5][61] round={round_id} count={count} valid=true latency_ms=50\n"
            (run / "app.log").write_text(log)


class HotPathThresholdTests(unittest.TestCase):
    def check_mutation(self, name, filename, mutate):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            fixture(directory, name)
            path = directory / filename
            value = json.loads(path.read_text())
            mutate(value)
            write_json(path, value)
            with self.assertRaises((ValueError, KeyError, TypeError)):
                POLICY.validate_case(directory, name, HASHES)

    def test_all_scenarios_accept_complete_evidence(self):
        for name in POLICY.CASES:
            with self.subTest(name=name), tempfile.TemporaryDirectory() as temporary:
                directory = Path(temporary)
                fixture(directory, name)
                self.assertTrue(POLICY.validate_case(directory, name, HASHES)["passed"])

    def test_missing_run_or_raw_evidence_fails(self):
        for filename in ("A-3/result.json", "B-1/samples.json", "B-2/app.log", "summary.json"):
            with self.subTest(filename=filename), tempfile.TemporaryDirectory() as temporary:
                directory = Path(temporary)
                fixture(directory, "find")
                (directory / filename).unlink()
                with self.assertRaises(OSError):
                    POLICY.validate_case(directory, "find", HASHES)

    def test_protocol_and_binary_identity_are_required(self):
        mutations = [lambda m: m.update(seconds=40), lambda m: m.update(pairs=2),
                     lambda m: m.update(scenario="code"),
                     lambda m: m["binaries"]["B"].update(sha256="wrong")]
        for mutate in mutations:
            with self.subTest(mutate=mutate):
                self.check_mutation("find", "metadata.json", mutate)

    def test_nan_infinity_boolean_missing_and_negative_metrics_fail(self):
        for value in (None, float("nan"), float("inf"), True, -1):
            with self.subTest(value=value):
                self.check_mutation("find", "B-1/result.json", lambda row: row.update(ui_p95_ms=value))

    def test_every_individual_run_must_fit_absolute_budgets(self):
        for name, key, value in [("project", "dispatch_to_ui_p95_ms", 51),
                                 ("project", "ui_commits", 65),
                                 ("job", "peak_queue", 17), ("job", "tail_bytes", 32769),
                                 ("job", "cancel_ms", 2001), ("job", "created_rows", 1025),
                                 ("preview-code", "created_lines", 2001),
                                 ("preview-paragraphs", "ui_p95_ms", 26),
                                 ("find", "framebuffer_errors", 1),
                                 ("find", "clean_exit", False), ("find", "fixture_complete", False)]:
            with self.subTest(name=name, key=key):
                self.check_mutation(name, "A-3/result.json", lambda row: row.update({key: value}))

    def test_rss_is_checked_against_raw_samples(self):
        for mutate in (lambda samples: samples[0].update(rss_mib=300),
                       lambda samples: samples.clear(),
                       lambda samples: samples[1].update(seconds=0),
                       lambda samples: samples[0].update(rss_mib=float("nan"))):
            with self.subTest(mutate=mutate):
                self.check_mutation("find", "B-1/samples.json", mutate)
        self.check_mutation("find", "B-1/result.json", lambda row: row.update(settled_mib=119))

    def test_summary_failure_cannot_be_ignored(self):
        self.check_mutation("find", "summary.json", lambda row: row.update(passed=False))

    def test_paired_regression_and_growth_are_recomputed(self):
        for growth in (False, True):
            with self.subTest(growth=growth), tempfile.TemporaryDirectory() as temporary:
                directory = Path(temporary)
                fixture(directory, "find")
                for index in range(1, 4):
                    run = directory / f"B-{index}"
                    samples = json.loads((run / "samples.json").read_text())
                    for sample in samples:
                        sample["rss_mib"] = 120 if growth and sample["seconds"] < 40 else 130
                    write_json(run / "samples.json", samples)
                    row = json.loads((run / "result.json").read_text())
                    row.update(peak_mib=130, settled_mib=130, growth_mib=10 if growth else 0)
                    write_json(run / "result.json", row)
                with self.assertRaises(ValueError):
                    POLICY.validate_case(directory, "find", HASHES)

    def test_runner_failures_leave_report_and_attempt_remaining_cases(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            binary = directory / "usher"
            binary.write_bytes(b"test executable")
            output = directory / "results"
            with patch.object(sys, "argv", ["benchmark-hot-paths.py", "--binary", str(binary), "--output", str(output)]), \
                    patch.object(POLICY.subprocess, "run", return_value=subprocess.CompletedProcess([], 7)) as run:
                self.assertEqual(POLICY.main(), 1)
            report = json.loads((output / "hot-paths.json").read_text())
            self.assertEqual(run.call_count, len(POLICY.CASES))
            self.assertFalse(report["passed"])
            self.assertEqual(report["mode"], "repeatability")
            self.assertTrue(all("exited 7" in case["error"] for case in report["cases"].values()))

    def test_false_completion_or_missing_find_round_fails(self):
        for old, new in [("valid=true", "valid=false"), ("round=11 ", "round=12 "),
                         ("count=7000", "count=6999"), ("latency_ms=50", "latency_ms=2001"),
                         ("[Startup] Window revealed", ""), ("[P5][61] complete", "")]:
            with self.subTest(old=old), tempfile.TemporaryDirectory() as temporary:
                directory = Path(temporary)
                fixture(directory, "find")
                log = directory / "B-1/app.log"
                log.write_text(log.read_text().replace(old, new))
                with self.assertRaises(ValueError):
                    POLICY.validate_case(directory, "find", HASHES)


if __name__ == "__main__":
    unittest.main()
