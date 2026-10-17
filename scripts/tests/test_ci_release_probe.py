"""Release probes use fixed Cargo arguments and bounded, non-raw diagnostics."""

import io
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))
import ci_release_build as BUILD
import ci_release_probe as PROBE

SECRET = "SYNTHETIC_RELEASE_SECRET_73828"


def probe_env(**overrides):
    value = {
        "RELEASE_PROBE_MODE": "cold", "RELEASE_DIAGNOSTICS": "false",
        "RELEASE_PROBE_ID": "", "RELEASE_CACHE_ONLY": "true",
        "RELEASE_EVENT": "workflow_dispatch", "RELEASE_REF": "refs/heads/main",
        "RELEASE_DEFAULT_BRANCH": "main", "RELEASE_RUN_ID": "12345",
        "RELEASE_RUN_ATTEMPT": "1", "RELEASE_TARGET": "aarch64-apple-darwin",
        "RELEASE_RUNNER_OS": "macOS", "RELEASE_RUNNER_ARCH": "ARM64",
        "RELEASE_RUST_KEY": "2026090148a2", "RELEASE_CONFIGURATION_HASH": "a" * 64,
        "RELEASE_REGISTRY_HASH": "b" * 64, "RELEASE_SOURCE_SHA": "c" * 40,
    }
    value.update(overrides)
    return value


