"""Release metrics must preserve timing evidence without exporting raw API data."""

import importlib.util
import json
import os
from collections.abc import Mapping
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "ci_release_metrics.py"
SPEC = importlib.util.spec_from_file_location("ci_release_metrics", SCRIPT)
METRICS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(METRICS)

SECRET = "SYNTHETIC_SECRET_MARKER_67c26e"
CACHE_ENV = (
    "RELEASE_TARGET", "CACHE_IDENTITY",
    "REGISTRY_CACHE_HIT", "REGISTRY_CACHE_MATCHED_KEY", "REGISTRY_CACHE_OUTCOME",
    "BUILD_CACHE_HIT", "BUILD_CACHE_MATCHED_KEY", "BUILD_CACHE_OUTCOME",
)
JOB_FIELDS = {
    "id", "name", "run_attempt", "status", "conclusion", "started_at",
    "completed_at", "duration_seconds", "steps",
}
STEP_FIELDS = {
    "id", "number", "name", "status", "conclusion", "started_at",
    "completed_at", "duration_seconds",
}


def cache_env(**overrides):
    env = dict.fromkeys(CACHE_ENV, "")
    env.update(RELEASE_TARGET="aarch64-apple-darwin", CACHE_IDENTITY="release-v1")
    env.update(overrides)
    return env


def step(**overrides):
    value = {
        "number": 1, "name": "Build Release", "status": "completed",
        "conclusion": "success", "started_at": "2026-10-16T00:00:05Z",
        "completed_at": "2026-10-16T00:00:15Z",
    }
    value.update(overrides)
    return value


def job(job_id=101, **overrides):
    value = {
        "id": job_id, "name": "Build macOS App (arm64)", "run_attempt": 2,
        "status": "completed", "conclusion": "success",
        "started_at": "2026-10-16T00:00:00Z",
        "completed_at": "2026-10-16T00:00:20Z", "steps": [step()],
    }
    value.update(overrides)
    return value


def payload(*jobs):
    return {"jobs": list(jobs)}


