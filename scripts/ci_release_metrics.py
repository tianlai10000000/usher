#!/usr/bin/env python3
"""Collect allowlisted release-cache metadata and local GitHub jobs API timings.

No network requests or commands are executed. ``cache`` reads only the eight
explicit environment names below. ``jobs`` accepts one API page or the page
array produced by ``gh api --paginate --slurp``. Prefer a current-attempt jobs
endpoint together with ``--attempt``; older API payloads omit job run_attempt.
"""

import argparse
from datetime import datetime
import html
import json
import os
from pathlib import Path
import re
import sys
import tempfile


class MetricsError(ValueError):
    """An invalid report input; messages must not contain raw input values."""


def _optional_text(value, field):
    if value is None or value == "":
        return None
    if not isinstance(value, str):
        raise MetricsError(f"{field} must be text or null")
    return value


def _cache_record(environ, prefix):
    raw_hit = environ.get(f"{prefix}_CACHE_HIT", "")
    if raw_hit not in ("true", "false", "", None):
        raise MetricsError(f"{prefix}_CACHE_HIT must be exactly true, false, or empty")
    hit = {"true": True, "false": False, "": None, None: None}[raw_hit]
    matched_key = _optional_text(
        environ.get(f"{prefix}_CACHE_MATCHED_KEY"), f"{prefix}_CACHE_MATCHED_KEY"
    )
    outcome = _optional_text(environ.get(f"{prefix}_CACHE_OUTCOME"), f"{prefix}_CACHE_OUTCOME")
    if outcome not in (None, "success", "failure", "cancelled", "skipped"):
        raise MetricsError(f"{prefix}_CACHE_OUTCOME has an unsupported outcome")
    if outcome in ("failure", "cancelled"):
        result = outcome
    elif outcome == "skipped":
        result = "not_run"
    elif hit is True:
        result = "exact"
    elif matched_key is not None:
        result = "fallback"
    elif hit is False or outcome == "success":
        result = "miss"
    else:
        result = "unknown"
    return {
        "cache_hit": hit,
        "matched_key": matched_key,
        "outcome": outcome,
        "result": result,
    }


def cache_metadata(environ=None):
    """Read named fields only; an empty hit is a miss only after successful restore."""
    environ = os.environ if environ is None else environ
    return {
        "schema_version": 1,
        "release_target": _optional_text(environ.get("RELEASE_TARGET"), "RELEASE_TARGET"),
        "cache_identity": _optional_text(environ.get("CACHE_IDENTITY"), "CACHE_IDENTITY"),
        "registry": _cache_record(environ, "REGISTRY"),
        "build": _cache_record(environ, "BUILD"),
    }


def _positive_id(value, field, optional=False):
    if value is None and optional:
        return None
    if isinstance(value, bool) or not isinstance(value, int) or value < 1:
        raise MetricsError(f"{field} must be a positive integer")
    return value


_TIMESTAMP = re.compile(
    r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{1,6})?"
    r"(?:Z|[+-](?:[01]\d|2[0-3]):[0-5]\d)\Z"
)


def _timestamp(value, field):
    text = _optional_text(value, field)
    if text is None:
        return None, None
    if not _TIMESTAMP.fullmatch(text):
        raise MetricsError(f"{field} must be an ISO 8601 timestamp with timezone")
    try:
        parsed = datetime.fromisoformat(text.replace("Z", "+00:00"))
    except ValueError:
        raise MetricsError(f"{field} contains an invalid timestamp") from None
    # GitHub can serialize an unstarted/skipped job's Go zero time instead of
    # null. It is missing timing evidence, including when only one end is zero.
    if parsed.replace(tzinfo=None) == datetime.min and parsed.utcoffset().total_seconds() == 0:
        return None, None
    return text, parsed