class ProbePlanTests(unittest.TestCase):
    def test_normal_is_default_and_does_not_change_cache_namespace(self):
        self.assertEqual(PROBE.probe_plan({})["namespace"], "")
        self.assertFalse(PROBE.probe_plan({})["fingerprint"])
        for enabled in ("false", "true"):
            plan = PROBE.probe_plan(probe_env(RELEASE_PROBE_MODE="normal", RELEASE_DIAGNOSTICS=enabled))
            self.assertEqual(plan["namespace"], "")
            self.assertEqual(plan["build_key"], "")
            self.assertEqual(plan["registry_key"], "")
            self.assertEqual(plan["fingerprint"], enabled == "true")

    def test_cold_and_warm_share_identity_without_cargo_graph_changes(self):
        for target, host, arch in (("aarch64-apple-darwin", "macOS", "ARM64"),
                                   ("x86_64-apple-darwin", "macOS", "ARM64"),
                                   ("x86_64-pc-windows-msvc", "Windows", "X64")):
            env = probe_env(RELEASE_TARGET=target, RELEASE_RUNNER_OS=host, RELEASE_RUNNER_ARCH=arch)
            cold = PROBE.probe_plan(env)
            warm = PROBE.probe_plan({**env, "RELEASE_PROBE_MODE": "warm", "RELEASE_PROBE_ID": cold["probe_id"]})
            with self.subTest(target=target):
                for field in ("namespace", "build_key", "registry_key", "source_sha", "target"):
                    self.assertEqual(cold[field], warm[field])
                self.assertTrue(cold["fingerprint"] and warm["fingerprint"])
                self.assertTrue(cold["namespace"].startswith("release-probe-v1-"))
                self.assertNotIn("release-build-v1-", cold["build_key"])
                self.assertEqual(BUILD.cargo_command(target), ["cargo", "build", "--release", "--target", target, "--timings", "--locked"])

    def test_new_cold_attempt_has_unique_identity(self):
        first = PROBE.probe_plan(probe_env())
        for field, value in (("RELEASE_RUN_ID", "12346"), ("RELEASE_RUN_ATTEMPT", "2")):
            self.assertNotEqual(first["namespace"], PROBE.probe_plan(probe_env(**{field: value}))["namespace"])

    def test_wrong_source_platform_rust_or_configuration_cannot_be_exact(self):
        base = probe_env(RELEASE_PROBE_MODE="warm", RELEASE_PROBE_ID="12345-1")
        key = PROBE.probe_plan(base)["build_key"]
        for field, value in (("RELEASE_SOURCE_SHA", "d" * 40), ("RELEASE_CONFIGURATION_HASH", "d" * 64),
                             ("RELEASE_TARGET", "x86_64-apple-darwin"), ("RELEASE_RUST_KEY", "other-rust"),
                             ("RELEASE_RUNNER_ARCH", "X64"), ("RELEASE_RUNNER_OS", "Windows")):
            with self.subTest(field=field):
                self.assertNotEqual(key, PROBE.probe_plan({**base, field: value})["build_key"])

    def test_probe_ref_and_mode_boundaries_fail_without_echoing_values(self):
        for overrides in (
            {"RELEASE_CACHE_ONLY": "false"}, {"RELEASE_EVENT": "push"},
            {"RELEASE_REF": "refs/tags/main"}, {"RELEASE_REF": "refs/heads/Main"},
            {"RELEASE_REF": "refs/heads/topic"}, {"RELEASE_PROBE_MODE": "Cold"},
            {"RELEASE_PROBE_MODE": SECRET}, {"RELEASE_DIAGNOSTICS": SECRET},
            {"RELEASE_RUN_ID": SECRET}, {"RELEASE_TARGET": SECRET},
            {"RELEASE_PROBE_ID": "12345-1"}, {"RELEASE_SOURCE_SHA": SECRET},
        ):
            with self.subTest(overrides=tuple(overrides)):
                with self.assertRaises(PROBE.MetricsError) as error:
                    PROBE.probe_plan(probe_env(**overrides))
                self.assertNotIn(SECRET, str(error.exception))

    def test_warm_id_is_bounded_data_and_cold_does_not_accept_reused_id(self):
        for value in ("", "0-1", "1-0", "01-1", "1-01", "1-1\nnamespace=bad", SECRET,
                      "$(touch /tmp/never)", "1" * 21 + "-1", "1-" + "1" * 5):
            with self.subTest(value=value):
                with self.assertRaises(PROBE.MetricsError):
                    PROBE.probe_plan(probe_env(RELEASE_PROBE_MODE="warm", RELEASE_PROBE_ID=value))

    def test_warm_requires_both_exact_successful_restores(self):
        env = {"BUILD_CACHE_HIT": "true", "REGISTRY_CACHE_HIT": "true",
               "BUILD_CACHE_OUTCOME": "success", "REGISTRY_CACHE_OUTCOME": "success"}
        PROBE.verify_warm(env)
        for field in env:
            for value in ("", "false", "failure", "cancelled", "skipped", SECRET):
                with self.subTest(field=field, value=value):
                    with self.assertRaises(PROBE.MetricsError) as error:
                        PROBE.verify_warm({**env, field: value})
                    self.assertNotIn(SECRET, str(error.exception))

    def test_unknown_environment_is_not_exported(self):
        report = PROBE.probe_plan({**probe_env(), "GITHUB_TOKEN": SECRET, "ARBITRARY": SECRET})
        self.assertNotIn(SECRET, json.dumps(report))

    def test_helper_cache_keys_match_actual_workflow_cold_save_and_warm_restore(self):
        import test_release_workflow as contract
        text = (SCRIPTS.parent / ".github/workflows/release.yml").read_text()
        for job_name, target, host, arch in (("build-macos", "aarch64-apple-darwin", "macOS", "ARM64"),
                                              ("build-macos", "x86_64-apple-darwin", "macOS", "ARM64"),
                                              ("build-windows", "x86_64-pc-windows-msvc", "Windows", "X64")):
            env = probe_env(RELEASE_TARGET=target, RELEASE_RUNNER_OS=host, RELEASE_RUNNER_ARCH=arch)
            plan = PROBE.probe_plan(env)
            steps = contract._steps(contract._jobs(text)[job_name])
            for identity in ("registry-cache", "build-cache"):
                restore = next(step for step in steps if contract._field(step, "id", 8) == identity)
                key = contract._field(restore, "key", 10)
                key = re.sub(r"\$\{\{ steps.cache-plan.outputs.namespace \|\| '[^']+' \}\}", plan["namespace"], key)
                config = env["RELEASE_CONFIGURATION_HASH"] if identity == "build-cache" else env["RELEASE_REGISTRY_HASH"]
                key = re.sub(r"\$\{\{ hashFiles\([^)]*\) \}\}", config, key)
                for field, value in {"runner.os": host, "runner.arch": arch, "matrix.target": target,
                                     "steps.rust.outputs.cachekey": env["RELEASE_RUST_KEY"], "github.sha": env["RELEASE_SOURCE_SHA"]}.items():
                    key = key.replace("${{ " + field + " }}", value)
                self.assertNotIn("${{", key)
                self.assertEqual(key, plan[identity.removesuffix("-cache") + "_key"])


