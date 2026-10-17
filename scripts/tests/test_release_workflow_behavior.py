"""Finite, local release-policy model; never executes a workflow or shell command.

The inputs are the actual workflow fields, not a hand-written replacement policy.
Only this repository's condition grammar, success/continue-on-error propagation,
and cache prefix/ref visibility scenarios are modeled. This is not the Actions
expression engine, cache service, Cargo fingerprint checker, or a timing test.
Unknown expressions fail closed. actionlint and hosted validation are separate.
"""

import re
import unittest

import test_release_workflow as contract

ROOT = contract.ROOT
BASELINE = ROOT / "scripts/tests/fixtures/release-ci-reliability/baseline-release.yml"
WORKFLOW = ROOT / ".github/workflows/release.yml"


class Expression:
    """Small allowlisted parser; no eval, imports, subprocesses or YAML execution."""

    TOKEN = re.compile(r"\s*('(?:[^']*)'|&&|\|\||==|!=|[!(),]|[A-Za-z_][A-Za-z0-9_.-]*)")

    def __init__(self, text, context, success):
        self.tokens = []
        pos = 0
        while pos < len(text):
            match = self.TOKEN.match(text, pos)
            if not match:
                raise ValueError("unsupported condition token")
            self.tokens.append(match[1])
            pos = match.end()
        self.index = 0
        self.context = context
        self.success = success

    def take(self, token=None):
        value = self.tokens[self.index] if self.index < len(self.tokens) else None
        if token is not None and value != token:
            raise ValueError("unsupported condition grammar")
        self.index += 1
        return value

    def peek(self):
        return self.tokens[self.index] if self.index < len(self.tokens) else None

    def parse(self):
        result = self.logical_or()
        if self.index != len(self.tokens):
            raise ValueError("unconsumed condition")
        return bool(result)

    def logical_or(self):
        value = self.logical_and()
        while self.peek() == "||":
            self.take()
            right = self.logical_and()
            value = bool(value) or bool(right)
        return value

    def logical_and(self):
        value = self.compare()
        while self.peek() == "&&":
            self.take()
            right = self.compare()
            value = bool(value) and bool(right)
        return value

    def compare(self):
        value = self.atom()
        if self.peek() in ("==", "!="):
            operation = self.take()
            right = self.atom()
            # GitHub string equality is case-insensitive. The ref safety check
            # deliberately lives in quoted Bash, modeled separately below.
            if isinstance(value, str) and isinstance(right, str):
                value, right = value.casefold(), right.casefold()
            value = (value == right) if operation == "==" else (value != right)
        return value

    def atom(self):
        token = self.take()
        if token == "!":
            return not self.atom()
        if token == "(":
            result = self.logical_or()
            self.take(")")
            return result
        if token and token.startswith("'"):
            return token[1:-1]
        if token in ("true", "false"):
            return token == "true"
        if self.peek() == "(":
            self.take()
            arguments = []
            if self.peek() != ")":
                arguments.append(self.logical_or())
                while self.peek() == ",":
                    self.take()
                    arguments.append(self.logical_or())
            self.take(")")
            if token == "always" and not arguments:
                return True
            if token == "success" and not arguments:
                return self.success
            if token == "failure" and not arguments:
                return not self.success
            if token == "startsWith" and len(arguments) == 2:
                return arguments[0].casefold().startswith(arguments[1].casefold())
            if token == "format" and len(arguments) == 2 and arguments[0] == "refs/heads/{0}":
                return "refs/heads/" + arguments[1]
            raise ValueError("unsupported condition function")
        if token not in self.context:
            raise ValueError("unsupported condition context")
        return self.context[token]


def enabled(expression, context, success):
    expression = contract._expression(expression)
    if not expression:
        return success
    explicit_status = re.search(r"\b(?:always|success|failure|cancelled)\(", expression)
    return (success or bool(explicit_status)) and Expression(expression, context, success).parse()


