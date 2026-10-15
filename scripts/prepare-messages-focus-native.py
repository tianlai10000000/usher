#!/usr/bin/env python3
"""Prepare a disposable native Usher bundle with isolated synthetic Inbox data.

Launch the returned .app with the native UI driver. This script does not send
input, launch applications, access workstation settings or change system prefs.
"""

import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import plistlib
import re
import shutil
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--label", required=True)
    parser.add_argument("--locale", default="en")
    parser.add_argument("--count", type=int, default=1000)
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    if not binary.is_file() or not os.access(binary, os.X_OK):
        parser.error("--binary must be an executable regular file")
    if not re.fullmatch(r"[A-Za-z0-9_-]{1,64}", args.label):
        parser.error("--label must contain only ASCII letters, digits, _ or -")
    if not 1 <= args.count <= 1000:
        parser.error("--count must be between 1 and 1000")
    if not re.fullmatch(r"[A-Za-z0-9_-]{1,40}", args.locale):
        parser.error("invalid locale identifier")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    state = output / "state/default/run-0"
    (state / "messages").mkdir(parents=True)
    (state / "settings.toml").write_text(
        f'language = {json.dumps(args.locale)}\ntheme_mode = 0\n'
        'navigation_drawer_expanded = true\nui_sound_effects = false\n'
        '\n[update]\nauto_update = false\n'
        '\n[diagnostics]\napplication_console_logging = true\n'
        'application_file_logging = true\ntracing_logger_active = false\n'
        'terminal_verbose_logging = false\nmemory_diagnostics_logging = false\n',
        encoding="utf-8",
    )
    start = datetime.datetime(2026, 10, 15, tzinfo=datetime.timezone.utc)
    messages = [
        dict(
            id=f"ui120-{index:04}",
            title=f"UI120 message {index:04}",
            body="Synthetic focus regression fixture",
            source="UI120 fixture",
            channel="application",
            category="general",
            severity="info",
            created_at=(start + datetime.timedelta(seconds=index)).isoformat(),
            read=False,
            target=None,
            deduplication_key=None,
            metadata={},
        )
        for index in range(1, args.count + 1)
    ]
    (state / "messages/messages.json").write_text(json.dumps(messages), encoding="utf-8")
    bundle = output / f"Usher UI120 {args.label}.app"
    macos = bundle / "Contents/MacOS"
    resources = bundle / "Contents/Resources"
    macos.mkdir(parents=True)
    resources.mkdir()
    executable = macos / "Usher"
    shutil.copy2(binary, executable)
    executable.chmod(0o755)
    environment = {
        "USHER_BENCHMARK_OFFLINE": "1",
        "USHER_BENCHMARK_STATE_ROOT": str(output / "state"),
        "USHER_BENCHMARK_INSTANCE_NAMESPACE": f"ui120-{args.label}",
        "SLINT_BACKEND": "winit-femtovg",
    }
    (resources / "environment.json").write_text(json.dumps(environment), encoding="utf-8")
    # execve passes paths and environment as data. No shell interprets them, and
    # isolation is installed before application initialization can read Settings.
    launcher = macos / "launch"
    launcher.write_text(
        f"#!{sys.executable}\n"
        "import json, os\nfrom pathlib import Path\n"
        "contents = Path(__file__).resolve().parent.parent\n"
        "environment = {key: value for key, value in os.environ.items() if not key.startswith('USHER_')}\n"
        "environment.update(json.loads((contents / 'Resources/environment.json').read_text()))\n"
        "fd = os.open(contents.parent.parent / 'application.log', os.O_WRONLY | os.O_CREAT | os.O_APPEND, 0o600)\n"
        "os.dup2(fd, 1)\nos.dup2(fd, 2)\nos.close(fd)\n"
        "binary = str(contents / 'MacOS/Usher')\n"
        "os.execve(binary, [binary], environment)\n",
        encoding="utf-8",
    )
    launcher.chmod(0o755)
    identity = f"com.usher.ui120.{args.label.lower().replace('_', '-')}"
    with (bundle / "Contents/Info.plist").open("wb") as stream:
        plistlib.dump(
            dict(
                CFBundleExecutable="launch",
                CFBundleIdentifier=identity,
                CFBundleName=f"Usher UI120 {args.label}",
                CFBundlePackageType="APPL",
                NSHighResolutionCapable=True,
            ),
            stream,
        )
    with binary.open("rb") as stream:
        checksum = hashlib.file_digest(stream, "sha256").hexdigest()
    manifest = dict(
        bundle=str(bundle),
        bundle_id=identity,
        binary_source=str(binary),
        binary_sha256=checksum,
        locale=args.locale,
        messages=args.count,
        state=str(state),
        environment=environment,
    )
    (output / "fixture.json").write_text(json.dumps(manifest, indent=2), encoding="utf-8")
    print(json.dumps(manifest, indent=2))


if __name__ == "__main__":
    main()
