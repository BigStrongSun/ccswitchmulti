"""Classify a GitHub event's complete Git diff; uncertainty always means full CI."""

import json
import os
from pathlib import Path, PurePosixPath
import re
import subprocess


DOC_SUFFIXES = {".md", ".rst", ".txt", ".png", ".jpg", ".jpeg", ".gif", ".webp"}
SHA = re.compile(r"(?:[0-9a-f]{40}|[0-9a-f]{64})\Z")
RAW_HEADER = re.compile(
    rb":([0-7]{6}) ([0-7]{6}) [0-9a-f]+ [0-9a-f]+ ([AMD])\Z"
)


def git(*args, cwd=None):
    return subprocess.run(
        ["git", *args], cwd=cwd, check=True, capture_output=True
    ).stdout


def is_document(path):
    """Only prose at the root, or prose/raster images below docs/, is exempt."""
    if any(char in path for char in "\\\r\n"):
        return False
    parts = path.split("/")
    if any(part in {"", ".", ".."} for part in parts):
        return False
    if len(parts) == 1:
        return path == "LICENSE" or PurePosixPath(path).suffix == ".md"
    return parts[0] == "docs" and PurePosixPath(path).suffix in DOC_SUFFIXES


def classify_raw_diff(raw):
    """Parse --raw -z --no-renames, including old modes and deleted paths."""
    if not raw or not raw.endswith(b"\0"):
        return "full"
    fields = raw[:-1].split(b"\0")
    if len(fields) % 2:
        return "full"
    for header, raw_path in zip(fields[::2], fields[1::2]):
        match = RAW_HEADER.fullmatch(header)
        if not match:
            return "full"
        old_mode, new_mode, _ = match.groups()
        # Executables, symlinks, submodules, and unknown modes are never docs-only.
        if any(mode not in {b"000000", b"100644"} for mode in (old_mode, new_mode)):
            return "full"
        try:
            path = raw_path.decode("utf-8")
        except UnicodeDecodeError:
            return "full"
        if not is_document(path):
            return "full"
    return "docs"


def event_range(event_name, event, cwd=None):
    if event_name == "pull_request":
        base = event["pull_request"]["base"]["sha"]
        head = event["pull_request"]["head"]["sha"]
    elif event_name == "push":
        base, head = event["before"], event["after"]
    else:
        raise ValueError("Unsupported event")
    for revision in (base, head):
        if not isinstance(revision, str) or not SHA.fullmatch(revision) or set(revision) == {"0"}:
            raise ValueError("Missing or invalid revision")
        git("cat-file", "-e", revision + "^{commit}", cwd=cwd)
    if event_name == "pull_request":
        base = git("merge-base", base, head, cwd=cwd).decode("ascii").strip()
        if not SHA.fullmatch(base):
            raise ValueError("Invalid merge base")
    return base, head


def classify_event(event_name, event, cwd=None):
    try:
        base, head = event_range(event_name, event, cwd=cwd)
        raw = git(
            "diff", "--raw", "-z", "--no-renames", "--no-abbrev",
            "--no-ext-diff", "--no-textconv", base, head, "--", cwd=cwd,
        )
        return classify_raw_diff(raw)
    except (KeyError, TypeError, ValueError, OSError, subprocess.SubprocessError):
        return "full"


def main():
    scope = "full"
    try:
        event = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text(encoding="utf-8"))
        scope = classify_event(os.environ.get("GITHUB_EVENT_NAME"), event)
    except (KeyError, ValueError, OSError):
        pass
    print(f"CI scope: {scope}")
    with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
        output.write(f"scope={scope}\n")


if __name__ == "__main__":
    main()