def default_cache_only(text):
    trigger = contract._section(contract._clean(text), "on", 0)
    inputs = contract._section(contract._section(trigger, "workflow_dispatch", 2), "inputs", 4)
    value = contract._field(contract._section(inputs, "cache_only", 6), "default", 8)
    if value not in (None, "false", "true"):
        raise ValueError("unsupported cache_only default")
    return value == "true"


def context_for(text, event="push", ref="refs/tags/v1.0.0", cache_only=None,
                cache_probe="normal", cache_diagnostics=False, probe_id=""):
    return {
        "github.event_name": event,
        "github.ref": ref,
        "github.event.repository.default_branch": "main",
        "inputs.cache_only": default_cache_only(text) if cache_only is None else cache_only,
        "inputs.cache_probe": cache_probe,
        "inputs.cache_diagnostics": cache_diagnostics,
        "inputs.probe_id": probe_id,
        "vars.MACOS_SIGNING_NOTARIZATION_ENABLED": "true",
    }


def simulate_build(text, job_name, context, failures=(), cache_hit="false", registry_cache_hit=None):
    steps = contract._steps(contract._jobs(contract._clean(text))[job_name])
    values = dict(context)
    for identity in ("registry-cache", "build-cache"):
        values["steps." + identity + ".outputs.cache-hit"] = cache_hit if identity == "build-cache" or registry_cache_hit is None else registry_cache_hit
    success = True
    records = []
    for index, step in enumerate(steps):
        name = contract._field(step, "name", 8) or contract._field(step, "id", 8) or "step-" + str(index)
        identity = contract._field(step, "id", 8)
        if not enabled(contract._field(step, "if", 8), values, success):
            outcome = conclusion = "skipped"
        else:
            # Recognized quoted Bash [[ != ]] comparison is case-sensitive.
            invalid_prewarm_ref = name == "Validate cache prewarm ref" and values["github.ref"] != "refs/heads/" + values["github.event.repository.default_branch"]
            mode, probe_id = values["inputs.cache_probe"], values["inputs.probe_id"]
            valid_inputs = ((mode == "normal" and not probe_id)
                            or (mode == "cold" and values["inputs.cache_only"] and not probe_id)
                            or (mode == "warm" and values["inputs.cache_only"] and bool(re.fullmatch(r"[1-9][0-9]{0,19}-[1-9][0-9]{0,3}", probe_id))))
            invalid_probe = name == "Validate release probe inputs" and not valid_inputs
            invalid_warm = name == "Verify warm cache exact" and any(
                values.get("steps." + identity + ".outputs.cache-hit") != "true"
                or values.get("steps." + identity + ".outcome") != "success"
                for identity in ("registry-cache", "build-cache"))
            failed = name in failures or invalid_prewarm_ref or invalid_probe or invalid_warm
            outcome = "failure" if failed else "success"
            tolerance = contract._field(step, "continue-on-error", 8)
            tolerated = enabled(tolerance, values, True) if tolerance is not None else False
            conclusion = "success" if tolerated else outcome
            if conclusion == "failure":
                success = False
        if identity:
            values["steps." + identity + ".outcome"] = outcome
        records.append({"name": name, "outcome": outcome, "conclusion": conclusion})
    return {"result": "success" if success else "failure", "steps": records}


def simulate(text, *, event="push", ref="refs/tags/v1.0.0", cache_only=None,
             failures=None, report_failure=False, publish_failure=False, cache_hit="false",
             registry_cache_hit=None, cache_probe="normal", cache_diagnostics=False, probe_id=""):
    context = context_for(text, event, ref, cache_only, cache_probe, cache_diagnostics, probe_id)
    jobs = contract._jobs(contract._clean(text))
    builds = {name: simulate_build(text, name, context, (failures or {}).get(name, ()), cache_hit, registry_cache_hit)
              for name in contract.BUILD_JOBS}
    build_success = all(build["result"] == "success" for build in builds.values())
    release_runs = enabled(contract._field(jobs["release"], "if", 4), context, build_success)
    release_result = "failure" if release_runs and publish_failure else "success" if release_runs else "skipped"
    report_runs = enabled(contract._field(jobs["report-timings"], "if", 4), context,
                          build_success and release_result != "failure")
    report_tolerated = contract._field(jobs["report-timings"], "continue-on-error", 4) == "true"
    workflow_success = build_success and release_result != "failure" and not (report_runs and report_failure and not report_tolerated)
    return {"builds": builds, "release": release_result, "report_runs": report_runs,
            "report_failure_tolerated": report_tolerated, "workflow_success": workflow_success}