class CacheMetadataTests(unittest.TestCase):
    def test_cache_reads_exactly_eight_named_fields_without_scanning_environment(self):
        class NamedEnvironment(Mapping):
            def __init__(self):
                self.read = []

            def get(self, key, default=None):
                self.read.append(key)
                return cache_env().get(key, default)

            def __getitem__(self, key):
                raise AssertionError("environment must be read through named get calls")

            def __iter__(self):
                raise AssertionError("environment must not be iterated")

            def __len__(self):
                raise AssertionError("environment must not be scanned")

            def items(self):
                raise AssertionError("environment items must not be scanned")

            def keys(self):
                raise AssertionError("environment keys must not be scanned")

        env = NamedEnvironment()
        METRICS.cache_metadata(env)
        self.assertEqual(set(env.read), set(CACHE_ENV))
        self.assertEqual(len(env.read), len(CACHE_ENV))

    def test_explicit_cache_metadata_and_unknown_environment_are_allowlisted(self):
        env = cache_env(REGISTRY_CACHE_HIT="true", REGISTRY_CACHE_OUTCOME="success",
                        REGISTRY_CACHE_MATCHED_KEY="registry-key",
                        BUILD_CACHE_HIT="false", BUILD_CACHE_OUTCOME="success")
        env.update(GITHUB_TOKEN=SECRET, ARBITRARY_ENV={"nested": SECRET})
        report = METRICS.cache_metadata(env)
        self.assertEqual(set(report), {
            "schema_version", "release_target", "cache_identity", "registry", "build",
        })
        self.assertEqual(report["release_target"], env["RELEASE_TARGET"])
        self.assertEqual(report["cache_identity"], env["CACHE_IDENTITY"])
        for name in ("registry", "build"):
            self.assertEqual(set(report[name]), {
                "cache_hit", "matched_key", "outcome", "result",
            })
        self.assertIs(report["registry"]["cache_hit"], True)
        self.assertIs(report["build"]["cache_hit"], False)
        self.assertEqual(report["registry"]["result"], "exact")
        self.assertEqual(report["build"]["result"], "miss")
        self.assertNotIn(SECRET, json.dumps(report))

    def test_empty_cache_outputs_remain_unknown(self):
        report = METRICS.cache_metadata({})
        for name in ("registry", "build"):
            self.assertIsNone(report[name]["cache_hit"])
            self.assertIsNone(report[name]["matched_key"])
            self.assertIsNone(report[name]["outcome"])
            self.assertEqual(report[name]["result"], "unknown")

    def test_false_string_is_not_a_truthy_exact_hit(self):
        for prefix in ("REGISTRY", "BUILD"):
            with self.subTest(cache=prefix):
                report = METRICS.cache_metadata(cache_env(**{f"{prefix}_CACHE_HIT": "false"}))
                self.assertIs(report[prefix.lower()]["cache_hit"], False)
                self.assertEqual(report[prefix.lower()]["result"], "miss")

    def test_matched_key_proves_fallback_without_an_exact_hit(self):
        for hit in ("false", ""):
            with self.subTest(hit=hit):
                report = METRICS.cache_metadata(cache_env(
                    BUILD_CACHE_HIT=hit, BUILD_CACHE_MATCHED_KEY="older-target-key"))
                self.assertEqual(report["build"]["matched_key"], "older-target-key")
                self.assertEqual(report["build"]["result"], "fallback")

    def test_success_without_hit_or_matched_key_is_a_miss(self):
        report = METRICS.cache_metadata(cache_env(BUILD_CACHE_OUTCOME="success"))
        self.assertIsNone(report["build"]["cache_hit"])
        self.assertEqual(report["build"]["result"], "miss")

    def test_failed_cancelled_or_skipped_actions_override_stale_hit_outputs(self):
        for outcome, result in (("failure", "failure"), ("cancelled", "cancelled"),
                                ("skipped", "not_run")):
            for hit in ("true", "false", ""):
                with self.subTest(outcome=outcome, hit=hit):
                    report = METRICS.cache_metadata(cache_env(
                        BUILD_CACHE_OUTCOME=outcome, BUILD_CACHE_HIT=hit,
                        BUILD_CACHE_MATCHED_KEY="stale-key"))
                    self.assertEqual(report["build"]["result"], result)

    def test_cache_booleans_are_strict_and_errors_do_not_echo_values(self):
        for value in ("True", "TRUE", "1", " true ", "false\n", SECRET):
            for prefix in ("REGISTRY", "BUILD"):
                with self.subTest(value=value, cache=prefix):
                    with self.assertRaises(METRICS.MetricsError) as caught:
                        METRICS.cache_metadata(cache_env(**{f"{prefix}_CACHE_HIT": value}))
                    self.assertNotIn(SECRET, str(caught.exception))

    def test_cache_outcomes_are_strict_and_errors_do_not_echo_values(self):
        for value in ("Success", "unknown", "timed_out", " success ", SECRET):
            with self.subTest(value=value):
                with self.assertRaises(METRICS.MetricsError) as caught:
                    METRICS.cache_metadata(cache_env(BUILD_CACHE_OUTCOME=value))
                self.assertNotIn(SECRET, str(caught.exception))


