#!/usr/bin/env python3
"""Run the fixed Release build and export bounded, allowlisted fingerprint facts.

No raw child stdout/stderr is replayed or written. Console progress is constructed from
fixed status labels and allowlisted workspace package names; compiler prose is
suppressed in this explicit mode. Cargo's actual exit status remains blocking.
The default workflow mode still invokes Cargo directly with its full diagnostics.
"""

import argparse
from collections import Counter
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time

from ci_release_metrics import _write_text
from ci_release_probe import TARGETS


MAX_LINE_BYTES = 16 * 1024
MAX_TRACE_BYTES = 8 * 1024 * 1024
MAX_RECORDS = 128
MAX_COUNT = 1_000_000
FINGERPRINT_LOG = "cargo::core::compiler::fingerprint=info"
PACKAGES = frozenset({
    "usher", "usher-ai-review", "usher-fs", "usher-i18n", "usher-i18n-build",
    "usher-messages", "usher-preview", "usher-search", "usher-state-store",
    "usher-ui", "usher-ui-build", "usher-ui-components", "usher-ui-pr-review", "usher-ui-settings",
    "usher-virtual-list",
})
ANSI = re.compile(r"\x1b\[[0-?]*[ -/]*[@-~]")
LOG_HEADER = re.compile(r"\b(?:TRACE|DEBUG|INFO|WARN|ERROR)\b")
CARGO_HEADER = re.compile(r"^\s*(?:Compiling |Checking |Fresh |Finished |Downloading |Downloaded |Updating |Locking |Blocking |warning(?:\[|:)|error(?:\[|:)|note:)")
PACKAGE_ID = re.compile(r"(?:package_id=|fingerprint (?:error|dirty) for )([a-z0-9_-]+) v")
MTIMES = re.compile(r"reference_mtime: FileTime \{ seconds: (\d{1,20}), nanos: (\d{1,9}) \}.*?stale_mtime: FileTime \{ seconds: (\d{1,20}), nanos: (\d{1,9}) \}")


def _reason(text):
    for needles, reason in (
        (("ChangedFile", "stale: changed"), "changed_file"),
        (("StaleDepFingerprint", "UnitDependencyInfoChanged"), "dependency_fingerprint"),
        (("EnvVarChanged", "RerunIfEnvChanged", "env variable"), "environment_changed"),
        (("ProfileChanged", "RustflagsChanged", "FeaturesChanged", "PrecalculatedComponentsChanged"), "configuration_changed"),
        (("RerunIfChanged", "BuildScript"), "build_script_changed"),
        (("failed to read",), "missing_fingerprint"),
        (("fingerprint dirty for",), "dirty_unit"),
        (("fingerprint error for",), "fingerprint_error"),
        (("FileTime {",), "timestamp_evidence"),
    ):
        if any(needle in text for needle in needles):
            return reason
    return "other_trace"


class Fingerprints:
    def __init__(self):
        self.records = Counter()
        self.trace_bytes = 0
        self.trace_lines = 0
        self.suppressed_continuations = 0
        self.oversized_lines = 0
        self.truncated = False
        # Unknown prologues/format changes are private until an ordinary Cargo
        # diagnostic header begins a known console section.
        self.in_trace = True

    def consume(self, line):
        """Return true only for lines that may continue to the ordinary console."""
        clean = ANSI.sub("", line)
        trace = "fingerprint" in clean.casefold() or LOG_HEADER.search(clean)
        if trace:
            self.in_trace = True
            self.trace_lines = min(self.trace_lines + 1, MAX_COUNT)
            self.trace_bytes = min(self.trace_bytes + len(line.encode("utf-8")), MAX_TRACE_BYTES + 1)
            if self.trace_bytes > MAX_TRACE_BYTES:
                self.truncated = True
                return False
            found = PACKAGE_ID.search(clean)
            package = found[1] if found and found[1] in PACKAGES else "other"
            times = MTIMES.search(clean)
            mtime = None
            if times:
                a, b, c, d = map(int, times.groups())
                mtime = "stale_newer" if (c, d) > (a, b) else "not_newer"
            key = (package, _reason(clean), mtime)
            if key not in self.records and len(self.records) >= MAX_RECORDS:
                self.truncated = True
            else:
                self.records[key] = min(self.records[key] + 1, MAX_COUNT)
            return False
        if self.in_trace and not CARGO_HEADER.match(clean):
            self.suppressed_continuations = min(self.suppressed_continuations + 1, MAX_COUNT)
            return False
        self.in_trace = False
        return True

    def oversized(self):
        self.oversized_lines = min(self.oversized_lines + 1, MAX_COUNT)
        self.truncated = True
        self.in_trace = True

    def report(self):
        return {
            "schema_version": 1, "trace_lines": self.trace_lines,
            "trace_bytes_examined": min(self.trace_bytes, MAX_TRACE_BYTES),
            "suppressed_continuations": self.suppressed_continuations,
            "oversized_lines": self.oversized_lines, "truncated": self.truncated,
            "records": [
                {"package": package, "reason": reason, "mtime_comparison": mtime, "count": count}
                for (package, reason, mtime), count in sorted(self.records.items(), key=lambda pair: str(pair[0]))
            ],
        }


