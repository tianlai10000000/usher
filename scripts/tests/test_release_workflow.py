"""Release cache/report structure contracts, using only the Python standard library.

This deliberately checks the repository's fixed job/step indentation, not general
YAML syntax. actionlint remains the separate workflow syntax/Actions validator.
``validate_contract(text)`` is importable for baseline/candidate fixture checks;
importing this module never starts Cargo, a workflow, or a network request.
"""
import re
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]
CACHE_VERSION = "v6.1.0"
BUILD_JOBS = ("build-macos", "build-windows")
HASH_INPUTS = ("Cargo.lock", "**/Cargo.toml", ".cargo/config.toml", ".github/workflows/release.yml")
BUILD_PATHS = {"target/release", "target/${{ matrix.target }}/release", "target/.rustc_info.json"}
COARSE_BUILD_PREFIX = "release-build-v1-${{ runner.os }}-${{ runner.arch }}-${{ matrix.target }}-${{ steps.rust.outputs.cachekey }}-release-default-"
PREWARM_GUARD = "github.event_name == 'workflow_dispatch' && inputs.cache_only"
PREWARM_SCRIPT = ('if [[ "$GITHUB_REF" != "refs/heads/$RELEASE_DEFAULT_BRANCH" ]]; then\n'
                  'echo "::error::Cache-only prewarming must run on the repository default branch."\n'
                  'exit 1\nfi')
PRODUCT_STEPS = {
    "build-macos": {"Install cargo-bundle", "Install create-dmg", "Create App Bundle",
                    "Install Apple signing certificate", "Sign macOS executable and app bundle",
                    "Create tar.gz for self_update", "Create DMG Installer", "Verify artifacts before upload",
                    "Notarize and staple DMG", "Remove temporary signing keychain", "Upload Artifacts"},
    "build-windows": {"Create zip artifacts", "Upload Artifacts"},
}


def _clean(text):
    return "\n".join(line for line in text.splitlines() if line.strip() and not line.lstrip().startswith("#"))


def _field(text, name, indent):
    match = re.search(r"^(?: {" + str(indent) + r"}| {" + str(indent - 2) + r"}- )"
                      + re.escape(name) + r":[ \t]*(.*?)[ \t]*$", text, re.M)
    if not match:
        return None
    value = match[1]
    return value[1:-1] if len(value) > 1 and value[0] == value[-1] and value[0] in "\"'" else value


def _section(text, name, indent):
    lines = text.splitlines()
    for i, line in enumerate(lines):
        if re.fullmatch(" " * indent + re.escape(name) + r":\s*", line):
            end = i + 1
            while end < len(lines) and (not lines[end].strip() or len(lines[end]) - len(lines[end].lstrip()) > indent):
                end += 1
            return "\n".join(lines[i + 1:end])
    return ""


def _jobs(text):
    body = _section(text, "jobs", 0)
    starts = list(re.finditer(r"^  ([a-z][a-z0-9-]*):\s*$", body, re.M))
    return {match[1]: body[match.end():starts[i + 1].start() if i + 1 < len(starts) else len(body)]
            for i, match in enumerate(starts)}


def _steps(job):
    starts = list(re.finditer(r"^      - \S", job, re.M))
    return [job[match.start():starts[i + 1].start() if i + 1 < len(starts) else len(job)]
            for i, match in enumerate(starts)]


def _mapping(text, name, indent):
    return {match[1]: match[2].strip().strip("\"'") for match in re.finditer(
        r"^ {" + str(indent + 2) + r"}([A-Za-z0-9_-]+):\s*(.*?)\s*$", _section(text, name, indent), re.M)}


def _compact(value):
    return re.sub(r"\s+", "", value or "")


def _expression(value):
    value = _compact(value)
    return value[3:-2] if value.startswith("${{") and value.endswith("}}") else value


def _paths(step):
    value = _field(step, "path", 10)
    if value in ("|", ">", "|-", ">-"):
        body = re.search(r"^          path:.*\n((?: {12}.*(?:\n|$))*)", step, re.M)
        return {line.strip().rstrip("/") for line in body[1].splitlines()} if body else set()
    return {value.rstrip("/")} if value else set()


def _restore_prefixes(step):
    match = re.search(r"^          restore-keys:\s*\|[-+]?\s*\n((?: {12}.*(?:\n|$))*)", step, re.M)
    return [line.strip() for line in match[1].splitlines()] if match else []


