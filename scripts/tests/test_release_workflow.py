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
PRODUCT_RETENTION_DAYS = 7
DIAGNOSTIC_RETENTION_DAYS = 3
COARSE_BUILD_PREFIX = "release-build-v1-${{ runner.os }}-${{ runner.arch }}-${{ matrix.target }}-${{ steps.rust.outputs.cachekey }}-release-default-"
PREWARM_GUARD = "github.event_name == 'workflow_dispatch' && inputs.cache_only"
PREWARM_SCRIPT = ('if [[ "$GITHUB_REF" != "refs/heads/$RELEASE_DEFAULT_BRANCH" ]]; then\n'
                  'echo "::error::Cache-only prewarming must run on the repository default branch."\n'
                  'exit 1\nfi')
PROBE_INPUT_SCRIPT = '''case "$RELEASE_PROBE_MODE" in
  normal) [[ -z "$RELEASE_PROBE_ID" ]] ;;
  cold) [[ "$RELEASE_CACHE_ONLY" == "true" && -z "$RELEASE_PROBE_ID" ]] ;;
  warm) [[ "$RELEASE_CACHE_ONLY" == "true" && "$RELEASE_PROBE_ID" =~ ^[1-9][0-9]{0,19}-[1-9][0-9]{0,3}$ ]] ;;
  *) false ;;
esac || { echo "::error::Invalid cache probe inputs; use cache-only cold or warm with a prior cold run-attempt ID."; exit 1; }'''
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


def _retention_within(step, maximum):
    value = _field(step, "retention-days", 10)
    return bool(re.fullmatch(r"[1-9][0-9]*", value or "")) and int(value) <= maximum


def _expanded_script(step):
    env = {"GITHUB_REPOSITORY": "${{ github.repository }}", "GITHUB_RUN_ID": "${{ github.run_id }}",
           "GITHUB_RUN_ATTEMPT": "${{ github.run_attempt }}", **_mapping(step, "env", 8)}
    text = step
    for name, value in env.items():
        if value.startswith("${{"):
            text = re.sub(r"\$\{" + re.escape(name) + r"\}|\$" + re.escape(name) + r"\b", lambda _: value, text)
    return _compact(text.replace("\\\n", "").replace('"', "").replace("'", ""))


def _script(step):
    match = re.search(r"^        run: \|\n((?: {10}.*(?:\n|$))*)", step, re.M)
    return "\n".join(line[10:] for line in match[1].splitlines()) if match else _field(step, "run", 8)


def _normal_key(value):
    for kind in ("registry", "build"):
        value = value.replace("${{ steps.cache-plan.outputs.namespace || 'release-" + kind + "-v1' }}", "release-" + kind + "-v1")
    return value


def _normal_restore_prefixes(step):
    """Evaluate only the three fixed normal-mode templates, never expressions."""
    registry = "${{ steps.cache-plan.outputs.namespace == '' && format('release-registry-v1-{0}-{1}-{2}-', runner.os, runner.arch, steps.rust.outputs.cachekey) || '' }}"
    coarse = "${{ steps.cache-plan.outputs.namespace == '' && format('release-build-v1-{0}-{1}-{2}-{3}-release-default-', runner.os, runner.arch, matrix.target, steps.rust.outputs.cachekey) || '' }}"
    strict = "${{ steps.cache-plan.outputs.namespace == '' && format('release-build-v1-{0}-{1}-{2}-{3}-release-default-{4}-', runner.os, runner.arch, matrix.target, steps.rust.outputs.cachekey, hashFiles('Cargo.lock', '**/Cargo.toml', '.cargo/config.toml', '.github/workflows/release.yml')) || '' }}"
    normal_strict = COARSE_BUILD_PREFIX + "${{ hashFiles('Cargo.lock', '**/Cargo.toml', '.cargo/config.toml', '.github/workflows/release.yml') }}-"
    fixed = {registry: "release-registry-v1-${{ runner.os }}-${{ runner.arch }}-${{ steps.rust.outputs.cachekey }}-",
             coarse: COARSE_BUILD_PREFIX, strict: normal_strict}
    return [fixed.get(value, value) for value in _restore_prefixes(step)]


