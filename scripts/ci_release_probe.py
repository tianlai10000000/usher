#!/usr/bin/env python3
"""Validate opt-in release cache probes; never dispatch jobs or run Cargo."""

import argparse
import json
import os
from pathlib import Path
import re
import sys

from ci_release_metrics import MetricsError, _write_text


TARGETS = {"aarch64-apple-darwin", "x86_64-apple-darwin", "x86_64-pc-windows-msvc"}


def _text(environ, name, pattern):
    value = environ.get(name, "")
    if not isinstance(value, str) or not re.fullmatch(pattern, value):
        raise MetricsError("invalid release probe identity field")
    return value


def probe_plan(environ):
    mode = environ.get("RELEASE_PROBE_MODE") or "normal"
    diagnostics = environ.get("RELEASE_DIAGNOSTICS") or "false"
    if mode not in ("normal", "cold", "warm") or diagnostics not in ("true", "false"):
        raise MetricsError("invalid release probe mode")
    probe_id = environ.get("RELEASE_PROBE_ID") or ""
    record = {"schema_version": 1, "mode": mode, "fingerprint": diagnostics == "true" or mode != "normal",
              "probe_id": None, "namespace": "", "build_key": "", "registry_key": ""}
    if mode == "normal":
        if probe_id:
            raise MetricsError("normal mode does not accept a probe id")
        return record
    if (environ.get("RELEASE_EVENT") != "workflow_dispatch"
            or environ.get("RELEASE_CACHE_ONLY") != "true"
            or not environ.get("RELEASE_DEFAULT_BRANCH")
            or environ.get("RELEASE_REF") != "refs/heads/" + environ["RELEASE_DEFAULT_BRANCH"]):
        raise MetricsError("cache probes require a manual default-branch cache-only run")
    if mode == "cold":
        if probe_id:
            raise MetricsError("cold mode assigns a new probe id; leave the input empty")
        run = _text(environ, "RELEASE_RUN_ID", r"[1-9][0-9]{0,19}")
        attempt = _text(environ, "RELEASE_RUN_ATTEMPT", r"[1-9][0-9]{0,3}")
        probe_id = run + "-" + attempt
    elif not re.fullmatch(r"[1-9][0-9]{0,19}-[1-9][0-9]{0,3}", probe_id):
        raise MetricsError("warm mode requires the prior cold run-attempt probe id")
    target = _text(environ, "RELEASE_TARGET", r"[a-z0-9_-]+")
    if target not in TARGETS:
        raise MetricsError("unsupported release target")
    host_os = _text(environ, "RELEASE_RUNNER_OS", r"(?:macOS|Windows)")
    host_arch = _text(environ, "RELEASE_RUNNER_ARCH", r"(?:ARM64|X64)")
    rust = _text(environ, "RELEASE_RUST_KEY", r"[A-Za-z0-9_-]{1,64}")
    config = _text(environ, "RELEASE_CONFIGURATION_HASH", r"[a-f0-9]{64}")
    registry = _text(environ, "RELEASE_REGISTRY_HASH", r"[a-f0-9]{64}")
    source = _text(environ, "RELEASE_SOURCE_SHA", r"[a-f0-9]{40}")
    namespace = "release-probe-v1-" + probe_id
    record.update(probe_id=probe_id, namespace=namespace, source_sha=source, target=target,
                  build_key=f"{namespace}-{host_os}-{host_arch}-{target}-{rust}-release-default-{config}-{source}",
                  registry_key=f"{namespace}-{host_os}-{host_arch}-{rust}-{registry}")
    return record


def verify_warm(environ):
    for prefix in ("BUILD", "REGISTRY"):
        if (environ.get(prefix + "_CACHE_HIT") != "true"
                or environ.get(prefix + "_CACHE_OUTCOME") != "success"):
            raise MetricsError("warm probe requires successful exact registry and build cache restores")


class _Parser(argparse.ArgumentParser):
    def error(self, message):
        self.exit(2, "Release probe error: invalid command-line arguments\n")


def main(argv=None):
    parser = _Parser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    plan = commands.add_parser("plan")
    plan.add_argument("--output", type=Path, required=True)
    commands.add_parser("verify-warm")
    args = parser.parse_args(argv)
    try:
        if args.command == "verify-warm":
            verify_warm(os.environ)
        else:
            record = probe_plan(os.environ)
            _write_text(args.output, json.dumps(record, indent=2, allow_nan=False) + "\n")
            output = os.environ.get("GITHUB_OUTPUT")
            if not output:
                raise MetricsError("GitHub step output path is unavailable")
            with Path(output).open("a", encoding="utf-8") as stream:
                for key in ("namespace", "build_key", "registry_key", "fingerprint"):
                    value = str(record[key]).lower() if isinstance(record[key], bool) else record[key]
                    stream.write(key.replace("_", "-") + "=" + value + "\n")
    except MetricsError as error:
        print("Release probe error: " + str(error), file=sys.stderr)
        return 2
    except OSError:
        print("Release probe error: probe file could not be written", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())