def _expanded_script(step):
    env = {"GITHUB_REPOSITORY": "${{ github.repository }}", "GITHUB_RUN_ID": "${{ github.run_id }}",
           "GITHUB_RUN_ATTEMPT": "${{ github.run_attempt }}", **_mapping(step, "env", 8)}
    text = step
    for name, value in env.items():
        if value.startswith("${{"):
            text = re.sub(r"\$\{" + re.escape(name) + r"\}|\$" + re.escape(name) + r"\b", lambda _: value, text)
    return _compact(text.replace("\\\n", "").replace('"', "").replace("'", ""))


def validate_contract(workflow_text):
    """Return safe contract errors without executing code or parsing arbitrary YAML."""
    errors = []
    def require(ok, message):
        if not ok:
            errors.append(message)
    text = _clean(workflow_text)
    require("\t" not in text, "workflow must use space indentation")
    triggers = _section(text, "on", 0)
    require(set(re.findall(r"^  ([a-z_]+):", triggers, re.M)) == {"push", "workflow_dispatch"}, "release triggers changed")
    require([line.strip() for line in _section(triggers, "push", 2).splitlines()] == ["tags:", "- 'v*'"], "release tag trigger changed")
    inputs = _section(_section(triggers, "workflow_dispatch", 2), "inputs", 4)
    require(re.findall(r"^      ([a-z_]+):", inputs, re.M) == ["cache_only"], "manual input contract changed")
    cache_only = _section(inputs, "cache_only", 6)
    require(_field(cache_only, "type", 8) == "boolean" and _field(cache_only, "default", 8) == "false"
            and _field(cache_only, "required", 8) == "false", "cache_only must be an optional boolean default false")
    jobs = _jobs(text)
    require(set(jobs) == {*BUILD_JOBS, "release", "report-timings"}, "expected only build, release and timing-report jobs")
    require(not re.search(r"permissions:\s*write-all", text), "write-all permissions forbidden")
    require(not re.search(r"^\s+(?:CARGO_PROFILE_RELEASE_[A-Z_]+|RUSTFLAGS|CARGO_ENCODED_RUSTFLAGS):", text, re.M),
            "release compiler/profile environment overrides forbidden")
    require(all(value == "read" for value in _mapping(text, "permissions", 0).values()), "global write permission forbidden")
    global_inline = re.search(r"^permissions:[ \t]*(\S.*)$", text, re.M)
    require(not global_inline or global_inline[1] in ("read-all", "{}"), "inline global permissions outside contract")
    for name, job in jobs.items():
        permissions = _mapping(job, "permissions", 4)
        if name == "release":
            require(permissions == {"contents": "write"}, "release publishing permissions changed")
        elif name == "report-timings":
            require(permissions == {"actions": "read", "contents": "read"}, "timing report permissions must be actions/contents read only")
        else:
            require(all(value == "read" for value in permissions.values()), name + ": elevated build permissions")
            require(_field(job, "permissions", 4) in (None, "", "read-all", "{}"), name + ": inline build permissions outside contract")
    for name in BUILD_JOBS:
        job = jobs.get(name, "")
        steps = _steps(job)
        require(_field(job, "continue-on-error", 4) in (None, "false"), name + ": build job must remain blocking")
        guards = [step for step in steps if _field(step, "name", 8) == "Validate cache prewarm ref"]
        require(len(guards) == 1 and steps[0] == guards[0], name + ": default-branch validation must be the first step")
        for guard in guards:
            require(_expression(_field(guard, "if", 8)) == _expression(PREWARM_GUARD), name + ": invalid cache prewarm ref guard")
            match = re.search(r"^        run: \|\n((?: {10}.*(?:\n|$))*)", guard, re.M)
            script = "\n".join(line.strip() for line in match[1].splitlines()) if match else ""
            require(_mapping(guard, "env", 8) == {"RELEASE_DEFAULT_BRANCH": "${{ github.event.repository.default_branch }}"}, name + ": prewarm default branch must be data in explicit env")
            require(_field(guard, "shell", 8) == "bash" and script == PREWARM_SCRIPT,
                    name + ": prewarm rejection must use a static bash error/exit")
        by_id = {_field(step, "id", 8): step for step in steps if _field(step, "id", 8)}
        require(len(by_id) == sum(bool(_field(step, "id", 8)) for step in steps), name + ": duplicate step ids")
        require("rust" in by_id and "dtolnay/rust-toolchain@" in (by_id.get("rust") or ""), name + ": rust step id missing")
        python = [step for step in steps if _field(step, "uses", 8) == "actions/setup-python@v6"]
        require(len(python) == 1 and _field(python[0], "python-version", 10) == "3.12", name + ": Python 3.12 setup missing")
        require(len(python) == 1 and _field(python[0], "continue-on-error", 8) == "true", name + ": diagnostic Python setup must be non-blocking")
        for cache_id in ("registry-cache", "build-cache"):
            restore = by_id.get(cache_id, "")
            require(_field(restore, "uses", 8) == "actions/cache/restore@" + CACHE_VERSION, name + ": " + cache_id + " restore missing")
            require(_field(restore, "if", 8) is None, name + ": " + cache_id + " restore must not be skipped")
        build_restore = by_id.get("build-cache", "")
        key = _field(build_restore, "key", 10) or ""
        compact_key = _compact(key)
        for value in ("${{ runner.os }}", "${{ runner.arch }}", "${{ matrix.target }}", "${{ steps.rust.outputs.cachekey }}", "${{ github.sha }}", "release-default"):
            require(_compact(value) in compact_key, name + ": compiled key missing " + value)
        hashes = re.findall(r"hashFiles\(([^)]*)\)", key)
        files = {value for expression in hashes for value in re.findall(r"['\"]([^'\"]+)['\"]", expression)}
        require(len(hashes) == 1 and files == set(HASH_INPUTS), name + ": compiled key configuration hash inputs differ")
        suffix = "-${{github.sha}}"
        require(compact_key.endswith(suffix), name + ": compiled key must rotate by commit SHA")
        prefixes = [_compact(value) for value in _restore_prefixes(build_restore)]
        require(prefixes == [compact_key[:-len(suffix)] + "-", _compact(COARSE_BUILD_PREFIX)] and bool(key),
                name + ": compiled fallback must retain strict then compatible platform/toolchain/profile/feature identity")
        require({_compact(value) for value in _paths(build_restore)} == {_compact(value) for value in BUILD_PATHS}, name + ": compiled cache paths changed")
        registry_paths = _paths(by_id.get("registry-cache", ""))
        require({"~/.cargo/registry/index", "~/.cargo/registry/cache", "~/.cargo/git/db"} <= registry_paths,
                name + ": registry download cache paths missing")
        require(not any(value in ("~/.cargo", "~/.cargo/credentials", "~/.cargo/credentials.toml") for value in registry_paths), name + ": registry cache includes credentials/root")
        registry = by_id.get("registry-cache", "")
        registry_key = _field(registry, "key", 10) or ""
        for value in ("${{ runner.os }}", "${{ runner.arch }}", "${{ steps.rust.outputs.cachekey }}"):
            require(_compact(value) in _compact(registry_key), name + ": registry key missing toolchain/platform boundary")
        registry_hash = re.search(r"-\$\{\{\s*hashFiles\(([^)]*)\)\s*\}\}$", registry_key)
        registry_files = set(re.findall(r"['\"]([^'\"]+)['\"]", registry_hash[1])) if registry_hash else set()
        require(registry_files == {"Cargo.lock", "**/Cargo.toml", ".github/workflows/release.yml"}, name + ": registry key hash inputs differ")
        expected_registry_prefix = registry_key[:registry_hash.start()] + "-" if registry_hash else None
        require(_restore_prefixes(registry) == [expected_registry_prefix], name + ": registry fallback crosses toolchain/platform")
        builds = [step for step in steps if _field(step, "name", 8) == "Build Release"]
        require(len(builds) == 1, name + ": expected one Build Release")
        build = builds[0] if builds else ""
        build_id = _field(build, "id", 8)
        require(bool(build_id), name + ": Build Release id missing")
        require(_field(build, "if", 8) is None, name + ": cache hit must not skip Cargo build")
        expected = "cargo build --release --target ${{ matrix.target }}"
        if name == "build-macos":
            expected += " --jobs $(sysctl -n hw.ncpu)"
        command = _field(build, "run", 8) or ""
        require(command.count(" --timings") == 1 and command.count(" --locked") == 1
                and command.replace(" --timings", "").replace(" --locked", "") == expected,
                name + ": release command/features/profile changed or --timings/--locked missing")
        require(_mapping(build, "env", 8).get("CARGO_INCREMENTAL") == "0", name + ": release incremental policy changed")
        build_index = steps.index(build) if build else -1
        packaging = [i for i, step in enumerate(steps) if i > build_index and re.search(r"cargo bundle|codesign |tar -cz|Compress-Archive|create-dmg \\", step)]
        boundary = min(packaging) if packaging else len(steps)
        saves = [step for step in steps if _field(step, "uses", 8) == "actions/cache/save@" + CACHE_VERSION]
        require(len(saves) == 2, name + ": expected registry/build save steps")
        for cache_id in ("registry-cache", "build-cache"):
            matching = [step for step in saves if _compact(_field(step, "key", 10)) == "${{steps." + cache_id + ".outputs.cache-primary-key}}"]
            require(len(matching) == 1, name + ": " + cache_id + " save must use restore primary key")
            for save in matching:
                condition = _expression(_field(save, "if", 8))
                success = "steps." + str(build_id) + ".outcome=='success'"
                allowed = {success, "success()", "steps." + cache_id + ".outputs.cache-hit!='true'"}
                terms = condition.split("&&")
                require(success in terms and set(terms) <= allowed, name + ": cache saved without build success")
                require(_paths(save) == _paths(by_id.get(cache_id, "")), name + ": cache save/restore paths differ")
                require(build_index < steps.index(save) < boundary, name + ": cache save must precede packaging/signing after build")
        metadata = [step for step in steps if re.search(r"ci_release_metrics\.py\s+cache\b", step)]
        require(len(metadata) == 1, name + ": cache metadata step missing/duplicate")
        for step in metadata:
            require(_field(step, "continue-on-error", 8) == "true", name + ": cache diagnostics must be non-blocking")
            env = _mapping(step, "env", 8)
            expected_env = {"RELEASE_TARGET", "CACHE_IDENTITY"}
            for prefix, cache_id in (("REGISTRY", "registry-cache"), ("BUILD", "build-cache")):
                for field, output in (("CACHE_HIT", "outputs.cache-hit"), ("CACHE_MATCHED_KEY", "outputs.cache-matched-key"), ("CACHE_OUTCOME", "outcome")):
                    variable = prefix + "_" + field
                    expected_env.add(variable)
                    require(_compact(env.get(variable)) == "${{steps." + cache_id + "." + output + "}}", name + ": metadata mapping differs for " + variable)
            require(set(env) == expected_env, name + ": metadata must use exactly eight explicit fields")
            require(_compact(env.get("RELEASE_TARGET")) == "${{matrix.target}}", name + ": metadata target missing")
            require(_compact(env.get("CACHE_IDENTITY")) == "${{steps.build-cache.outputs.cache-primary-key}}", name + ": metadata identity differs")
            require(_expression(_field(step, "if", 8)) == "always()", name + ": cache metadata must survive failed builds")
        uploads = [step for step in steps if (_field(step, "uses", 8) or "").startswith("actions/upload-artifact@") and _compact(_field(step, "name", 10)) == "release-diagnostics-${{matrix.target}}-attempt-${{github.run_attempt}}"]
        require(len(uploads) == 1, name + ": isolated release diagnostics upload missing")
        for upload in uploads:
            require(_field(upload, "continue-on-error", 8) == "true", name + ": diagnostics upload must be non-blocking")
            require(_expression(_field(upload, "if", 8)) == "always()", name + ": diagnostics upload must run always")
            paths = " ".join(_paths(upload))
            require("cache.json" in paths and ("cargo-timings" in paths or ".html" in paths), name + ": diagnostics must contain cache JSON and Cargo HTML")
        for upload in steps:
            if not (_field(upload, "uses", 8) or "").startswith("actions/upload-artifact@"):
                continue
            artifact_name = _compact(_field(upload, "name", 10))
            if artifact_name.startswith(("Usher-", "${{env.APP_NAME}}-")):
                paths = _paths(upload)
                require(bool(paths) and all(not value.startswith(("release-diagnostics", "release-timings", "target/"))
                        and value.endswith((".tar.gz", ".zip", ".dmg")) for value in paths), name + ": product artifact contains diagnostic/non-package paths")
        diagnostic_steps = python + metadata + uploads
        for step in steps:
            if step not in diagnostic_steps:
                require(_field(step, "continue-on-error", 8) in (None, "false"), name + ": non-diagnostic step must remain blocking")
            if _field(step, "name", 8) in PRODUCT_STEPS[name] or _field(step, "name", 8) == "Package":
                terms = _expression(_field(step, "if", 8)).split("&&")
                require("!inputs.cache_only" in terms and "||" not in "".join(terms), name + ": product step must be excluded from cache_only")
        bundle = [step for step in steps if _field(step, "name", 8) == "Create App Bundle"]
        for step in bundle:
            require(_field(step, "run", 8) == "cargo bundle --release --format osx --target ${{ matrix.target }}", name + ": bundle command changed")
    release = jobs.get("release", "")
    require(set(re.findall(r"[a-z][a-z-]*", _field(release, "needs", 4) or "")) == set(BUILD_JOBS), "release build dependencies changed")
    require(_expression(_field(release, "if", 4)) == "!inputs.cache_only&&startsWith(github.ref,'refs/tags/')", "release tag/cache_only publishing guard changed")
    require(_field(release, "continue-on-error", 4) in (None, "false") and all(_field(step, "continue-on-error", 8) in (None, "false") for step in _steps(release)), "release publishing must remain blocking")
    downloads = [step for step in _steps(release) if (_field(step, "uses", 8) or "").startswith("actions/download-artifact@")]
    pattern = _field(downloads[0], "pattern", 10) if len(downloads) == 1 else None
    app_name = _mapping(text, "env", 0).get("APP_NAME")
    require(pattern == "Usher-*" or (_compact(pattern) == "${{env.APP_NAME}}-*" and app_name == "Usher"), "release must download only product artifacts")
    report = jobs.get("report-timings", "")
    require(_field(report, "name", 4) == "Release timing report", "timing report name changed")
    require(_expression(_field(report, "if", 4)) == "always()", "timing report must run always")
    require(_field(report, "continue-on-error", 4) == "true", "timing report job must be non-blocking")
    needs = _field(report, "needs", 4) or ""
    require(set(re.findall(r"[a-z][a-z-]*", needs)) == {*BUILD_JOBS, "release"}, "timing report needs all release jobs")
    report_steps = _steps(report)
    python = [step for step in report_steps if _field(step, "uses", 8) == "actions/setup-python@v6"]
    require(len(python) == 1 and _field(python[0], "python-version", 10) == "3.12", "timing report Python 3.12 setup missing")
    scripts = [_expanded_script(step) for step in report_steps]
    endpoint = "repos/${{github.repository}}/actions/runs/${{github.run_id}}/attempts/${{github.run_attempt}}/jobs"
    require(any("ghapi" in script and endpoint in script and "--paginate" in script and "--slurp" in script for script in scripts), "jobs API must use current run attempt and all pages")
    require(any("ci_release_metrics.pyjobs" in script and "--attempt${{github.run_attempt}}" in script for script in scripts), "jobs projection must pass current --attempt")
    require("GITHUB_STEP_SUMMARY" in report and "summary.md" in report, "timing report must publish step summary")
    report_uploads = [step for step in report_steps if (_field(step, "uses", 8) or "").startswith("actions/upload-artifact@")]
    require(bool(report_uploads) and all(_expression(_field(step, "if", 8)) == "always()" for step in report_uploads), "timing JSON artifact must upload always")
    require(len(report_uploads) == 1 and _paths(report_uploads[0]) == {"release-timings"}
            and _compact(_field(report_uploads[0], "name", 10)) == "release-stage-timings-attempt-${{github.run_attempt}}", "timing JSON artifact must remain isolated")
    return errors