def cargo_command(target, jobs=None):
    if target not in TARGETS:
        raise ValueError("unsupported release target")
    command = ["cargo", "build", "--release", "--target", target]
    if jobs is not None:
        if isinstance(jobs, bool) or not isinstance(jobs, int) or not 1 <= jobs <= 512:
            raise ValueError("invalid release job count")
        command += ["--jobs", str(jobs)]
    return command + ["--timings", "--locked"]


def _console_status(line):
    """Do not replay tails, even if a trace payload imitates a Cargo header."""
    clean = ANSI.sub("", line).lstrip()
    compiling = re.match(r"Compiling ([a-z0-9_-]+)\s", clean)
    if compiling:
        package = compiling[1] if compiling[1] in PACKAGES else "dependency"
        return "Cargo compiling " + package + " (fingerprint mode)\n"
    if re.match(r"error(?:\[|:)", clean):
        return "Cargo compiler error (prose suppressed in fingerprint mode)\n"
    if re.match(r"warning(?:\[|:)", clean):
        return "Cargo compiler warning (prose suppressed in fingerprint mode)\n"
    if clean.startswith("Finished "):
        return "Cargo finished (fingerprint mode)\n"
    return None


def run_build(command, output, environ=None, stderr=None, popen=subprocess.Popen):
    environ = os.environ if environ is None else environ
    stderr = sys.stderr if stderr is None else stderr
    child_env = dict(environ)
    child_env["CARGO_LOG"] = FINGERPRINT_LOG
    collector = Fingerprints()
    started = time.monotonic()
    try:
        child = popen(command, env=child_env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    except OSError:
        print("Release build error: Cargo could not be started", file=stderr)
        return 127
    collector_failed = False
    try:
        with child.stderr as stream:
            while True:
                raw = stream.readline(MAX_LINE_BYTES + 1)
                if not raw:
                    break
                if len(raw) > MAX_LINE_BYTES:
                    while raw and not raw.endswith(b"\n"):
                        raw = stream.readline(MAX_LINE_BYTES + 1)
                    collector.oversized()
                    continue
                line = raw.decode("utf-8", errors="replace")
                try:
                    ordinary = not collector_failed and collector.consume(line)
                except Exception:
                    # A new Cargo log format/parser defect is a diagnostic
                    # failure. Consume subsequent stderr privately, wait for
                    # Cargo, and retain its status rather than building twice.
                    collector_failed = True
                    ordinary = False
                status = _console_status(line) if ordinary else None
                if status:
                    stderr.write(status)
                    stderr.flush()
        code = child.wait()
    except KeyboardInterrupt:
        child.terminate()
        try:
            child.wait(timeout=5)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait()
        code = 130
    report = collector.report()
    report.update(cargo_exit_code=code, collector_error=collector_failed,
                  elapsed_seconds=round(time.monotonic() - started, 6))
    try:
        _write_text(output, json.dumps(report, indent=2, allow_nan=False) + "\n")
    except (OSError, ValueError):
        # An export failure must never rerun Cargo, mask a compiler failure, or
        # fail an otherwise successful production build.
        print("Release diagnostics warning: fingerprint report unavailable", file=stderr)
    print("Cargo exit status: " + str(code), file=stderr)
    return code if code >= 0 else 128 - code


class _Parser(argparse.ArgumentParser):
    def error(self, message):
        self.exit(2, "Release build error: invalid command-line arguments\n")


def main(argv=None):
    parser = _Parser(description=__doc__)
    parser.add_argument("--target", required=True)
    parser.add_argument("--jobs", type=int)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args(argv)
    try:
        command = cargo_command(args.target, args.jobs)
    except ValueError as error:
        print("Release build error: " + str(error), file=sys.stderr)
        return 2
    return run_build(command, args.output)


if __name__ == "__main__":
    sys.exit(main())