class BashGuardTests(unittest.TestCase):
    def test_actual_guards_keep_shell_looking_inputs_as_data(self):
        import test_release_workflow as contract
        steps = contract._steps(contract._jobs((SCRIPTS.parent / ".github/workflows/release.yml").read_text())["build-macos"])
        first = contract._script(steps[0])
        second = contract._script(steps[1])
        for mode, identifier, cache_only, ref, expected in (
            ("normal", "", False, "refs/heads/main", 0),
            ("cold", "", True, "refs/heads/main", 0),
            ("warm", "12345-1", True, "refs/heads/main", 0),
            ("cold", "", False, "refs/heads/main", 1),
            ("warm", "12345-1", False, "refs/heads/main", 1),
            ("cold", "12345-1", True, "refs/heads/main", 1),
            ("warm", "", True, "refs/heads/main", 1),
            ("Cold", "", True, "refs/heads/main", 1),
            ("cold", "", True, "refs/tags/main", 1),
            ("cold", "", True, "refs/heads/Main", 1),
        ):
            env = {**os.environ, "RELEASE_PROBE_MODE": mode, "RELEASE_PROBE_ID": identifier,
                   "RELEASE_CACHE_ONLY": str(cache_only).lower(), "RELEASE_DEFAULT_BRANCH": "main", "GITHUB_REF": ref}
            script = (first + '\n' if cache_only else '') + second
            result = subprocess.run(["bash", "-e", "-o", "pipefail", "-c", script], env=env, capture_output=True, text=True, timeout=10)
            with self.subTest(mode=mode, id=identifier, ref=ref):
                self.assertEqual(result.returncode, expected, result.stderr)
        with tempfile.TemporaryDirectory(prefix="release guard ") as temporary:
            marker = Path(temporary) / "must-not-exist"
            hostile = '$(touch "' + str(marker) + '")' + SECRET
            env = {**os.environ, "RELEASE_PROBE_MODE": "warm", "RELEASE_PROBE_ID": hostile,
                   "RELEASE_CACHE_ONLY": "true", "RELEASE_DEFAULT_BRANCH": hostile, "GITHUB_REF": "refs/heads/" + hostile}
            result = subprocess.run(["bash", "-e", "-o", "pipefail", "-c", first + '\n' + second], env=env, capture_output=True, text=True, timeout=10)
            self.assertEqual(result.returncode, 1)
            self.assertFalse(marker.exists())
            self.assertNotIn(SECRET, result.stdout + result.stderr)