# Small complete structure fixture; no private repository evidence is required.
def candidate_fixture():
    key = "release-build-v1-${{ runner.os }}-${{ runner.arch }}-${{ matrix.target }}-${{ steps.rust.outputs.cachekey }}-release-default-${{ hashFiles('Cargo.lock', '**/Cargo.toml', '.cargo/config.toml', '.github/workflows/release.yml') }}"
    jobs = []
    for job in BUILD_JOBS:
        registry = "            ~/.cargo/registry/index/\n            ~/.cargo/registry/cache/\n            ~/.cargo/git/db/"
        paths = "            target/release/\n            target/${{ matrix.target }}/release/\n            target/.rustc_info.json"
        body = f"  {job}:\n    steps:\n      - name: Validate cache prewarm ref\n        if: {PREWARM_GUARD}\n        shell: bash\n        env:\n          RELEASE_DEFAULT_BRANCH: ${{{{ github.event.repository.default_branch }}}}\n        run: |\n" + "\n".join("          " + line for line in PREWARM_SCRIPT.splitlines()) + "\n      - uses: actions/setup-python@v6\n        continue-on-error: true\n        with:\n          python-version: '3.12'\n      - uses: dtolnay/rust-toolchain@1.98.1\n        id: rust\n"
        registry_base = "release-registry-v1-${{ runner.os }}-${{ runner.arch }}-${{ steps.rust.outputs.cachekey }}"
        registry_key = registry_base + "-${{ hashFiles('Cargo.lock', '**/Cargo.toml', '.github/workflows/release.yml') }}"
        for identity, path, cache_key, restore in (("registry-cache", registry, registry_key, registry_base + "-"),
                                                   ("build-cache", paths, key + "-${{ github.sha }}", key + "-")):
            body += f"      - uses: actions/cache/restore@v6.1.0\n        id: {identity}\n        with:\n          path: |\n{path}\n          key: {cache_key}\n          restore-keys: |\n            {restore}\n"
            if identity == "build-cache":
                body += f"            {COARSE_BUILD_PREFIX}\n"
        command = "cargo build --release --target ${{ matrix.target }} --timings --locked"
        if job == "build-macos":
            command += " --jobs $(sysctl -n hw.ncpu)"
        body += f"      - name: Build Release\n        id: build\n        run: {command}\n        env:\n          CARGO_INCREMENTAL: 0\n"
        for identity, path in (("registry-cache", registry), ("build-cache", paths)):
            body += f"      - uses: actions/cache/save@v6.1.0\n        if: steps.build.outcome == 'success'\n        with:\n          path: |\n{path}\n          key: ${{{{ steps.{identity}.outputs.cache-primary-key }}}}\n"
        body += "      - name: Record cache metadata\n        if: always()\n        continue-on-error: true\n        run: python3 scripts/ci_release_metrics.py cache --output release-diagnostics/cache.json\n        env:\n          RELEASE_TARGET: ${{ matrix.target }}\n          CACHE_IDENTITY: ${{ steps.build-cache.outputs.cache-primary-key }}\n"
        for prefix, identity in (("REGISTRY", "registry-cache"), ("BUILD", "build-cache")):
            for field, output in (("CACHE_HIT", "outputs.cache-hit"), ("CACHE_MATCHED_KEY", "outputs.cache-matched-key"), ("CACHE_OUTCOME", "outcome")):
                body += f"          {prefix}_{field}: ${{{{ steps.{identity}.{output} }}}}\n"
        body += "      - name: Package\n        if: ${{ !inputs.cache_only }}\n        run: cargo bundle --release\n"
        body += "      - uses: actions/upload-artifact@v7.0.1\n        if: always()\n        continue-on-error: true\n        with:\n          name: release-diagnostics-${{ matrix.target }}-attempt-${{ github.run_attempt }}\n          path: |\n            release-diagnostics/cache.json\n            target/cargo-timings/*.html\n"
        jobs.append(body)
    return """name: Release
on:
  push:
    tags:
      - 'v*'
  workflow_dispatch:
    inputs:
      cache_only:
        description: Build caches only
        type: boolean
        required: false
        default: false
jobs:
""" + "".join(jobs) + """  release:
    needs: [build-macos, build-windows]
    if: ${{ !inputs.cache_only && startsWith(github.ref, 'refs/tags/') }}
    permissions:
      contents: write
    steps:
      - uses: actions/download-artifact@v8.0.1
        with:
          pattern: Usher-*
  report-timings:
    name: Release timing report
    continue-on-error: true
    needs: [build-macos, build-windows, release]
    if: always()
    permissions:
      actions: read
      contents: read
    steps:
      - uses: actions/setup-python@v6
        with:
          python-version: '3.12'
      - name: Fetch jobs
        run: |
          gh api --paginate --slurp "repos/${GITHUB_REPOSITORY}/actions/runs/${GITHUB_RUN_ID}/attempts/${GITHUB_RUN_ATTEMPT}/jobs?per_page=100" > jobs.json
          python3 scripts/ci_release_metrics.py jobs --input jobs.json --output-dir timings --attempt "$GITHUB_RUN_ATTEMPT"
          cat timings/summary.md >> "$GITHUB_STEP_SUMMARY"
      - uses: actions/upload-artifact@v7.0.1
        if: always()
        with:
          name: release-stage-timings-attempt-${{ github.run_attempt }}
          path: release-timings/
"""


