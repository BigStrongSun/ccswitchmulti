"""Validate immutable release sources and optional retained build artifacts."""

import json
import os
import re
import subprocess
import sys


_IDENT = r"(?:0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)"
SEMVER_TAG = re.compile(
    rf"v(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)"
    rf"(?:-{_IDENT}(?:\.{_IDENT})*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?\Z"
)
SHA = re.compile(r"(?:[0-9a-f]{40}|[0-9a-f]{64})\Z")
REPOSITORY = re.compile(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+\Z")
RUN_ID = re.compile(r"[1-9][0-9]{0,19}\Z")

STANDARD_MATRIX = {
    "include": [
        {"os": "windows-2022"},
        {"os": "windows-2022", "arch": "arm64"},
        {"os": "ubuntu-22.04"},
        {"os": "ubuntu-22.04-arm", "arch": "arm64"},
        {"os": "macos-15-intel"},
    ]
}
RECOVERY_MATRIX = {"include": [{"os": "macos-15-intel"}]}

EXPECTED_JOBS = {
    "release (windows-2022)": "success",
    "release (windows-2022, arm64)": "success",
    "release (ubuntu-22.04)": "success",
    "release (ubuntu-22.04-arm, arm64)": "success",
    "release (macos-15-intel)": "failure",
}
EXPECTED_ARTIFACTS = {
    "release-assets-Windows-X64",
    "release-assets-Windows-arm64",
    "release-assets-Linux-X64",
    "release-assets-Linux-arm64",
}


def _require(condition, message):
    if not condition:
        raise ValueError(message)


def _is_sha(value):
    return isinstance(value, str) and SHA.fullmatch(value) is not None


def validate_reuse(tag, source_sha, repo, run, jobs, artifacts):
    """Return sorted artifact IDs only when the failed source run is reusable."""
    _require(isinstance(tag, str) and SEMVER_TAG.fullmatch(tag), "invalid release tag")
    _require(_is_sha(source_sha), "invalid source SHA")
    _require(isinstance(repo, str) and REPOSITORY.fullmatch(repo), "invalid repository")
    source_sha = source_sha.lower()
    _require(isinstance(run, dict), "missing source run")
    run_id = run.get("id")
    _require(type(run_id) is int and run_id > 0, "invalid source run ID")
    _require(
        isinstance(run.get("repository"), dict)
        and isinstance(run["repository"].get("full_name"), str)
        and run["repository"]["full_name"].casefold() == repo.casefold(),
        "source run repository mismatch",
    )
    _require(
        isinstance(run.get("head_repository"), dict)
        and isinstance(run["head_repository"].get("full_name"), str)
        and run["head_repository"]["full_name"].casefold() == repo.casefold(),
        "source head repository mismatch",
    )
    _require(run.get("event") == "push", "source run is not a tag push")
    _require(run.get("path") == ".github/workflows/release.yml", "source workflow mismatch")
    _require(run.get("head_branch") == tag, "source branch/tag mismatch")
    _require(
        _is_sha(run.get("head_sha")) and run["head_sha"].lower() == source_sha,
        "source run SHA mismatch",
    )
    _require(
        run.get("status") == "completed" and run.get("conclusion") == "failure",
        "source run is not a completed failure",
    )
    _require(isinstance(jobs, list), "missing source jobs")

    release_jobs = {}
    for job in jobs:
        _require(isinstance(job, dict), "invalid source job")
        name = job.get("name")
        _require(isinstance(name, str), "source job missing name")
        normalized_name = name.strip().casefold()
        if normalized_name.startswith("release ("):
            _require(normalized_name not in release_jobs, "duplicate platform job")
            release_jobs[normalized_name] = job
    _require(set(release_jobs) == set(EXPECTED_JOBS), "unexpected or missing platform job")
    for name, expected_conclusion in EXPECTED_JOBS.items():
        job = release_jobs[name]
        _require(job.get("run_id") == run_id, "platform job run ID mismatch")
        _require(
            _is_sha(job.get("head_sha")) and job["head_sha"].lower() == source_sha,
            "platform job SHA mismatch",
        )
        _require(
            job.get("status") == "completed"
            and job.get("conclusion") == expected_conclusion,
            "platform job result mismatch",
        )

    _require(isinstance(artifacts, list), "missing source artifacts")
    seen_names = set()
    ids = []
    for artifact in artifacts:
        _require(isinstance(artifact, dict), "invalid source artifact")
        name = artifact.get("name")
        _require(isinstance(name, str) and name in EXPECTED_ARTIFACTS, "unexpected artifact")
        _require(name not in seen_names, "duplicate artifact name")
        seen_names.add(name)
        _require(artifact.get("expired") is False, "expired artifact")
        artifact_id = artifact.get("id")
        _require(type(artifact_id) is int and artifact_id > 0, "invalid artifact ID")
        workflow_run = artifact.get("workflow_run")
        _require(
            isinstance(workflow_run, dict)
            and workflow_run.get("id") == run_id
            and isinstance(workflow_run.get("head_sha"), str)
            and workflow_run["head_sha"].lower() == source_sha,
            "artifact source mismatch",
        )
        ids.append(artifact_id)
    _require(seen_names == EXPECTED_ARTIFACTS, "missing platform artifact")
    return ",".join(str(value) for value in sorted(ids))


def _git_text(*args):
    result = subprocess.run(
        ["git", *args], check=True, capture_output=True, text=True, encoding="utf-8"
    )
    return result.stdout.strip()


def _gh_json(*args):
    result = subprocess.run(
        ["gh", "api", *args], check=True, capture_output=True, text=True, encoding="utf-8"
    )
    return json.loads(result.stdout)


def _flatten_pages(payload, key):
    _require(isinstance(payload, list) and payload, f"missing paginated {key}")
    flattened = []
    for page in payload:
        _require(isinstance(page, dict) and isinstance(page.get(key), list), f"invalid {key} page")
        flattened.extend(page[key])
    return flattened


def validate_remote_tag(local_tag_object, source_sha, remote_ref, remote_tag=None):
    """Require the GitHub ref and its optional annotated target to match local Git."""
    _require(_is_sha(local_tag_object) and _is_sha(source_sha), "invalid local tag SHA")
    remote_object = remote_ref.get("object") if isinstance(remote_ref, dict) else None
    _require(
        isinstance(remote_object, dict)
        and isinstance(remote_object.get("sha"), str)
        and remote_object["sha"].lower() == local_tag_object.lower(),
        "local and remote tag objects differ",
    )
    if remote_object.get("type") == "commit":
        _require(remote_object["sha"].lower() == source_sha.lower(), "remote tag commit differs")
    elif remote_object.get("type") == "tag":
        target = remote_tag.get("object") if isinstance(remote_tag, dict) else None
        _require(
            isinstance(target, dict)
            and target.get("type") == "commit"
            and isinstance(target.get("sha"), str)
            and target["sha"].lower() == source_sha.lower(),
            "remote annotated tag target differs",
        )
    else:
        raise ValueError("unsupported remote tag object")


def _validate_event_ref(tag, event_name, github_ref):
    _require(isinstance(tag, str) and SEMVER_TAG.fullmatch(tag), "invalid release tag")
    if event_name == "push":
        _require(github_ref == f"refs/tags/{tag}", "push ref is not the requested tag")
    elif event_name == "workflow_dispatch":
        _require(github_ref == "refs/heads/main", "manual dispatch must run from main")
    else:
        raise ValueError("unsupported workflow event")


def _resolve_tag(repo, tag):
    tag_ref = f"refs/tags/{tag}"
    source_sha = _git_text("rev-parse", f"{tag_ref}^{{commit}}").lower()
    local_tag_object = _git_text("rev-parse", tag_ref).lower()
    _require(_is_sha(source_sha) and _is_sha(local_tag_object), "invalid local tag object")

    remote_ref = _gh_json(f"repos/{repo}/git/ref/tags/{tag}")
    remote_object = remote_ref.get("object") if isinstance(remote_ref, dict) else None
    remote_tag = None
    if isinstance(remote_object, dict) and remote_object.get("type") == "tag":
        remote_tag = _gh_json(f"repos/{repo}/git/tags/{local_tag_object}")
    validate_remote_tag(local_tag_object, source_sha, remote_ref, remote_tag)
    return source_sha, local_tag_object


def _validate_push_sha(event_name, source_sha, github_sha):
    if event_name == "push":
        _require(_is_sha(github_sha) and github_sha.lower() == source_sha, "push SHA differs from tag")


def _fetch_reuse_data(repo, run_id):
    run = _gh_json(f"repos/{repo}/actions/runs/{run_id}")
    jobs_pages = _gh_json(
        "--paginate", "--slurp", f"repos/{repo}/actions/runs/{run_id}/jobs?per_page=100"
    )
    artifacts_pages = _gh_json(
        "--paginate", "--slurp", f"repos/{repo}/actions/runs/{run_id}/artifacts?per_page=100"
    )
    return run, _flatten_pages(jobs_pages, "jobs"), _flatten_pages(artifacts_pages, "artifacts")


def _write_outputs(result, output_path):
    matrix = json.dumps(result["matrix"], separators=(",", ":"), sort_keys=True)
    lines = [
        f"sha={result['sha']}",
        f"tag_object={result['tag_object']}",
        f"matrix={matrix}",
        f"artifact_ids={result['artifact_ids']}",
    ]
    with open(output_path, "a", encoding="utf-8", newline="\n") as output:
        output.write("\n".join(lines) + "\n")


def main():
    try:
        tag = os.environ.get("RELEASE_TAG", "")
        repo = os.environ.get("GITHUB_REPOSITORY", "")
        _require(isinstance(repo, str) and REPOSITORY.fullmatch(repo), "invalid repository")
        _validate_event_ref(tag, os.environ.get("GITHUB_EVENT_NAME"), os.environ.get("GITHUB_REF"))
        source_sha, tag_object = _resolve_tag(repo, tag)
        event_name = os.environ.get("GITHUB_EVENT_NAME")
        _validate_push_sha(event_name, source_sha, os.environ.get("GITHUB_SHA"))

        run_id = os.environ.get("REUSE_RUN_ID", "")
        if run_id:
            _require(RUN_ID.fullmatch(run_id), "invalid reuse run ID")
            run, jobs, artifacts = _fetch_reuse_data(repo, run_id)
            artifact_ids = validate_reuse(tag, source_sha, repo, run, jobs, artifacts)
            matrix = RECOVERY_MATRIX
        else:
            artifact_ids = ""
            matrix = STANDARD_MATRIX

        result = {
            "sha": source_sha,
            "tag_object": tag_object,
            "matrix": matrix,
            "artifact_ids": artifact_ids,
        }
        output_path = os.environ.get("GITHUB_OUTPUT")
        _require(isinstance(output_path, str) and output_path, "missing workflow output path")
        _write_outputs(result, output_path)
        print(json.dumps(result, separators=(",", ":"), sort_keys=True))
        return 0
    except (KeyError, OSError, ValueError, subprocess.SubprocessError, json.JSONDecodeError):
        print("release plan validation failed", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