def _build_script(job):
    jobs = ' --jobs "$(sysctl -n hw.ncpu)"' if job == "build-macos" else ""
    direct_jobs = " --jobs $(sysctl -n hw.ncpu)" if job == "build-macos" else ""
    return ('if [[ "$RELEASE_FINGERPRINT" == "true" ]]; then\n'
            '  python scripts/ci_release_build.py --target "$RELEASE_TARGET"' + jobs + ' --output "release-diagnostics/$RELEASE_TARGET/fingerprints.json"\n'
            'else\n  cargo build --release --target "${RELEASE_TARGET}"' + direct_jobs + ' --timings --locked\nfi')


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
    require(re.findall(r"^      ([a-z_]+):", inputs, re.M) == ["cache_only", "cache_diagnostics", "cache_probe", "probe_id"], "manual input contract changed")
    cache_only = _section(inputs, "cache_only", 6)
    require(_field(cache_only, "type", 8) == "boolean" and _field(cache_only, "default", 8) == "false"
            and _field(cache_only, "required", 8) == "false", "cache_only must be an optional boolean default false")
    diagnostics = _section(inputs, "cache_diagnostics", 6)
    require(_field(diagnostics, "type", 8) == "boolean" and _field(diagnostics, "default", 8) == "false"
            and _field(diagnostics, "required", 8) == "false", "cache diagnostics must be optional boolean default false")
    mode = _section(inputs, "cache_probe", 6)
    require(_field(mode, "type", 8) == "choice" and _field(mode, "default", 8) == "normal"
            and _field(mode, "required", 8) == "false"
            and [line.strip() for line in _section(mode, "options", 8).splitlines()] == ["- normal", "- cold", "- warm"],
            "cache probe must be optional normal/cold/warm choice default normal")
    probe_id = _section(inputs, "probe_id", 6)
    require(_field(probe_id, "type", 8) == "string" and _field(probe_id, "default", 8) == ""
            and _field(probe_id, "required", 8) == "false", "probe id must be optional string default empty")
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
        guards = [step for step in steps if _field(step, "name", 8) == "Validate release probe inputs"]
        require(len(guards) == 1 and steps[1] == guards[0], name + ": probe inputs must be checked before checkout")
        for guard in guards:
            require(_script(guard) == PROBE_INPUT_SCRIPT and _field(guard, "shell", 8) == "bash"
                    and _expression(_field(guard, "if", 8)) == "github.event_name=='workflow_dispatch'"
                    and _mapping(guard, "env", 8) == {
                        "RELEASE_PROBE_MODE": "${{ inputs.cache_probe || 'normal' }}",
                        "RELEASE_PROBE_ID": "${{ inputs.probe_id }}", "RELEASE_CACHE_ONLY": "${{ inputs.cache_only }}"},
                    name + ": probe input validation must use fixed quoted Bash data")
        plan = by_id.get("cache-plan", "")
        require(_expression(_field(plan, "if", 8)) == "github.event_name=='workflow_dispatch'&&(inputs.cache_diagnostics||inputs.cache_probe!='normal')"
                and _expression(_field(plan, "continue-on-error", 8)) == "inputs.cache_probe=='normal'",
                name + ": normal diagnostics must be optional but probe planning strict")
        require(_field(plan, "run", 8) == 'python scripts/ci_release_probe.py plan --output "release-diagnostics/${{ matrix.target }}/probe.json"'
                and _field(plan, "shell", 8) == "bash", name + ": probe planner command changed")
        env = {
            "RELEASE_DIAGNOSTICS": "inputs.cache_diagnostics", "RELEASE_PROBE_MODE": "inputs.cache_probe",
            "RELEASE_PROBE_ID": "inputs.probe_id", "RELEASE_CACHE_ONLY": "inputs.cache_only",
            "RELEASE_EVENT": "github.event_name", "RELEASE_REF": "github.ref",
            "RELEASE_DEFAULT_BRANCH": "github.event.repository.default_branch", "RELEASE_RUN_ID": "github.run_id",
            "RELEASE_RUN_ATTEMPT": "github.run_attempt", "RELEASE_TARGET": "matrix.target",
            "RELEASE_RUNNER_OS": "runner.os", "RELEASE_RUNNER_ARCH": "runner.arch",
            "RELEASE_RUST_KEY": "steps.rust.outputs.cachekey", "RELEASE_SOURCE_SHA": "github.sha",
            "RELEASE_CONFIGURATION_HASH": "hashFiles('Cargo.lock', '**/Cargo.toml', '.cargo/config.toml', '.github/workflows/release.yml')",
            "RELEASE_REGISTRY_HASH": "hashFiles('Cargo.lock', '**/Cargo.toml', '.github/workflows/release.yml')",
        }
        require(_mapping(plan, "env", 8) == {key: "${{ " + value + " }}" for key, value in env.items()},
                name + ": probe planner identity allowlist changed")
        python = [step for step in steps if _field(step, "uses", 8) == "actions/setup-python@v6"]
        require(len(python) == 1 and _field(python[0], "python-version", 10) == "3.12", name + ": Python 3.12 setup missing")
        require(len(python) == 1 and _field(python[0], "continue-on-error", 8) == "true", name + ": diagnostic Python setup must be non-blocking")
        for cache_id in ("registry-cache", "build-cache"):
            restore = by_id.get(cache_id, "")
            require(_field(restore, "uses", 8) == "actions/cache/restore@" + CACHE_VERSION, name + ": " + cache_id + " restore missing")
            require(_expression(_field(restore, "if", 8)) == "inputs.cache_probe!='cold'", name + ": only cold probes may skip restore")
            require(_compact(_field(restore, "key", 10)).startswith("${{steps.cache-plan.outputs.namespace||'release-" + cache_id.removesuffix("-cache") + "-v1'}}-"),
                    name + ": probe cache namespace must be isolated from normal caches")
            require(bool(_restore_prefixes(restore)) and all(_compact(value).startswith("${{steps.cache-plan.outputs.namespace==''&&format(")
                    for value in _restore_prefixes(restore)), name + ": probes must not use normal fallback prefixes")
            require(restore in steps and plan in steps and by_id.get("rust") in steps
                    and steps.index(by_id["rust"]) < steps.index(plan) < steps.index(restore), name + ": probe plan must precede restores")
        build_restore = by_id.get("build-cache", "")
        key = _normal_key(_field(build_restore, "key", 10) or "")
        compact_key = _compact(key)
        for value in ("${{ runner.os }}", "${{ runner.arch }}", "${{ matrix.target }}", "${{ steps.rust.outputs.cachekey }}", "${{ github.sha }}", "release-default"):
            require(_compact(value) in compact_key, name + ": compiled key missing " + value)
        hashes = re.findall(r"hashFiles\(([^)]*)\)", key)
        files = {value for expression in hashes for value in re.findall(r"['\"]([^'\"]+)['\"]", expression)}
        require(len(hashes) == 1 and files == set(HASH_INPUTS), name + ": compiled key configuration hash inputs differ")
        suffix = "-${{github.sha}}"
        require(compact_key.endswith(suffix), name + ": compiled key must rotate by commit SHA")
        prefixes = [_compact(value) for value in _normal_restore_prefixes(build_restore)]
        require(prefixes == [compact_key[:-len(suffix)] + "-", _compact(COARSE_BUILD_PREFIX)] and bool(key),
                name + ": compiled fallback must retain strict then compatible platform/toolchain/profile/feature identity")
        require({_compact(value) for value in _paths(build_restore)} == {_compact(value) for value in BUILD_PATHS}, name + ": compiled cache paths changed")
        registry_paths = _paths(by_id.get("registry-cache", ""))
        require({"~/.cargo/registry/index", "~/.cargo/registry/cache", "~/.cargo/git/db"} <= registry_paths,
                name + ": registry download cache paths missing")
        require(not any(value in ("~/.cargo", "~/.cargo/credentials", "~/.cargo/credentials.toml") for value in registry_paths), name + ": registry cache includes credentials/root")
        registry = by_id.get("registry-cache", "")
        registry_key = _normal_key(_field(registry, "key", 10) or "")
        for value in ("${{ runner.os }}", "${{ runner.arch }}", "${{ steps.rust.outputs.cachekey }}"):
            require(_compact(value) in _compact(registry_key), name + ": registry key missing toolchain/platform boundary")
        registry_hash = re.search(r"-\$\{\{\s*hashFiles\(([^)]*)\)\s*\}\}$", registry_key)
        registry_files = set(re.findall(r"['\"]([^'\"]+)['\"]", registry_hash[1])) if registry_hash else set()
        require(registry_files == {"Cargo.lock", "**/Cargo.toml", ".github/workflows/release.yml"}, name + ": registry key hash inputs differ")
        expected_registry_prefix = registry_key[:registry_hash.start()] + "-" if registry_hash else None
        require(_normal_restore_prefixes(registry) == [expected_registry_prefix], name + ": registry fallback crosses toolchain/platform")
        builds = [step for step in steps if _field(step, "name", 8) == "Build Release"]
        require(len(builds) == 1, name + ": expected one Build Release")
        build = builds[0] if builds else ""
        build_id = _field(build, "id", 8)
        require(bool(build_id), name + ": Build Release id missing")
        require(_field(build, "if", 8) is None, name + ": cache hit must not skip Cargo build")
        require(_script(build) == _build_script(name),
                name + ": release command/features/profile changed or --timings/--locked missing")
        build_env = _mapping(build, "env", 8)
        require(build_env.get("CARGO_INCREMENTAL") == "0", name + ": release incremental policy changed")
        require(set(build_env) == {"CARGO_INCREMENTAL", "CARGO_NET_GIT_FETCH_WITH_CLI", "RELEASE_TARGET", "RELEASE_FINGERPRINT"}
                and build_env.get("CARGO_NET_GIT_FETCH_WITH_CLI") == "true", name + ": unexpected release environment override")
        require(_compact(_mapping(build, "env", 8).get("RELEASE_TARGET")) == "${{matrix.target}}"
                and _compact(_mapping(build, "env", 8).get("RELEASE_FINGERPRINT")) == "${{steps.cache-plan.outputs.fingerprint||'false'}}",
                name + ": fixed build diagnostic inputs differ")
        build_index = steps.index(build) if build else -1
        verifiers = [step for step in steps if _field(step, "name", 8) == "Verify warm cache exact"]
        require(len(verifiers) == 1, name + ": strict warm verification missing")
        for verify in verifiers:
            require(_expression(_field(verify, "if", 8)) == "inputs.cache_probe=='warm'"
                    and _field(verify, "run", 8) == "python scripts/ci_release_probe.py verify-warm"
                    and _field(verify, "continue-on-error", 8) in (None, "false")
                    and by_id.get("build-cache") in steps and steps.index(by_id["build-cache"]) < steps.index(verify) < build_index,
                    name + ": warm miss/fallback must fail before building")
            require(_mapping(verify, "env", 8) == {
                "REGISTRY_CACHE_HIT": "${{ steps.registry-cache.outputs.cache-hit }}",
                "REGISTRY_CACHE_OUTCOME": "${{ steps.registry-cache.outcome }}",
                "BUILD_CACHE_HIT": "${{ steps.build-cache.outputs.cache-hit }}",
                "BUILD_CACHE_OUTCOME": "${{ steps.build-cache.outcome }}"}, name + ": warm verifier needs actual restore outcomes")
        packaging = [i for i, step in enumerate(steps) if i > build_index and re.search(r"cargo bundle|codesign |tar -cz|Compress-Archive|create-dmg \\", step)]
        boundary = min(packaging) if packaging else len(steps)
        saves = [step for step in steps if _field(step, "uses", 8) == "actions/cache/save@" + CACHE_VERSION]
        require(len(saves) == 2, name + ": expected registry/build save steps")
        for cache_id in ("registry-cache", "build-cache"):
            matching = [step for step in saves if _compact(_field(step, "key", 10)) == "${{steps.cache-plan.outputs." + cache_id.removesuffix("-cache") + "-key||steps." + cache_id + ".outputs.cache-primary-key}}"]
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
            require(_compact(env.get("CACHE_IDENTITY")) == "${{steps.cache-plan.outputs.build-key||steps.build-cache.outputs.cache-primary-key}}", name + ": metadata identity differs")
            require(_expression(_field(step, "if", 8)) == "always()", name + ": cache metadata must survive failed builds")
        uploads = [step for step in steps if (_field(step, "uses", 8) or "").startswith("actions/upload-artifact@") and _compact(_field(step, "name", 10)) == "release-diagnostics-${{matrix.target}}-attempt-${{github.run_attempt}}"]
        require(len(uploads) == 1, name + ": isolated release diagnostics upload missing")
        for upload in uploads:
            require(_field(upload, "continue-on-error", 8) == "true", name + ": diagnostics upload must be non-blocking")
            require(_expression(_field(upload, "if", 8)) == "always()", name + ": diagnostics upload must run always")
            paths = " ".join(_paths(upload))
            require("cache.json" in paths and ("cargo-timings" in paths or ".html" in paths), name + ": diagnostics must contain cache JSON and Cargo HTML")
            require("probe.json" in paths and "fingerprints.json" in paths, name + ": bounded probe/fingerprint reports missing")
            require(_paths(upload) == {"release-diagnostics/${{ matrix.target }}/" + filename for filename in ("cache.json", "probe.json", "fingerprints.json")}
                    | {"target/cargo-timings/*.html"}, name + ": diagnostic artifact must contain only bounded reports and Cargo HTML")
            require(_retention_within(upload, DIAGNOSTIC_RETENTION_DAYS),
                    name + ": diagnostic artifact retention must be explicit 1-3 days")
        products = []
        for upload in steps:
            if not (_field(upload, "uses", 8) or "").startswith("actions/upload-artifact@"):
                continue
            artifact_name = _compact(_field(upload, "name", 10))
            if artifact_name.startswith(("Usher-", "${{env.APP_NAME}}-")):
                products.append(upload)
                paths = _paths(upload)
                require(bool(paths) and all(not value.startswith(("release-diagnostics", "release-timings", "target/"))
                        and value.endswith((".tar.gz", ".zip", ".dmg")) for value in paths), name + ": product artifact contains diagnostic/non-package paths")
                require(_retention_within(upload, PRODUCT_RETENTION_DAYS),
                        name + ": product artifact retention must be explicit 1-7 days")
        require(len(products) == 1, name + ": expected one product artifact upload")
        diagnostic_steps = python + metadata + uploads + [step for step in steps if _field(step, "id", 8) == "cache-plan"]
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
    require(all(_retention_within(upload, DIAGNOSTIC_RETENTION_DAYS) for upload in report_uploads),
            "timing artifact retention must be explicit 1-3 days")
    return errors