class ContractFixtures(unittest.TestCase):
    def setUp(self):
        self.candidate = candidate_fixture()

    def reject(self, before, after):
        self.assertIn(before, self.candidate)
        self.assertTrue(validate_contract(self.candidate.replace(before, after, 1)))

    def test_candidate_passes(self):
        self.assertEqual(validate_contract(self.candidate), [])

    def test_missing_windows_cache_rejected(self):
        start = self.candidate.index("  build-windows:\n")
        cache = next(step for step in _steps(_jobs(self.candidate)["build-windows"])
                     if _field(step, "id", 8) == "build-cache")
        text = self.candidate[:start] + self.candidate[start:].replace(cache, "", 1)
        self.assertTrue(any("build-windows" in error for error in validate_contract(text)))

    def test_key_identity_boundaries_rejected(self):
        for context in ("runner.os", "runner.arch", "matrix.target", "steps.rust.outputs.cachekey", "github.sha"):
            with self.subTest(context=context):
                self.assertTrue(validate_contract(self.candidate.replace("${{ " + context + " }}", "identity-removed")))

    def test_configuration_hash_inputs_rejected(self):
        for path in HASH_INPUTS:
            with self.subTest(path=path):
                self.reject("'" + path + "'", "'different-input'")

    def test_configuration_hash_exclusion_cannot_disable_inputs(self):
        self.reject("'.cargo/config.toml', '.github/workflows/release.yml')", "'.cargo/config.toml', '.github/workflows/release.yml', '!**')")

    def test_cross_configuration_fallback_rejected(self):
        self.reject("          restore-keys: |\n            release-build-v1-", "          restore-keys: |\n            broad-\n            release-build-v1-")

    def test_compatible_coarse_fallback_is_required_and_ordered_after_strict(self):
        self.reject("            " + COARSE_BUILD_PREFIX + "\n", "")
        coarse = "            " + COARSE_BUILD_PREFIX + "\n"
        restore = next(step for step in _steps(_jobs(self.candidate)["build-macos"]) if _field(step, "id", 8) == "build-cache")
        strict = "            " + _restore_prefixes(restore)[0] + "\n"
        self.reject(strict + coarse, coarse + strict)

    def test_coarse_fallback_must_not_drop_platform_toolchain_or_explicit_profile_feature(self):
        for token in ("${{ runner.os }}-", "${{ runner.arch }}-", "${{ matrix.target }}-", "${{ steps.rust.outputs.cachekey }}-", "release-default-"):
            with self.subTest(token=token):
                self.reject("            " + COARSE_BUILD_PREFIX + "\n", "            " + COARSE_BUILD_PREFIX.replace(token, "") + "\n")

    def test_prewarm_input_requires_boolean_false_default(self):
        self.reject("type: boolean", "type: string")
        self.reject("default: false", "default: true")
        self.reject("required: false", "required: true")

    def test_prewarm_guard_cannot_move_after_setup(self):
        guard = _steps(_jobs(self.candidate)["build-macos"])[0]
        text = self.candidate.replace(guard, "", 1)
        rust = next(step for step in _steps(_jobs(text)["build-macos"]) if _field(step, "id", 8) == "rust")
        self.assertTrue(validate_contract(text.replace(rust, rust + guard, 1)))

    def test_prewarm_ref_uses_quoted_bash_and_explicit_environment(self):
        self.reject(PREWARM_GUARD, PREWARM_GUARD + " && github.ref != 'refs/heads/main'")
        self.reject('"refs/heads/$RELEASE_DEFAULT_BRANCH"', 'refs/heads/${{ github.event.repository.default_branch }}')
        self.reject('"$GITHUB_REF"', '$GITHUB_REF')
        self.reject("RELEASE_DEFAULT_BRANCH: ${{ github.event.repository.default_branch }}", "RELEASE_DEFAULT_BRANCH: main")
        self.reject("exit 1", "exit 0")

    def test_diagnostic_failures_cannot_block_build_or_workflow(self):
        self.reject("      - uses: actions/setup-python@v6\n        continue-on-error: true", "      - uses: actions/setup-python@v6")
        self.reject("        if: always()\n        continue-on-error: true", "        if: always()")
        self.reject("    name: Release timing report\n    continue-on-error: true", "    name: Release timing report")

    def test_production_failures_cannot_be_silenced(self):
        self.reject("        id: build\n", "        id: build\n        continue-on-error: true\n")
        self.reject("      - name: Package\n", "      - name: Package\n        continue-on-error: true\n")
        self.reject("  release:\n", "  release:\n    continue-on-error: true\n")
        self.reject("  build-windows:\n", "  build-windows:\n    continue-on-error: true\n")

    def test_product_and_release_must_be_excluded_from_cache_only(self):
        self.reject("      - name: Package\n        if: ${{ !inputs.cache_only }}", "      - name: Package")
        self.reject("if: ${{ !inputs.cache_only && startsWith(github.ref, 'refs/tags/') }}", "if: startsWith(github.ref, 'refs/tags/')")

    def test_locked_build_is_required(self):
        self.reject(" --locked", "")

    def test_whole_target_cache_rejected(self):
        self.reject("            target/.rustc_info.json", "            target/")

    def test_cache_hit_must_not_skip_build(self):
        self.reject("        id: build\n", "        id: build\n        if: steps.build-cache.outputs.cache-hit != 'true'\n")

    def test_release_command_and_feature_changes_rejected(self):
        for before, after in (("--release", "--profile release-fast"), ("--timings", ""),
                              ("--target ${{ matrix.target }}", "--features ui-components --target ${{ matrix.target }}")):
            with self.subTest(after=after):
                self.reject(before, after)

    def test_release_incremental_policy_rejected(self):
        self.reject("CARGO_INCREMENTAL: 0", "CARGO_INCREMENTAL: 1")

    def test_failed_build_cache_save_rejected(self):
        self.reject("if: steps.build.outcome == 'success'", "if: always()")
        self.reject("if: steps.build.outcome == 'success'", "if: failure() || steps.build.outcome == 'success'")

    def test_save_after_packaging_rejected(self):
        save = next(step for step in _steps(_jobs(self.candidate)["build-macos"])
                    if _field(step, "uses", 8) == "actions/cache/save@v6.1.0")
        text = self.candidate.replace(save, "", 1)
        package = "      - name: Package\n        if: ${{ !inputs.cache_only }}\n        run: cargo bundle --release\n"
        self.assertTrue(validate_contract(text.replace(package, package + save, 1)))

    def test_registry_restore_must_keep_toolchain_boundary(self):
        self.reject("            release-registry-v1-${{ runner.os }}-${{ runner.arch }}-${{ steps.rust.outputs.cachekey }}-\n", "            release-registry-v1-${{ runner.os }}-\n")

    def test_failure_diagnostics_must_survive(self):
        self.reject("        if: always()\n        continue-on-error: true\n        with:\n          name: release-diagnostics-", "        if: success()\n        continue-on-error: true\n        with:\n          name: release-diagnostics-")

    def test_release_download_cannot_include_diagnostics(self):
        self.reject("pattern: Usher-*", "pattern: '*'")

    def test_permissions_cannot_expand(self):
        for before, after in (("actions: read", "actions: write"), ("contents: write", "contents: read")):
            with self.subTest(before=before):
                self.reject(before, after)

    def test_inline_permission_escalation_cannot_bypass_mapping_check(self):
        self.reject("  build-windows:\n", "  build-windows:\n    permissions: {contents: write}\n")
        self.assertTrue(validate_contract("permissions: {contents: write}\n" + self.candidate))

    def test_profile_environment_overrides_rejected(self):
        self.reject("CARGO_INCREMENTAL: 0", "CARGO_INCREMENTAL: 0\n          CARGO_PROFILE_RELEASE_LTO: thin")

    def test_python_setup_missing_rejected(self):
        self.reject("uses: actions/setup-python@v6", "uses: unused/setup-python@v6")

    def test_product_artifact_cannot_contain_diagnostics(self):
        added = "      - uses: actions/upload-artifact@v7.0.1\n        with:\n          name: Usher-windows\n          path: release-diagnostics/\n"
        self.reject("  release:\n", added + "  release:\n")

    def test_timing_json_upload_must_remain_isolated(self):
        self.reject("name: release-stage-timings-attempt-", "name: Usher-timings-attempt-")

    def test_trigger_changes_rejected(self):
        self.reject("  workflow_dispatch:\n", "  workflow_dispatch:\n  pull_request:\n")

    def test_wrong_attempt_endpoint_rejected(self):
        self.reject("/attempts/${GITHUB_RUN_ATTEMPT}/jobs", "/jobs")

    def test_wrong_attempt_argument_rejected(self):
        self.reject('--attempt "$GITHUB_RUN_ATTEMPT"', "--attempt 1")

    def test_report_dependencies_and_always_required(self):
        self.reject("needs: [build-macos, build-windows, release]", "needs: [build-macos, build-windows]")
        self.reject("\n    if: always()", "\n    if: success()")

    def test_cache_metadata_cannot_dump_arbitrary_environment(self):
        self.reject("REGISTRY_CACHE_HIT:", "UNBOUNDED_ENV:")
        self.reject("CACHE_IDENTITY: ${{ steps.build-cache.outputs.cache-primary-key }}", "CACHE_IDENTITY: arbitrary")

    def test_import_has_no_execution_side_effects(self):
        self.assertTrue(callable(validate_contract))


class RepositoryContract(unittest.TestCase):
    def test_current_release_workflow_contract(self):
        errors = validate_contract((ROOT / ".github/workflows/release.yml").read_text())
        self.assertEqual(errors, [], "\n".join(errors))


if __name__ == "__main__":
    unittest.main()