class FingerprintTests(unittest.TestCase):
    def line(self, body, package="usher-ui"):
        return f' 0.05s INFO prepare_target{{package_id={package} v1.0.0 (/{SECRET})}}: cargo::core::compiler::fingerprint: {body}\n'

    def test_payload_paths_environment_versions_and_unknown_packages_never_export(self):
        collector = BUILD.Fingerprints()
        for value in (self.line('RerunIfEnvChanged { var: "TOKEN", val: "' + SECRET + '" }'),
                      self.line('ChangedFile { stale: "C:\\' + SECRET + '\\src.rs" }'),
                      self.line(SECRET, package=SECRET), self.line('format changed: ' + SECRET),
                      '\x1b[32mINFO\x1b[0m ' + SECRET + '\n'):
            self.assertFalse(collector.consume(value))
        self.assertFalse(collector.consume('continuation ' + SECRET + '\n'))
        result = json.dumps(collector.report())
        self.assertNotIn(SECRET, result)
        self.assertIn('environment_changed', result)
        self.assertIn('changed_file', result)
        self.assertIn('"package": "other"', result)

    def test_mtime_evidence_is_only_a_fixed_comparison(self):
        collector = BUILD.Fingerprints()
        self.assertFalse(collector.consume(self.line('dirty: ChangedFile { reference_mtime: FileTime { seconds: 100, nanos: 1 }, stale_mtime: FileTime { seconds: 101, nanos: 0 } }')))
        row = collector.report()["records"][0]
        self.assertEqual(row["mtime_comparison"], "stale_newer")
        self.assertEqual(set(row), {"package", "reason", "mtime_comparison", "count"})
        collector.consume(self.line('reference_mtime: FileTime { seconds: ' + '1' * 5000 + ', nanos: 1 }'))

    def test_pr_review_owner_retains_attribution_and_unknown_stays_other(self):
        collector = BUILD.Fingerprints()
        collector.consume(self.line('dirty: StaleDepFingerprint', package='usher-ui-pr-review'))
        collector.consume(self.line(SECRET, package='private-' + SECRET))
        self.assertEqual({row["package"] for row in collector.report()["records"]}, {"usher-ui-pr-review", "other"})
        self.assertNotIn(SECRET, json.dumps(collector.report()))

    def test_logger_format_variations_and_unknown_prologue_fail_private(self):
        for raw in (
            '2026-10-16T00:00:00Z INFO cargo::compiler::fingerprint: ' + SECRET,
            '{"level":"INFO","target":"cargo::compiler::fingerprint","value":"' + SECRET + '"}',
            '\x1b]0;private\x07INFO changed_logger: ' + SECRET,
            'unrecognized logger prologue ' + SECRET,
        ):
            with self.subTest(format=raw[:20]):
                collector = BUILD.Fingerprints()
                self.assertFalse(collector.consume(raw + '\n'))
                self.assertFalse(collector.consume(SECRET + '\n'))
                self.assertNotIn(SECRET, json.dumps(collector.report()))

    def test_record_and_trace_budgets_remain_bounded(self):
        with patch.object(BUILD, "MAX_RECORDS", 1), patch.object(BUILD, "MAX_TRACE_BYTES", 1024):
            collector = BUILD.Fingerprints()
            for body in ("ChangedFile", "StaleDepFingerprint", SECRET * 100, SECRET):
                self.assertFalse(collector.consume(self.line(body)))
            report = collector.report()
            self.assertTrue(report["truncated"])
            self.assertEqual(len(report["records"]), 1)
            self.assertLessEqual(report["trace_bytes_examined"], 1024)
            self.assertNotIn(SECRET, json.dumps(report))

    def test_ordinary_cargo_error_and_diagnostic_continuations_survive(self):
        collector = BUILD.Fingerprints()
        collector.consume(self.line('fingerprint dirty for usher-ui v1.0.0'))
        for line in ('   Compiling usher-ui v1.0.0\n', 'error[E0001]: ordinary failure\n',
                     ' --> src/lib.rs:1:1\n', '  | ordinary source\n'):
            self.assertTrue(collector.consume(line))