# Mutate the real, tracked workflow; no dated/private evidence is required.
def candidate_fixture():
    return (ROOT / ".github/workflows/release.yml").read_text()


class ContractFixtures(unittest.TestCase):
    def setUp(self):
        self.candidate = candidate_fixture()

    def reject(self, before, after):
        self.assertTrue(before in self.candidate, "mutation pattern must exist in workflow fixture")
        self.assertTrue(validate_contract(self.candidate.replace(before, after, 1)))

    def test_candidate_passes(self):
        self.assertEqual(validate_contract(self.candidate), [])

    def test_artifact_retention_requires_explicit_positive_bounded_days(self):
        jobs = _jobs(self.candidate)
        for job_name, job in jobs.items():
            for step in _steps(job):
                if not (_field(step, "uses", 8) or "").startswith("actions/upload-artifact@"):
                    continue
                original = _field(step, "retention-days", 10)
                maximum = PRODUCT_RETENTION_DAYS if _compact(_field(step, "name", 10)).startswith(("Usher-", "${{env.APP_NAME}}-")) else DIAGNOSTIC_RETENTION_DAYS
                retention = "          retention-days: " + original
                for value in (None, "0", "-1", "1.5", "true", "abc", "${{ inputs.days }}", str(maximum + 1), "90"):
                    with self.subTest(job=job_name, artifact=_field(step, "name", 10), days=value):
                        changed = step.replace(retention, "" if value is None else "          retention-days: " + value)
                        self.assertTrue(any("retention" in error for error in validate_contract(self.candidate.replace(step, changed, 1))))

    def test_shorter_positive_artifact_retention_remains_valid(self):
        shorter = self.candidate.replace("retention-days: 7", "retention-days: 1").replace("retention-days: 3", "retention-days: 1")
        self.assertEqual(validate_contract(shorter), [])

    def test_product_upload_cannot_be_removed_to_bypass_retention(self):
        upload = next(step for step in _steps(_jobs(self.candidate)["build-macos"]) if _field(step, "name", 8) == "Upload Artifacts")
        self.assertTrue(any("product artifact upload" in error for error in validate_contract(self.candidate.replace(upload, "", 1))))

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
        restore = next(step for step in _steps(_jobs(self.candidate)["build-macos"]) if _field(step, "id", 8) == "build-cache")
        self.reject(restore, restore.replace("          restore-keys: |\n", "          restore-keys: |\n            broad-\n"))

    def test_compatible_coarse_fallback_is_required_and_ordered_after_strict(self):
        restore = next(step for step in _steps(_jobs(self.candidate)["build-macos"]) if _field(step, "id", 8) == "build-cache")
        strict, coarse = ("            " + value + "\n" for value in _restore_prefixes(restore))
        self.reject(coarse, "")
        self.reject(strict + coarse, coarse + strict)

    def test_coarse_fallback_must_not_drop_platform_toolchain_or_explicit_profile_feature(self):
        restore = next(step for step in _steps(_jobs(self.candidate)["build-macos"]) if _field(step, "id", 8) == "build-cache")
        coarse = "            " + _restore_prefixes(restore)[1] + "\n"
        for token in ("runner.os, ", "runner.arch, ", "matrix.target, ", ", steps.rust.outputs.cachekey", "release-default-"):
            with self.subTest(token=token):
                self.reject(coarse, coarse.replace(token, ""))

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
        self.reject("      - name: Install Python for diagnostics\n        continue-on-error: true", "      - name: Install Python for diagnostics")
        self.reject("        if: always()\n        continue-on-error: true", "        if: always()")
        report = _jobs(self.candidate)["report-timings"]
        self.reject(report, report.replace("    continue-on-error: true\n", "", 1))

    def test_production_failures_cannot_be_silenced(self):
        self.reject("        id: release-build\n", "        id: release-build\n        continue-on-error: true\n")
        self.reject("      - name: Create App Bundle\n", "      - name: Create App Bundle\n        continue-on-error: true\n")
        self.reject("  release:\n", "  release:\n    continue-on-error: true\n")
        self.reject("  build-windows:\n", "  build-windows:\n    continue-on-error: true\n")

    def test_product_and_release_must_be_excluded_from_cache_only(self):
        self.reject("      - name: Create App Bundle\n        if: ${{ !inputs.cache_only }}", "      - name: Create App Bundle")
        self.reject("if: ${{ !inputs.cache_only && startsWith(github.ref, 'refs/tags/') }}", "if: startsWith(github.ref, 'refs/tags/')")

    def test_locked_build_is_required(self):
        self.reject(' --timings --locked\n', ' --timings\n')

    def test_whole_target_cache_rejected(self):
        self.reject("            target/.rustc_info.json", "            target/")

    def test_cache_hit_must_not_skip_build(self):
        self.reject("        id: release-build\n", "        id: release-build\n        if: steps.build-cache.outputs.cache-hit != 'true'\n")

    def test_release_command_and_feature_changes_rejected(self):
        for before, after in (("--release", "--profile release-fast"), ("--timings", ""),
                              ('--target "${RELEASE_TARGET}"', '--features ui-components --target "${RELEASE_TARGET}"')):
            with self.subTest(after=after):
                self.reject(before, after)

    def test_release_incremental_policy_rejected(self):
        self.reject("CARGO_INCREMENTAL: 0", "CARGO_INCREMENTAL: 1")

    def test_failed_build_cache_save_rejected(self):
        self.reject("if: success() && steps.release-build.outcome == 'success'", "if: always()")
        self.reject("if: success() && steps.release-build.outcome == 'success'", "if: failure() || steps.release-build.outcome == 'success'")

    def test_save_after_packaging_rejected(self):
        save = next(step for step in _steps(_jobs(self.candidate)["build-macos"])
                    if _field(step, "uses", 8) == "actions/cache/save@v6.1.0")
        text = self.candidate.replace(save, "", 1)
        package = next(step for step in _steps(_jobs(text)["build-macos"]) if _field(step, "name", 8) == "Create App Bundle")
        self.assertTrue(validate_contract(text.replace(package, package + save, 1)))

    def test_registry_restore_must_keep_toolchain_boundary(self):
        restore = next(step for step in _steps(_jobs(self.candidate)["build-macos"]) if _field(step, "id", 8) == "registry-cache")
        self.reject(restore, restore.replace("runner.arch, ", ""))

    def test_failure_diagnostics_must_survive(self):
        upload = next(step for step in _steps(_jobs(self.candidate)["build-macos"]) if _field(step, "name", 8) == "Upload build diagnostics")
        self.reject(upload, upload.replace("if: always()", "if: success()"))

    def test_release_download_cannot_include_diagnostics(self):
        self.reject("pattern: ${{ env.APP_NAME }}-*", "pattern: '*'")

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
        self.reject("CACHE_IDENTITY: ${{ steps.cache-plan.outputs.build-key || steps.build-cache.outputs.cache-primary-key }}", "CACHE_IDENTITY: arbitrary")

    def test_probe_input_modes_guard_and_strict_planning_cannot_be_relaxed(self):
        self.reject("default: normal", "default: cold")
        self.reject("type: choice", "type: string")
        self.reject(PROBE_INPUT_SCRIPT.splitlines()[-1], "true")
        self.reject("continue-on-error: ${{ inputs.cache_probe == 'normal' }}", "continue-on-error: true")
        self.reject("RELEASE_SOURCE_SHA: ${{ github.sha }}", "RELEASE_SOURCE_SHA: fixed")

    def test_warm_verification_cannot_be_skipped_or_tolerated(self):
        self.reject("if: inputs.cache_probe == 'warm'", "if: false")
        self.reject("      - name: Verify warm cache exact\n", "      - name: Verify warm cache exact\n        continue-on-error: true\n")
        self.reject("run: python scripts/ci_release_probe.py verify-warm", "run: python scripts/ci_release_probe.py verify-warm || true")

    def test_probes_cannot_consume_normal_namespace_or_fallback(self):
        self.reject("${{ steps.cache-plan.outputs.namespace || 'release-build-v1' }}", "release-build-v1")
        restore = next(step for step in _steps(_jobs(self.candidate)["build-macos"]) if _field(step, "id", 8) == "build-cache")
        self.reject(_restore_prefixes(restore)[1], COARSE_BUILD_PREFIX)
        self.reject("if: inputs.cache_probe != 'cold'", "if: always()")

    def test_diagnostic_export_cannot_add_raw_logs_or_unbounded_directories(self):
        self.reject("            target/cargo-timings/*.html", "            target/cargo-timings/*.html\n            release-diagnostics/raw.log")
        self.reject("CARGO_INCREMENTAL: 0", "CARGO_INCREMENTAL: 0\n          CARGO_LOG: trace")
        self.reject(' --timings --locked\n', ' --timings --locked || true\n')

    def test_import_has_no_execution_side_effects(self):
        self.assertTrue(callable(validate_contract))


class RepositoryContract(unittest.TestCase):
    def test_current_release_workflow_contract(self):
        errors = validate_contract((ROOT / ".github/workflows/release.yml").read_text())
        self.assertEqual(errors, [], "\n".join(errors))


if __name__ == "__main__":
    unittest.main()