def _timed_fields(item, kind):
    if not isinstance(item, dict):
        raise MetricsError(f"{kind} must be an object")
    started, start = _timestamp(item.get("started_at"), f"{kind} started_at")
    completed, end = _timestamp(item.get("completed_at"), f"{kind} completed_at")
    duration = None
    if start is not None and end is not None:
        duration = (end - start).total_seconds()
        if duration < 0:
            raise MetricsError(f"{kind} completed_at precedes started_at")
    return {
        "name": _optional_text(item.get("name"), f"{kind} name"),
        "status": _optional_text(item.get("status"), f"{kind} status"),
        "conclusion": _optional_text(item.get("conclusion"), f"{kind} conclusion"),
        "started_at": started,
        "completed_at": completed,
        "duration_seconds": duration,
    }


def _steps(items):
    if items is None:
        return []
    if not isinstance(items, list):
        raise MetricsError("job steps must be an array")
    result = []
    identified = {}
    for item in items:
        fields = _timed_fields(item, "step")
        step = {
            "id": _positive_id(item.get("id"), "step id", optional=True),
            "number": _positive_id(item.get("number"), "step number", optional=True),
            **fields,
        }
        identity = ("id", step["id"]) if step["id"] is not None else ("number", step["number"])
        if identity[1] is not None and identity in identified:
            if identified[identity] != step:
                raise MetricsError("duplicate step identity has conflicting timing fields")
            continue
        if identity[1] is not None:
            identified[identity] = step
        result.append(step)
    return result


def jobs_metadata(payload, attempt=None):
    """Project timing fields, deduplicate IDs, and keep one attempt's jobs only."""
    if attempt is not None:
        _positive_id(attempt, "attempt")
    pages = payload if isinstance(payload, list) else [payload]
    if not pages:
        raise MetricsError("jobs response contains no pages")
    jobs = {}
    totals = set()
    for page in pages:
        if not isinstance(page, dict) or not isinstance(page.get("jobs"), list):
            raise MetricsError("each jobs response page must contain a jobs array")
        if "total_count" in page:
            total = page["total_count"]
            if isinstance(total, bool) or not isinstance(total, int) or total < 0:
                raise MetricsError("total_count must be a nonnegative integer")
            totals.add(total)
        for item in page["jobs"]:
            fields = _timed_fields(item, "job")
            job = {
                "id": _positive_id(item.get("id"), "job id"),
                **fields,
                "run_attempt": _positive_id(item.get("run_attempt"), "job run_attempt", optional=True),
                "steps": _steps(item.get("steps")),
            }
            if job["id"] in jobs and jobs[job["id"]] != job:
                raise MetricsError("duplicate job id has conflicting timing fields")
            jobs[job["id"]] = job
    if len(totals) > 1:
        raise MetricsError("jobs pages disagree on total_count")
    if totals and len(jobs) != next(iter(totals)):
        raise MetricsError("jobs pages do not contain total_count unique jobs")

    reported_attempts = {job["run_attempt"] for job in jobs.values() if job["run_attempt"] is not None}
    if reported_attempts and any(job["run_attempt"] is None for job in jobs.values()):
        raise MetricsError("jobs mix reported and missing run_attempt")
    if reported_attempts:
        selected = attempt if attempt is not None else max(reported_attempts)
        if selected not in reported_attempts:
            raise MetricsError("requested attempt is absent from the jobs response")
        selection = "explicit" if attempt is not None else "highest_reported"
    else:
        selected = attempt
        selection = "explicit_endpoint_scope" if attempt is not None else "endpoint_scope"
    return {
        "schema_version": 1,
        "run_attempt": selected,
        "attempt_selection": selection,
        "jobs": [
            job for job in jobs.values()
            if (not reported_attempts or job["run_attempt"] == selected)
            and job["name"] != "Release timing report"
        ],
    }


def _markdown(value):
    if value is None:
        return "unknown"
    text = " ".join(str(value).split())
    text = re.sub(r"[\x00-\x1f\x7f]", " ", text)
    return re.sub(r"([\\`*_{}\[\]()#+\-.!|])", r"\\\1", html.escape(text, quote=True))


def _duration(value):
    return "unknown" if value is None else f"{value:.3f}"