class BuildExecutionTests(unittest.TestCase):
    def execute(self, raw, code=0, output_blocked=False):
        class Child:
            def __init__(self):
                self.stderr = io.BytesIO(raw)

            def wait(self):
                return code

        calls = []
        def popen(command, **options):
            calls.append((command, options))
            return Child()
        with tempfile.TemporaryDirectory(prefix="release wrapper ") as temporary:
            root = Path(temporary)
            output = root / "fingerprints.json"
            if output_blocked:
                output.mkdir()
            stderr = io.StringIO()
            command = BUILD.cargo_command("x86_64-pc-windows-msvc")
            result = BUILD.run_build(command, output, {"CARGO_INCREMENTAL": "0", "RUSTFLAGS": "existing", "TOKEN": SECRET}, stderr, popen)
            report = json.loads(output.read_text()) if output.is_file() else None
            return result, stderr.getvalue(), report, calls

    def test_no_shell_fixed_args_and_only_logging_environment_is_changed(self):
        result, _, report, calls = self.execute(b'    Finished `release` profile in 0s\n')
        self.assertEqual(result, 0)
        self.assertEqual(len(calls), 1)
        self.assertNotIn("shell", calls[0][1])
        self.assertEqual(calls[0][1]["stdout"], subprocess.DEVNULL)
        self.assertEqual(calls[0][1]["env"], {"CARGO_INCREMENTAL": "0", "RUSTFLAGS": "existing", "TOKEN": SECRET, "CARGO_LOG": BUILD.FINGERPRINT_LOG})
        self.assertNotIn(SECRET, json.dumps(report))
        self.assertEqual(BUILD.cargo_command("aarch64-apple-darwin", 8), ["cargo", "build", "--release", "--target", "aarch64-apple-darwin", "--jobs", "8", "--timings", "--locked"])

    def test_cargo_failure_and_signal_status_are_preserved_even_if_export_fails(self):
        for code, expected in ((0, 0), (1, 1), (101, 101), (-15, 143)):
            for blocked in (False, True):
                with self.subTest(code=code, blocked=blocked):
                    result, stderr, report, calls = self.execute(b'error: ordinary compiler failure\n', code, blocked)
                    self.assertEqual(result, expected)
                    self.assertEqual(len(calls), 1)
                    self.assertIn("Cargo compiler error", stderr)
                    self.assertNotIn(SECRET, stderr)
                    if report:
                        self.assertEqual(report["cargo_exit_code"], code)

    def test_parser_exception_is_private_and_does_not_change_cargo_status(self):
        for code in (0, 101):
            with patch.object(BUILD.Fingerprints, "consume", side_effect=ValueError(SECRET)):
                result, stderr, report, calls = self.execute(('INFO fingerprint: ' + SECRET + '\n' + SECRET).encode(), code)
            self.assertEqual(result, code)
            self.assertEqual(len(calls), 1)
            self.assertTrue(report["collector_error"])
            self.assertNotIn(SECRET, stderr + json.dumps(report))

    def test_oversized_and_multiline_trace_is_not_printed_or_written(self):
        raw = ('INFO cargo::core::compiler::fingerprint: ' + SECRET + '\n' + SECRET + '\n'
               + 'INFO ' + SECRET * 2000 + '\n' + '    Finished `release` profile in 0s\n').encode()
        result, stderr, report, _ = self.execute(raw)
        self.assertEqual(result, 0)
        self.assertNotIn(SECRET, stderr + json.dumps(report))
        self.assertTrue(report["truncated"])
        self.assertEqual(report["oversized_lines"], 1)
        self.assertIn("Cargo finished", stderr)

    def test_unknown_trace_after_progress_and_cargo_shaped_payload_never_replay(self):
        for raw in (
            '   Compiling usher v1.0.0\nunrecognized logger prologue ' + SECRET + '\n',
            'INFO fingerprint: value\nwarning: ' + SECRET + '\nerror: ' + SECRET + '\n',
            'Compiling ' + SECRET + ' v1.0.0\nFinished ' + SECRET + '\n',
            'error[E0425]: cannot find value INFO in this scope ' + SECRET + '\n',
        ):
            result, stderr, report, _ = self.execute(raw.encode(), 101)
            self.assertEqual(result, 101)
            self.assertIn("Cargo exit status: 101", stderr)
            self.assertNotIn(SECRET, stderr + json.dumps(report))

    def test_spawn_failure_is_private_and_does_not_claim_build_success(self):
        stderr = io.StringIO()
        def fail(*args, **kwargs):
            raise OSError(SECRET)
        result = BUILD.run_build(["cargo"], Path("unused"), {}, stderr, fail)
        self.assertEqual(result, 127)
        self.assertNotIn(SECRET, stderr.getvalue())

    def test_real_child_both_streams_are_private_and_status_remains_authoritative(self):
        for code in (0, 101):
            with tempfile.TemporaryDirectory(prefix="release streams ") as temporary:
                output = Path(temporary) / "fingerprints.json"
                stderr = io.StringIO()
                program = ('import sys; print(' + repr(SECRET) + '); '
                           'print("   Compiling usher v1.0.0",file=sys.stderr); '
                           'print("unknown logger " + ' + repr(SECRET) + ',file=sys.stderr); '
                           'print("warning: " + ' + repr(SECRET) + ',file=sys.stderr); '
                           'sys.exit(' + str(code) + ')')
                result = BUILD.run_build([sys.executable, "-c", program], output, stderr=stderr)
                self.assertEqual(result, code)
                self.assertIn("Cargo exit status: " + str(code), stderr.getvalue())
                self.assertNotIn(SECRET, stderr.getvalue() + output.read_text())

    def test_invalid_target_or_jobs_cannot_add_shell_or_profile_flags(self):
        for target in (SECRET, "x86_64-pc-windows-msvc --features ui-components", "$(touch /tmp/never)"):
            with self.assertRaises(ValueError):
                BUILD.cargo_command(target)
        for jobs in (0, 513, -1, True, "8"):
            with self.assertRaises(ValueError):
                BUILD.cargo_command("aarch64-apple-darwin", jobs)