def outcomes(result, job, names):
    return {step["name"]: step["outcome"] for step in result["builds"][job]["steps"] if step["name"] in names}


def cache_identity(text, job="build-macos", *, configuration="old-config", sha="old-sha", **boundaries):
    restore = next(step for step in contract._steps(contract._jobs(contract._clean(text))[job])
                   if contract._field(step, "id", 8) == "build-cache")
    values = {"runner.os": "macOS", "runner.arch": "ARM64", "matrix.target": "aarch64-apple-darwin",
              "steps.rust.outputs.cachekey": "rust-1.98.1", "github.sha": sha, **boundaries}

    def render(template):
        template = re.sub(r"\$\{\{\s*hashFiles\([^)]*\)\s*\}\}", configuration, template)
        for name, value in values.items():
            template = re.sub(r"\$\{\{\s*" + re.escape(name) + r"\s*\}\}", lambda _: value, template)
        if "${{" in template:
            raise ValueError("unsupported cache identity")
        return template

    return render(contract._normal_key(contract._field(restore, "key", 10))), [render(value) for value in contract._normal_restore_prefixes(restore)]


def cache_match(request, entry_key, *, current_ref="refs/tags/v2", entry_ref="refs/heads/main", default_ref="refs/heads/main"):
    """Fixture visibility: current ref or default branch only, never sibling tags."""
    if entry_ref not in (current_ref, default_ref):
        return "miss"
    key, prefixes = request
    if key == entry_key:
        return "exact"
    for index, prefix in enumerate(prefixes):
        if entry_key.startswith(prefix):
            return "fallback-" + str(index + 1)
    return "miss"