class JobsMetadataTests(unittest.TestCase):
    def test_single_page_preserves_job_and_step_timings_without_inventing_step_id(self):
        report = METRICS.jobs_metadata(payload(job()))
        self.assertEqual(set(report), {"schema_version", "run_attempt", "attempt_selection", "jobs"})
        self.assertEqual(report["run_attempt"], 2)
        projected = report["jobs"][0]
        self.assertEqual(set(projected), JOB_FIELDS)
        self.assertEqual(projected["duration_seconds"], 20)
        self.assertEqual(set(projected["steps"][0]), STEP_FIELDS)
        self.assertEqual(projected["steps"][0]["duration_seconds"], 10)
        self.assertIsNone(projected["steps"][0]["id"])
        identified = METRICS.jobs_metadata(payload(job(steps=[step(id=456)])))
        self.assertEqual(identified["jobs"][0]["steps"][0]["id"], 456)

    def test_slurped_pages_preserve_distinct_same_name_matrix_jobs(self):
        report = METRICS.jobs_metadata([
            {"total_count": 2, "jobs": [job(101)]},
            {"total_count": 2, "jobs": [job(102)]},
        ], attempt=2)
        self.assertEqual({row["id"] for row in report["jobs"]}, {101, 102})
        self.assertEqual(len(report["jobs"]), 2)

    def test_duplicate_job_ids_deduplicate_after_ignoring_unknown_fields(self):
        first = job(html_url="https://example.invalid/first")
        second = job(html_url=SECRET, arbitrary={"private": SECRET})
        report = METRICS.jobs_metadata([
            {"total_count": 1, "jobs": [first]},
            {"total_count": 1, "jobs": [second]},
        ])
        self.assertEqual(len(report["jobs"]), 1)
        self.assertNotIn(SECRET, json.dumps(report))

    def test_conflicting_duplicate_job_ids_are_rejected(self):
        changes = ({"name": "other"}, {"conclusion": "failure"},
                   {"completed_at": "2026-10-16T00:00:21Z"},
                   {"steps": [step(name="Different step")]})
        for change in changes:
            with self.subTest(change=change):
                with self.assertRaises(METRICS.MetricsError):
                    METRICS.jobs_metadata([payload(job()), payload(job(**change))])

    def test_total_count_rejects_missing_pages_and_counts_duplicate_ids_once(self):
        for pages in (
            [{"total_count": 2, "jobs": [job()]}],
            [{"total_count": 2, "jobs": [job()]}, {"total_count": 2, "jobs": [job()]}],
            [{"total_count": 0, "jobs": [job()]}],
        ):
            with self.subTest(pages=pages):
                with self.assertRaises(METRICS.MetricsError):
                    METRICS.jobs_metadata(pages)

    def test_page_total_counts_must_agree(self):
        with self.assertRaises(METRICS.MetricsError):
            METRICS.jobs_metadata([
                {"total_count": 1, "jobs": [job()]},
                {"total_count": 2, "jobs": [job(102)]},
            ])
        for total in (True, -1, "1", 1.0):
            with self.subTest(total=total):
                with self.assertRaises(METRICS.MetricsError):
                    METRICS.jobs_metadata({"total_count": total, "jobs": [job()]})

    def test_duplicate_step_identity_deduplicates_or_rejects_conflicts(self):
        for identity in ({"number": 1}, {"id": 456, "number": None}):
            first = step(**identity)
            second = step(**identity, private=SECRET)
            report = METRICS.jobs_metadata(payload(job(steps=[first, second])))
            self.assertEqual(len(report["jobs"][0]["steps"]), 1)
            self.assertNotIn(SECRET, json.dumps(report))
            second["completed_at"] = "2026-10-16T00:00:16Z"
            with self.subTest(identity=identity):
                with self.assertRaises(METRICS.MetricsError):
                    METRICS.jobs_metadata(payload(job(steps=[first, second])))

    def test_invalid_page_shapes_are_rejected_without_echoing_unknown_payload(self):
        for bad in (None, SECRET, {"message": SECRET}, {"jobs": SECRET},
                    {"jobs": [SECRET]}, [payload(job()), {"jobs": None, "private": SECRET}]):
            with self.subTest(shape=type(bad).__name__):
                with self.assertRaises(METRICS.MetricsError) as caught:
                    METRICS.jobs_metadata(bad)
                self.assertNotIn(SECRET, str(caught.exception))

    def test_explicit_attempt_is_preserved_and_mismatched_reported_attempt_is_rejected(self):
        report = METRICS.jobs_metadata(payload(job()), attempt=2)
        self.assertEqual(report["run_attempt"], 2)
        self.assertTrue(report["attempt_selection"])
        with self.assertRaises(METRICS.MetricsError):
            METRICS.jobs_metadata(payload(job(run_attempt=1)), attempt=2)

    def test_attempt_argument_requires_a_positive_integer(self):
        for attempt in (0, -1, True, False, 2.0, "2", SECRET):
            with self.subTest(attempt=attempt):
                with self.assertRaises(METRICS.MetricsError) as caught:
                    METRICS.jobs_metadata(payload(job()), attempt=attempt)
                self.assertNotIn(SECRET, str(caught.exception))

    def test_default_attempt_selects_latest_reported_attempt(self):
        report = METRICS.jobs_metadata(payload(job(101, run_attempt=1), job(102, run_attempt=2)))
        self.assertEqual(report["run_attempt"], 2)
        self.assertEqual([row["id"] for row in report["jobs"]], [102])

    def test_mixing_reported_and_missing_attempts_is_rejected(self):
        no_attempt = job(102)
        no_attempt.pop("run_attempt")
        for attempt in (None, 2):
            with self.subTest(attempt=attempt):
                with self.assertRaises(METRICS.MetricsError):
                    METRICS.jobs_metadata(payload(job(), no_attempt), attempt=attempt)

    def test_endpoint_scoped_jobs_keep_attempt_absence_explicit(self):
        no_attempt = job()
        no_attempt.pop("run_attempt")
        for attempt in (None, 3):
            with self.subTest(attempt=attempt):
                report = METRICS.jobs_metadata(payload(no_attempt), attempt=attempt)
                self.assertEqual(report["run_attempt"], attempt)
                self.assertIsNone(report["jobs"][0]["run_attempt"])
                self.assertIn("endpoint_scope", report["attempt_selection"])

    def test_unknown_fields_are_removed_at_every_payload_level(self):
        source = {
            "total_count": 1, "private": SECRET,
            "jobs": [job(runner_name=SECRET, labels=[SECRET],
                         steps=[step(private={"token": SECRET}, outputs=[SECRET])])],
        }
        report = METRICS.jobs_metadata(source)
        self.assertNotIn(SECRET, json.dumps(report))
        self.assertNotIn(SECRET, METRICS.render_summary(report))
        self.assertEqual(set(report["jobs"][0]), JOB_FIELDS)
        self.assertEqual(set(report["jobs"][0]["steps"][0]), STEP_FIELDS)

    def test_missing_null_and_empty_times_are_unknown_not_zero(self):
        for missing in ("absent", None, ""):
            for field in ("started_at", "completed_at"):
                row = job(steps=[step()])
                for owner in (row, row["steps"][0]):
                    if missing == "absent":
                        owner.pop(field)
                    else:
                        owner[field] = missing
                with self.subTest(missing=missing, field=field):
                    projected = METRICS.jobs_metadata(payload(row))["jobs"][0]
                    self.assertIsNone(projected[field])
                    self.assertIsNone(projected["duration_seconds"])
                    self.assertIsNone(projected["steps"][0]["duration_seconds"])
        empty = METRICS.jobs_metadata(payload())
        self.assertEqual(empty["jobs"], [])

    def test_timezone_offsets_and_fractional_seconds_measure_real_elapsed_time(self):
        row = job(started_at="2026-10-16T08:00:00.125+08:00",
                  completed_at="2026-10-16T00:00:01.625Z")
        report = METRICS.jobs_metadata(payload(row))
        self.assertAlmostEqual(report["jobs"][0]["duration_seconds"], 1.5)

    def test_github_zero_date_is_unknown_at_either_or_both_interval_ends(self):
        for zero in ("0001-01-01T00:00:00Z", "0001-01-01T00:00:00.000+00:00"):
            for fields in (("started_at",), ("completed_at",), ("started_at", "completed_at")):
                row = job(status="queued", conclusion="skipped", steps=[step(conclusion="skipped")])
                for owner in (row, row["steps"][0]):
                    for field in fields:
                        owner[field] = zero
                with self.subTest(zero=zero, fields=fields):
                    projected = METRICS.jobs_metadata(payload(row))["jobs"][0]
                    self.assertEqual(projected["status"], "queued")
                    self.assertEqual(projected["conclusion"], "skipped")
                    self.assertIsNone(projected["duration_seconds"])
                    self.assertIsNone(projected["steps"][0]["duration_seconds"])
                    for field in fields:
                        self.assertIsNone(projected[field])
                        self.assertIsNone(projected["steps"][0][field])

    def test_timestamps_require_valid_timezone_aware_iso8601(self):
        for bad in ("2026-10-16T00:00:00", "2026-10-16", "2026-99-16T00:00:00Z",
                    "2026-10-16T00:00:00+00:60", "2026-10-16T00:00:00+24:00",
                    "not-a-date", 123, SECRET):
            for in_step in (False, True):
                row = job()
                owner = row["steps"][0] if in_step else row
                owner["started_at"] = bad
                with self.subTest(bad=bad, in_step=in_step):
                    with self.assertRaises(METRICS.MetricsError) as caught:
                        METRICS.jobs_metadata(payload(row))
                    self.assertNotIn(SECRET, str(caught.exception))

    def test_job_and_step_end_before_start_are_rejected(self):
        for in_step in (False, True):
            row = job()
            owner = row["steps"][0] if in_step else row
            owner["completed_at"] = "2026-10-15T23:59:59Z"
            with self.subTest(in_step=in_step):
                with self.assertRaises(METRICS.MetricsError):
                    METRICS.jobs_metadata(payload(row))

    def test_cancelled_negative_interval_retains_timestamps_and_reports_unknown(self):
        for in_step in (False, True):
            row = job()
            owner = row["steps"][0] if in_step else row
            owner.update(conclusion="cancelled", started_at="2026-10-16T00:00:05Z",
                         completed_at="2026-10-16T00:00:04Z")
            with self.subTest(in_step=in_step):
                report = METRICS.jobs_metadata(payload(row))
                projected = report["jobs"][0]["steps"][0] if in_step else report["jobs"][0]
                self.assertIsNone(projected["duration_seconds"])
                self.assertEqual(projected["conclusion"], "cancelled")
                self.assertEqual(projected["started_at"], owner["started_at"])
                self.assertEqual(projected["completed_at"], owner["completed_at"])
                self.assertEqual(projected["timing_anomaly"], "cancelled_end_precedes_start")
                self.assertIn("Timing anomaly", METRICS.render_summary(report))

    def test_non_cancelled_reverse_intervals_remain_invalid(self):
        for conclusion in ("success", "failure", "skipped", None):
            for in_step in (False, True):
                row = job()
                owner = row["steps"][0] if in_step else row
                owner.update(conclusion=conclusion, completed_at="2026-10-15T23:59:59Z")
                with self.subTest(conclusion=conclusion, in_step=in_step):
                    with self.assertRaises(METRICS.MetricsError):
                        METRICS.jobs_metadata(payload(row))

    def test_failure_durations_are_retained_and_skipped_durations_are_unknown(self):
        for conclusion in ("failure", "cancelled"):
            failed = job(conclusion=conclusion, steps=[step(conclusion=conclusion)])
            with self.subTest(conclusion=conclusion):
                projected = METRICS.jobs_metadata(payload(failed))["jobs"][0]
                self.assertEqual(projected["conclusion"], conclusion)
                self.assertEqual(projected["duration_seconds"], 20)
                self.assertEqual(projected["steps"][0]["duration_seconds"], 10)
        skipped = job(102, conclusion="skipped", started_at=None, completed_at=None,
                      steps=[step(conclusion="skipped", started_at=None, completed_at=None)])
        projected = METRICS.jobs_metadata(payload(skipped))["jobs"][0]
        self.assertEqual(projected["conclusion"], "skipped")
        self.assertIsNone(projected["duration_seconds"])
        self.assertIsNone(projected["steps"][0]["duration_seconds"])

    def test_in_progress_is_preserved_and_known_zero_differs_from_missing_time(self):
        running = job(status="in_progress", conclusion=None, completed_at=None,
                      steps=[step(status="in_progress", conclusion=None, completed_at=None)])
        projected = METRICS.jobs_metadata(payload(running))["jobs"][0]
        self.assertEqual(projected["status"], "in_progress")
        self.assertIsNone(projected["conclusion"])
        self.assertIsNone(projected["duration_seconds"])
        self.assertIsNone(projected["steps"][0]["duration_seconds"])
        zero = job(completed_at="2026-10-16T00:00:00Z")
        self.assertEqual(METRICS.jobs_metadata(payload(zero))["jobs"][0]["duration_seconds"], 0)

    def test_only_exact_report_job_name_is_excluded_after_pagination_validation(self):
        report = METRICS.jobs_metadata({"total_count": 3, "jobs": [
            job(101, name="Release timing report"),
            job(102, name="Release timing report (other)"), job(103),
        ]})
        self.assertEqual({row["id"] for row in report["jobs"]}, {102, 103})