def render_summary(report):
    lines = [
        "# Release timing report",
        "",
        f"Run attempt: {_markdown(report['run_attempt'])}. Selection: {_markdown(report['attempt_selection'])}.",
        "",
        "Durations use reported start/end timestamps. Missing times remain unknown.",
        "Jobs can run in parallel; their durations must not be added as workflow wall time.",
        "The Release timing report job itself is excluded.",
    ]
    if report["attempt_selection"].endswith("endpoint_scope"):
        lines += [
            "",
            "Job payloads omit run_attempt; attempt scope relies on the caller's current-attempt API endpoint.",
        ]
    lines += [
        "",
        "| Job ID | Job | Status | Conclusion | Started | Completed | Seconds |",
        "| --- | --- | --- | --- | --- | --- | ---: |",
    ]
    for job in report["jobs"]:
        cells = [_markdown(job[key]) for key in ("id", "name", "status", "conclusion", "started_at", "completed_at")]
        lines.append("| " + " | ".join(cells + [_duration(job["duration_seconds"])]) + " |")
    if not report["jobs"]:
        lines += ["", "No other jobs were reported for this attempt."]
    for job in report["jobs"]:
        lines += [
            "", f"## {_markdown(job['name'])} (job {_markdown(job['id'])})", "",
            "| Step | Name | Status | Conclusion | Started | Completed | Seconds |",
            "| --- | --- | --- | --- | --- | --- | ---: |",
        ]
        for step in job["steps"]:
            identity = step["number"] if step["number"] is not None else step["id"]
            cells = [_markdown(identity)] + [
                _markdown(step[key]) for key in ("name", "status", "conclusion", "started_at", "completed_at")
            ]
            lines.append("| " + " | ".join(cells + [_duration(step["duration_seconds"])]) + " |")
        if not job["steps"]:
            lines += ["", "No step timing fields were reported."]
    lines += [
        "", "## Cache and compilation evidence", "",
        "The jobs API does not report cache hits or Cargo compilation-unit timings.",
        "Inspect each release target's artifact for cache.json (restore outcome, exact/fallback/miss)",
        "and its Cargo timing HTML. Unreported cache metadata is unknown, not an exact hit.",
        "",
    ]
    return "\n".join(lines)


def _write_text(path, text):
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", dir=path.parent,
                                         prefix=".ci-release-metrics-", delete=False) as stream:
            temporary = Path(stream.name)
            stream.write(text)
        os.replace(temporary, path)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def _json_text(value):
    return json.dumps(value, ensure_ascii=False, indent=2, allow_nan=False) + "\n"


def _attempt_argument(value):
    try:
        attempt = int(value)
        _positive_id(attempt, "attempt")
    except (ValueError, MetricsError):
        raise argparse.ArgumentTypeError("must be a positive integer") from None
    return attempt


class _ArgumentParser(argparse.ArgumentParser):
    def error(self, message):
        # argparse's default diagnostics echo unknown argument values. Keep the
        # CLI failure path as private as JSON/metadata validation failures.
        self.print_usage(sys.stderr)
        self.exit(2, "Release metrics error: invalid command-line arguments; see --help\n")


def main(argv=None):
    parser = _ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    cache = commands.add_parser("cache", help="write named release cache environment fields")
    cache.add_argument("--output", type=Path, required=True)
    jobs = commands.add_parser("jobs", help="summarize local jobs API pages without network access")
    jobs.add_argument("--input", type=Path, required=True)
    jobs.add_argument("--output-dir", type=Path, required=True)
    jobs.add_argument("--attempt", type=_attempt_argument)
    args = parser.parse_args(argv)
    try:
        if args.command == "cache":
            _write_text(args.output, _json_text(cache_metadata()))
        else:
            try:
                payload = json.loads(args.input.read_text(encoding="utf-8"))
            except (json.JSONDecodeError, UnicodeError):
                raise MetricsError("input is not valid UTF-8 jobs JSON") from None
            report = jobs_metadata(payload, args.attempt)
            _write_text(args.output_dir / "timings.json", _json_text(report))
            _write_text(args.output_dir / "summary.md", render_summary(report))
    except MetricsError as error:
        print(f"Release metrics error: {error}", file=sys.stderr)
        return 2
    except OSError:
        print("Release metrics error: report file could not be read or written", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())