def compare(before, after):
    """Public, deterministic allowlist evidence; no original logs or environment."""
    cases = []

    def add(name, baseline, candidate, expected):
        cases.append({"name": name, "baseline": baseline, "candidate": candidate, "expectation": expected})

    for job in contract.BUILD_JOBS:
        for diagnostic in ("Install Python for diagnostics", "Record cache identity", "Upload build diagnostics"):
            left = simulate(before, failures={job: {diagnostic}})
            right = simulate(after, failures={job: {diagnostic}})
            add(job + "/diagnostic-failure/" + diagnostic,
                {"workflow_success": left["workflow_success"], "release": left["release"]},
                {"workflow_success": right["workflow_success"], "release": right["release"]},
                "baseline failure; candidate release remains successful")
        critical = ["Build Release", "Upload Artifacts"]
        if job == "build-macos":
            critical.append("Sign macOS executable and app bundle")
        for step in critical:
            left = simulate(before, failures={job: {step}})
            right = simulate(after, failures={job: {step}})
            add(job + "/critical-failure/" + step,
                {"workflow_success": left["workflow_success"], "release": left["release"]},
                {"workflow_success": right["workflow_success"], "release": right["release"]},
                "both fail and release is skipped")
    left, right = simulate(before, report_failure=True), simulate(after, report_failure=True)
    add("timing-report-failure", {"workflow_success": left["workflow_success"], "release": left["release"]},
        {"workflow_success": right["workflow_success"], "release": right["release"]},
        "baseline workflow fails after release; candidate workflow succeeds")
    for label, event, ref, flag in (("manual-default-prewarm", "workflow_dispatch", "refs/heads/main", True),
                                   ("manual-nondefault-rejected", "workflow_dispatch", "refs/heads/topic", True),
                                   ("manual-default-false", "workflow_dispatch", "refs/heads/main", None),
                                   ("tag-release-control", "push", "refs/tags/v2", None)):
        arms = []
        for text in (before, after):
            result = simulate(text, event=event, ref=ref, cache_only=flag)
            arms.append({"workflow_success": result["workflow_success"], "release": result["release"],
                         "builds": {job: outcomes(result, job, {"Build Release", *contract.PRODUCT_STEPS[job]})
                                    for job in contract.BUILD_JOBS}})
        add(label, *arms, "candidate follows optional-default-false/default-branch-only build-only policy; ordinary release remains enabled")
    for changed in ("package-version", "dev-profile", "workflow", "lockfile", "release-config-hash"):
        arms = []
        for text in (before, after):
            key, _ = cache_identity(text)
            arms.append(cache_match(cache_identity(text, configuration="changed-" + changed, sha="new-sha"), key))
        add("cache-config/" + changed, *arms, "baseline miss; candidate compatible coarse fallback; Cargo must revalidate")
    for boundary, changed in (("runner.os", "Windows"), ("runner.arch", "X64"),
                              ("matrix.target", "x86_64-apple-darwin"), ("steps.rust.outputs.cachekey", "rust-other")):
        arms = []
        for text in (before, after):
            key, _ = cache_identity(text, **{boundary: changed})
            arms.append(cache_match(cache_identity(text, configuration="new", sha="new"), key))
        add("cache-boundary/" + boundary, *arms, "both miss across platform/host/target/toolchain boundaries")
    for label in ("release-fast-default", "release-ui-components"):
        arms = []
        for text in (before, after):
            key, _ = cache_identity(text)
            arms.append(cache_match(cache_identity(text), key.replace("release-default", label)))
        add("cache-boundary/" + label, *arms, "both miss across explicit profile/feature namespace labels")
    for source_ref in ("refs/tags/v1", "refs/heads/main"):
        arms = []
        for text in (before, after):
            key, _ = cache_identity(text)
            arms.append(cache_match(cache_identity(text, sha="new"), key, current_ref="refs/tags/v2", entry_ref=source_ref))
        add("cache-ref/" + source_ref, *arms, "sibling tag remains invisible; default branch compatible prefix is visible")
    return cases


class BehaviorTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.before = BASELINE.read_text()
        cls.after = WORKFLOW.read_text()

    def test_before_after_diagnostics_and_critical_controls(self):
        for case in compare(self.before, self.after):
            with self.subTest(case=case["name"]):
                if "/diagnostic-failure/" in case["name"]:
                    self.assertEqual(case["baseline"], {"workflow_success": False, "release": "skipped"})
                    self.assertEqual(case["candidate"], {"workflow_success": True, "release": "success"})
                elif "/critical-failure/" in case["name"]:
                    self.assertEqual(case["baseline"], {"workflow_success": False, "release": "skipped"})
                    self.assertEqual(case["candidate"], case["baseline"])
                elif case["name"] == "timing-report-failure":
                    self.assertEqual(case["baseline"], {"workflow_success": False, "release": "success"})
                    self.assertEqual(case["candidate"], {"workflow_success": True, "release": "success"})
                elif case["name"].startswith("cache-config/"):
                    self.assertEqual((case["baseline"], case["candidate"]), ("miss", "fallback-2"))
                elif case["name"].startswith("cache-boundary/"):
                    self.assertEqual((case["baseline"], case["candidate"]), ("miss", "miss"))
                elif case["name"] == "cache-ref/refs/tags/v1":
                    self.assertEqual((case["baseline"], case["candidate"]), ("miss", "miss"))
                elif case["name"] == "cache-ref/refs/heads/main":
                    self.assertEqual((case["baseline"], case["candidate"]), ("fallback-1", "fallback-1"))

    def test_cache_only_default_false_and_legitimate_tag_release(self):
        self.assertFalse(default_cache_only(self.after))
        result = simulate(self.after)
        self.assertTrue(result["workflow_success"])
        self.assertEqual(result["release"], "success")
        for job in contract.BUILD_JOBS:
            self.assertTrue(all(value == "success" for value in outcomes(result, job, contract.PRODUCT_STEPS[job]).values()))

    def test_same_matrix_runner_toolchain_and_bundle_production_contract(self):
        before_jobs = contract._jobs(contract._clean(self.before))
        after_jobs = contract._jobs(contract._clean(self.after))
        for job in contract.BUILD_JOBS:
            self.assertEqual(contract._field(before_jobs[job], "runs-on", 4), contract._field(after_jobs[job], "runs-on", 4))
            self.assertEqual(contract._section(before_jobs[job], "strategy", 4), contract._section(after_jobs[job], "strategy", 4))
            for name in ("Build Release", "Create App Bundle"):
                left = next((step for step in contract._steps(before_jobs[job]) if contract._field(step, "name", 8) == name), None)
                right = next((step for step in contract._steps(after_jobs[job]) if contract._field(step, "name", 8) == name), None)
                if left is not None:
                    original, current = contract._field(left, "run", 8), contract._field(right, "run", 8)
                    if name == "Build Release":
                        self.assertEqual(contract._script(right), contract._build_script(job))
                        direct = next(line.strip() for line in contract._build_script(job).splitlines() if line.strip().startswith("cargo build"))
                        direct = direct.replace('"${RELEASE_TARGET}"', '${{ matrix.target }}').replace(" --locked", "")
                        self.assertEqual(direct, original)
                    else:
                        self.assertEqual(current, original)
            rust_before = next(step for step in contract._steps(before_jobs[job]) if contract._field(step, "id", 8) == "rust")
            rust_after = next(step for step in contract._steps(after_jobs[job]) if contract._field(step, "id", 8) == "rust")
            self.assertEqual(rust_before, rust_after)

    def test_manual_false_keeps_build_and_packaging_without_release(self):
        result = simulate(self.after, event="workflow_dispatch", ref="refs/heads/main")
        self.assertEqual(result["release"], "skipped")
        for job in contract.BUILD_JOBS:
            self.assertEqual(outcomes(result, job, {"Build Release"})["Build Release"], "success")
            self.assertTrue(all(value == "success" for name, value in outcomes(result, job, contract.PRODUCT_STEPS[job]).items()
                                if "signing" not in name and name not in {"Sign macOS executable and app bundle", "Notarize and staple DMG"}))

    def test_prewarm_builds_saves_but_skips_every_product_step(self):
        result = simulate(self.after, event="workflow_dispatch", ref="refs/heads/main", cache_only=True)
        self.assertTrue(result["workflow_success"])
        self.assertEqual(result["release"], "skipped")
        for job in contract.BUILD_JOBS:
            names = contract.PRODUCT_STEPS[job]
            values = outcomes(result, job, names)
            self.assertEqual(set(values), names)
            self.assertEqual(set(values.values()), {"skipped"})
            self.assertEqual(outcomes(result, job, {"Build Release", "Save Cargo registry", "Save Release build"}),
                             dict.fromkeys(("Build Release", "Save Cargo registry", "Save Release build"), "success"))

    def test_prewarm_other_branch_and_tag_rejected_before_checkout(self):
        for ref in ("refs/heads/topic", "refs/heads/Main", "refs/tags/main", "refs/tags/v2"):
            result = simulate(self.after, event="workflow_dispatch", ref=ref, cache_only=True)
            self.assertFalse(result["workflow_success"])
            for job in contract.BUILD_JOBS:
                records = result["builds"][job]["steps"]
                self.assertEqual(records[0]["outcome"], "failure")
                self.assertEqual(outcomes(result, job, {"Checkout", "Build Release", "Save Release build"}),
                                 dict.fromkeys(("Checkout", "Build Release", "Save Release build"), "skipped"))

    def test_cache_hit_does_not_skip_cargo_or_enable_failed_build_save(self):
        result = simulate(self.after, cache_hit="true")
        for job in contract.BUILD_JOBS:
            self.assertEqual(outcomes(result, job, {"Build Release"})["Build Release"], "success")
            failed = simulate(self.after, failures={job: {"Build Release"}})
            self.assertEqual(set(outcomes(failed, job, {"Save Cargo registry", "Save Release build"}).values()), {"skipped"})

    def test_report_does_not_turn_build_or_publish_failure_green(self):
        self.assertFalse(simulate(self.after, failures={"build-windows": {"Build Release"}}, report_failure=True)["workflow_success"])
        self.assertFalse(simulate(self.after, publish_failure=True, report_failure=True)["workflow_success"])

    def test_strict_cache_priority_and_exact_control(self):
        key, prefixes = cache_identity(self.after)
        self.assertEqual(cache_match((key, prefixes), key), "exact")
        self.assertEqual(cache_match(cache_identity(self.after, sha="other"), key), "fallback-1")

    def test_coarse_cache_preserves_explicit_boundaries(self):
        for boundary, changed in (("runner.os", "Windows"), ("runner.arch", "X64"),
                                  ("matrix.target", "x86_64-apple-darwin"), ("steps.rust.outputs.cachekey", "rust-other")):
            key, _ = cache_identity(self.after, **{boundary: changed})
            self.assertEqual(cache_match(cache_identity(self.after, configuration="new", sha="new"), key), "miss")
        key, _ = cache_identity(self.after)
        for label in ("release-fast-default", "release-ui-components"):
            self.assertEqual(cache_match(cache_identity(self.after), key.replace("release-default", label)), "miss")

    def test_sibling_tag_cache_is_invisible_but_default_prewarm_is_visible(self):
        key, _ = cache_identity(self.after)
        request = cache_identity(self.after, configuration="new", sha="new")
        self.assertEqual(cache_match(request, key, current_ref="refs/tags/v2", entry_ref="refs/tags/v1"), "miss")
        self.assertEqual(cache_match(request, key, current_ref="refs/tags/v2", entry_ref="refs/heads/main"), "fallback-2")
        self.assertEqual(cache_match(request, key, current_ref="refs/tags/v1", entry_ref="refs/tags/v1"), "fallback-2")

    def test_model_rejects_unrecognized_expression_instead_of_assuming_success(self):
        for expression in ("contains(github.ref,'tag')", "unknown.context", "true + true", "success() garbage"):
            with self.assertRaises(ValueError):
                enabled(expression, context_for(self.after), True)

    def test_case_insensitive_actions_expression_is_not_the_ref_security_boundary(self):
        context = context_for(self.after, ref="refs/heads/Main")
        self.assertTrue(enabled("github.ref == 'refs/heads/main'", context, True))
        result = simulate(self.after, event="workflow_dispatch", ref="refs/heads/Main", cache_only=True)
        self.assertFalse(result["workflow_success"])

    def test_implicit_success_and_continued_failure_outcome(self):
        context = context_for(self.after)
        self.assertFalse(enabled("!inputs.cache_only", context, False))
        self.assertTrue(enabled("always() && !inputs.cache_only", context, False))
        result = simulate(self.after, failures={"build-macos": {"Record cache identity"}})
        record = next(step for step in result["builds"]["build-macos"]["steps"] if step["name"] == "Record cache identity")
        self.assertEqual((record["outcome"], record["conclusion"]), ("failure", "success"))

    def test_behavior_negative_removed_diagnostic_tolerance_blocks_release(self):
        changed = self.after.replace("      - name: Record cache identity\n        if: always()\n        continue-on-error: true", "      - name: Record cache identity\n        if: always()", 1)
        self.assertNotEqual(changed, self.after)
        self.assertFalse(simulate(changed, failures={"build-macos": {"Record cache identity"}})["workflow_success"])

    def test_behavior_negative_missing_product_guard_runs_packaging(self):
        changed = self.after.replace("      - name: Create zip artifacts\n        if: ${{ !inputs.cache_only }}", "      - name: Create zip artifacts", 1)
        self.assertNotEqual(changed, self.after)
        result = simulate(changed, event="workflow_dispatch", ref="refs/heads/main", cache_only=True)
        self.assertEqual(outcomes(result, "build-windows", {"Create zip artifacts"}), {"Create zip artifacts": "success"})

    def test_normal_diagnostic_plan_failure_is_tolerated_but_probe_failure_is_strict(self):
        for job in contract.BUILD_JOBS:
            normal = simulate(self.after, event="workflow_dispatch", ref="refs/heads/main", cache_diagnostics=True,
                              failures={job: {"Plan cache diagnostics"}})
            self.assertTrue(normal["workflow_success"])
            self.assertEqual(outcomes(normal, job, {"Build Release"}), {"Build Release": "success"})
            cold = simulate(self.after, event="workflow_dispatch", ref="refs/heads/main", cache_only=True,
                            cache_probe="cold", failures={job: {"Plan cache diagnostics"}})
            self.assertFalse(cold["workflow_success"])
            self.assertEqual(outcomes(cold, job, {"Build Release", "Save Release build"}),
                             {"Build Release": "skipped", "Save Release build": "skipped"})

    def test_cold_probe_skips_both_restores_builds_saves_and_has_no_product_actions(self):
        result = simulate(self.after, event="workflow_dispatch", ref="refs/heads/main", cache_only=True, cache_probe="cold")
        self.assertTrue(result["workflow_success"])
        self.assertEqual(result["release"], "skipped")
        for job in contract.BUILD_JOBS:
            self.assertEqual(set(outcomes(result, job, {"Restore Cargo registry", "Restore Release build"}).values()), {"skipped"})
            self.assertEqual(set(outcomes(result, job, {"Build Release", "Save Cargo registry", "Save Release build"}).values()), {"success"})
            self.assertEqual(set(outcomes(result, job, contract.PRODUCT_STEPS[job]).values()), {"skipped"})

    def test_warm_probe_requires_both_exact_hits_including_windows_and_still_builds(self):
        options = dict(event="workflow_dispatch", ref="refs/heads/main", cache_only=True, cache_probe="warm", probe_id="12345-1")
        warm = simulate(self.after, cache_hit="true", registry_cache_hit="true", **options)
        self.assertTrue(warm["workflow_success"])
        for job in contract.BUILD_JOBS:
            self.assertEqual(outcomes(warm, job, {"Build Release"}), {"Build Release": "success"})
            self.assertEqual(set(outcomes(warm, job, {"Save Cargo registry", "Save Release build"}).values()), {"skipped"})
            self.assertEqual(set(outcomes(warm, job, contract.PRODUCT_STEPS[job]).values()), {"skipped"})
        for build, registry in (("false", "true"), ("true", "false"), ("", "")):
            result = simulate(self.after, cache_hit=build, registry_cache_hit=registry, **options)
            self.assertFalse(result["workflow_success"])
            for job in contract.BUILD_JOBS:
                self.assertEqual(outcomes(result, job, {"Verify warm cache exact"}), {"Verify warm cache exact": "failure"})
                self.assertEqual(outcomes(result, job, {"Build Release"}), {"Build Release": "skipped"})

    def test_probe_non_cache_only_and_invalid_cohort_fail_before_checkout(self):
        for mode, flag, identifier in (("cold", False, ""), ("warm", False, "12345-1"),
                                       ("warm", True, ""), ("cold", True, "12345-1")):
            result = simulate(self.after, event="workflow_dispatch", ref="refs/heads/main", cache_only=flag,
                              cache_probe=mode, probe_id=identifier)
            self.assertFalse(result["workflow_success"])
            for job in contract.BUILD_JOBS:
                self.assertEqual(outcomes(result, job, {"Checkout", "Build Release"}), {"Checkout": "skipped", "Build Release": "skipped"})

    def test_negative_probe_tolerance_would_make_failed_probe_falsely_green(self):
        changed = self.after.replace("continue-on-error: ${{ inputs.cache_probe == 'normal' }}", "continue-on-error: true")
        self.assertNotEqual(changed, self.after)
        result = simulate(changed, event="workflow_dispatch", ref="refs/heads/main", cache_only=True,
                          cache_probe="cold", failures={"build-windows": {"Plan cache diagnostics"}})
        self.assertTrue(result["workflow_success"])
        self.assertTrue(contract.validate_contract(changed))


if __name__ == "__main__":
    unittest.main()