class SummaryTests(unittest.TestCase):
    def test_untrusted_names_and_statuses_cannot_inject_markdown_or_html(self):
        hostile = "Build | INJECTED_ROW | [run](https://example.invalid) <script>x</script>\n## injected"
        row = job(name=hostile, status="<img src=x>", conclusion="[link](https://example.invalid)",
                  steps=[step(name=hostile, status="<svg onload=x>")])
        summary = METRICS.render_summary(METRICS.jobs_metadata(payload(row)))
        for active in (hostile, "| INJECTED_ROW |", "<script>", "<img src=x>",
                       "<svg onload=x>", "[run](https://example.invalid)",
                       "[link](https://example.invalid)", "\n## injected"):
            with self.subTest(active=active):
                self.assertNotIn(active, summary)

    def test_summary_exposes_unknown_times_parallelism_and_cache_evidence_limits(self):
        report = METRICS.jobs_metadata(payload(job(started_at=None)))
        summary = METRICS.render_summary(report).lower()
        self.assertIn("unknown", summary)
        self.assertIn("parallel", summary)
        self.assertIn("cache.json", summary)
        self.assertIn("cargo", summary)
        self.assertIn("html", summary)
        self.assertIsNone(report["jobs"][0]["duration_seconds"])


class CliTests(unittest.TestCase):
    def run_cli(self, *arguments, env=None):
        child_env = os.environ.copy()
        for key in CACHE_ENV:
            child_env.pop(key, None)
        child_env.update(cache_env())
        child_env["GITHUB_TOKEN"] = SECRET
        if env:
            child_env.update(env)
        return subprocess.run(
            [sys.executable, str(SCRIPT), *map(str, arguments)],
            text=True, capture_output=True, env=child_env, timeout=15, check=False,
        )

    def assert_private_failure(self, result):
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertNotIn(SECRET, result.stdout)
        self.assertNotIn(SECRET, result.stderr)
        self.assertNotIn("Traceback", result.stderr)

    def test_cache_cli_writes_only_explicit_metadata(self):
        with tempfile.TemporaryDirectory(prefix="release metrics ") as temporary:
            output = Path(temporary) / "cache.json"
            result = self.run_cli("cache", "--output", output,
                                  env={"BUILD_CACHE_HIT": "true", "BUILD_CACHE_OUTCOME": "success"})
            self.assertEqual(result.returncode, 0, result.stderr)
            report = json.loads(output.read_text())
            self.assertEqual(report["build"]["result"], "exact")
            self.assertNotIn(SECRET, output.read_text() + result.stdout + result.stderr)

    def test_jobs_cli_writes_json_and_markdown_from_slurped_pages(self):
        with tempfile.TemporaryDirectory(prefix="release metrics ") as temporary:
            root = Path(temporary)
            source = root / "API pages.json"
            source.write_text(json.dumps([{"total_count": 1, "jobs": [job()]}]))
            output = root / "metrics output"
            result = self.run_cli("jobs", "--input", source, "--output-dir", output, "--attempt", 2)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual({path.name for path in output.iterdir()}, {"timings.json", "summary.md"})
            self.assertEqual(json.loads((output / "timings.json").read_text())["run_attempt"], 2)
            self.assertIn("Build Release", (output / "summary.md").read_text())

    def test_jobs_cli_reads_and_writes_utf8_independent_of_locale(self):
        with tempfile.TemporaryDirectory(prefix="release metrics ") as temporary:
            root = Path(temporary)
            source = root / "input.json"
            fixture = payload(job(name="构建 Windows", steps=[step(name="打包完成")]))
            source.write_bytes(json.dumps(fixture, ensure_ascii=False).encode("utf-8"))
            output = root / "out"
            result = self.run_cli("jobs", "--input", source, "--output-dir", output,
                                  env={"LC_ALL": "C", "PYTHONUTF8": "0", "PYTHONCOERCECLOCALE": "0"})
            self.assertEqual(result.returncode, 0, result.stderr)
            report = json.loads((output / "timings.json").read_bytes().decode("utf-8"))
            self.assertEqual(report["jobs"][0]["name"], "构建 Windows")
            self.assertIn("打包完成", (output / "summary.md").read_bytes().decode("utf-8"))

    def test_cancelled_negative_step_does_not_discard_other_job_timings(self):
        with tempfile.TemporaryDirectory(prefix="release cancellation ") as temporary:
            root = Path(temporary)
            source = root / "input.json"
            source.write_text(json.dumps(payload(
                job(), job(102, conclusion="cancelled", steps=[step(
                    conclusion="cancelled", completed_at="2026-10-16T00:00:04Z",
                    private=SECRET)]))))
            output = root / "out"
            result = self.run_cli("jobs", "--input", source, "--output-dir", output)
            self.assertEqual(result.returncode, 0, result.stderr)
            text = (output / "timings.json").read_text()
            rows = json.loads(text)["jobs"]
            self.assertEqual(rows[0]["duration_seconds"], 20)
            self.assertIsNone(rows[1]["steps"][0]["duration_seconds"])
            self.assertEqual(rows[1]["steps"][0]["timing_anomaly"], "cancelled_end_precedes_start")
            self.assertIn("Timing anomaly", (output / "summary.md").read_text())
            self.assertNotIn(SECRET, text + result.stdout + result.stderr)

    def test_invalid_json_and_invalid_schema_do_not_echo_payload(self):
        for label, contents in (
            ("invalid-json", '{"private":"' + SECRET + '", broken}'),
            ("invalid-schema", json.dumps({"jobs": SECRET, "private": SECRET})),
        ):
            with self.subTest(contents_type=label):
                with tempfile.TemporaryDirectory() as temporary:
                    root = Path(temporary)
                    source = root / "input.json"
                    source.write_text(contents)
                    result = self.run_cli("jobs", "--input", source, "--output-dir", root / "out")
                    self.assert_private_failure(result)

    def test_io_failures_and_bad_attempt_return_two_without_traceback(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            missing = self.run_cli("jobs", "--input", root / "missing.json",
                                   "--output-dir", root / "out")
            self.assert_private_failure(missing)
            blocker = root / "file"
            blocker.write_text("ordinary fixture")
            self.assert_private_failure(self.run_cli("cache", "--output", blocker / "cache.json"))
            source = root / "input.json"
            source.write_text(json.dumps(payload(job())))
            self.assert_private_failure(self.run_cli(
                "jobs", "--input", source, "--output-dir", root / "out", "--attempt", 0))

    def test_invalid_cache_env_diagnostic_does_not_echo_value(self):
        with tempfile.TemporaryDirectory() as temporary:
            result = self.run_cli("cache", "--output", Path(temporary) / "cache.json",
                                  env={"BUILD_CACHE_HIT": SECRET})
            self.assert_private_failure(result)

    def test_invalid_cli_arguments_do_not_echo_untrusted_values(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for arguments in (
                [SECRET],
                ["cache", "--output", root / "cache.json", "--unknown", SECRET],
                ["jobs", "--input", root / "input.json", "--output-dir", root / "out",
                 "--attempt", SECRET],
            ):
                with self.subTest(command=arguments[0]):
                    self.assert_private_failure(self.run_cli(*arguments))


if __name__ == "__main__":
    unittest.main()