class CliTests(unittest.TestCase):
    def test_probe_plan_cli_outputs_only_validated_fields_and_private_errors(self):
        with tempfile.TemporaryDirectory(prefix="release probe ") as temporary:
            root = Path(temporary)
            output, step = root / "probe.json", root / "step-output"
            env = {**os.environ, **probe_env(), "GITHUB_OUTPUT": str(step), "TOKEN": SECRET}
            result = subprocess.run([sys.executable, str(SCRIPTS / "ci_release_probe.py"), "plan", "--output", str(output)], env=env, capture_output=True, text=True, timeout=10)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("namespace=release-probe-v1-12345-1", step.read_text())
            self.assertNotIn(SECRET, output.read_text() + step.read_text() + result.stdout + result.stderr)
            env["RELEASE_PROBE_ID"] = SECRET
            result = subprocess.run([sys.executable, str(SCRIPTS / "ci_release_probe.py"), "plan", "--output", str(output)], env=env, capture_output=True, text=True, timeout=10)
            self.assertEqual(result.returncode, 2)
            self.assertNotIn(SECRET, result.stdout + result.stderr)

    def test_invalid_cli_values_never_echo_untrusted_input_or_spawn_cargo(self):
        for script, arguments in (("ci_release_build.py", ["--target", SECRET, "--output", "unused"]),
                                  ("ci_release_build.py", ["--target", "aarch64-apple-darwin", "--jobs", SECRET, "--output", "unused"]),
                                  ("ci_release_probe.py", [SECRET])):
            result = subprocess.run([sys.executable, str(SCRIPTS / script), *arguments], capture_output=True, text=True, timeout=10)
            self.assertEqual(result.returncode, 2)
            self.assertNotIn(SECRET, result.stdout + result.stderr)
            self.assertNotIn("Traceback", result.stderr)


if __name__ == "__main__":
    unittest.main()
