"""Focused fail-closed tests for retained release artifact provenance."""

from copy import deepcopy
from pathlib import Path
import unittest

from release_plan import (
    EXPECTED_ARTIFACTS,
    EXPECTED_JOBS,
    _flatten_pages,
    _validate_event_ref,
    _validate_push_sha,
    validate_remote_tag,
    validate_reuse,
)


TAG = "v3.20.4-16"
SHA = "a" * 40
REPO = "BigStrongSun/ccswitchmulti"
RUN_ID = 44133452548


def fixture():
    run = {
        "id": RUN_ID,
        "repository": {"full_name": REPO},
        "head_repository": {"full_name": REPO},
        "event": "push",
        "path": ".github/workflows/release.yml",
        "head_branch": TAG,
        "head_sha": SHA,
        "status": "completed",
        "conclusion": "failure",
    }
    jobs = [
        {
            "name": name,
            "run_id": RUN_ID,
            "head_sha": SHA,
            "status": "completed",
            "conclusion": conclusion,
        }
        for name, conclusion in EXPECTED_JOBS.items()
    ]
    artifacts = [
        {
            "id": index + 200,
            "name": name,
            "expired": False,
            "workflow_run": {"id": RUN_ID, "head_sha": SHA},
        }
        for index, name in enumerate(sorted(EXPECTED_ARTIFACTS))
    ]
    return run, jobs, artifacts


class ReuseValidationTests(unittest.TestCase):
    def assert_rejected(self, run=None, jobs=None, artifacts=None, tag=TAG, source_sha=SHA, repo=REPO):
        base_run, base_jobs, base_artifacts = fixture()
        with self.assertRaises(ValueError):
            validate_reuse(
                tag,
                source_sha,
                repo,
                base_run if run is None else run,
                base_jobs if jobs is None else jobs,
                base_artifacts if artifacts is None else artifacts,
            )

    def test_actual_api_shape_is_accepted_and_ids_are_numerically_sorted(self):
        run, jobs, artifacts = fixture()
        self.assertEqual(validate_reuse(TAG, SHA, REPO, run, jobs, artifacts), "200,201,202,203")

    def test_paginated_api_pages_are_flattened_without_dropping_items(self):
        self.assertEqual(
            _flatten_pages([{"jobs": [{"id": 1}]}, {"jobs": [{"id": 2}]}], "jobs"),
            [{"id": 1}, {"id": 2}],
        )
        with self.assertRaises(ValueError):
            _flatten_pages([{"jobs": []}, {"missing": []}], "jobs")

    def test_invalid_tag_repository_event_and_workflow_are_rejected(self):
        run, jobs, artifacts = fixture()
        for tag in ("v03.20.4-16", "v3.20.4-16/other", "3.20.4-16"):
            with self.subTest(tag=tag):
                self.assert_rejected(run, jobs, artifacts, tag=tag)
        self.assert_rejected(run, jobs, artifacts, repo="attacker/ccswitchmulti")
        for key, value in (("event", "workflow_dispatch"), ("path", ".github/workflows/ci.yml")):
            changed = deepcopy(run)
            changed[key] = value
            with self.subTest(key=key):
                self.assert_rejected(changed, jobs, artifacts)
        _validate_event_ref(TAG, "push", f"refs/tags/{TAG}")
        _validate_event_ref(TAG, "workflow_dispatch", "refs/heads/main")
        for event, ref in (
            ("push", "refs/heads/main"),
            ("workflow_dispatch", f"refs/tags/{TAG}"),
            ("pull_request", "refs/heads/main"),
        ):
            with self.subTest(event=event, ref=ref), self.assertRaises(ValueError):
                _validate_event_ref(TAG, event, ref)

    def test_repository_and_head_repository_must_both_match(self):
        run, jobs, artifacts = fixture()
        for key in ("repository", "head_repository"):
            changed = deepcopy(run)
            changed[key]["full_name"] = "another-owner/ccswitchmulti"
            with self.subTest(key=key):
                self.assert_rejected(changed, jobs, artifacts)

    def test_run_must_be_failed_and_match_tag_source(self):
        run, jobs, artifacts = fixture()
        for key, value in (("status", "in_progress"), ("conclusion", "success")):
            changed = deepcopy(run)
            changed[key] = value
            with self.subTest(key=key):
                self.assert_rejected(changed, jobs, artifacts)
        changed = deepcopy(run)
        changed["head_sha"] = "b" * 40
        self.assert_rejected(changed, jobs, artifacts)
        changed = deepcopy(run)
        changed["head_branch"] = "v3.20.4-15"
        self.assert_rejected(changed, jobs, artifacts)

    def test_all_four_platform_jobs_must_succeed_and_mac_job_must_fail(self):
        run, jobs, artifacts = fixture()
        changed = deepcopy(jobs)
        changed[0]["conclusion"] = "failure"
        self.assert_rejected(run, changed, artifacts)
        changed = deepcopy(jobs[:-1])
        self.assert_rejected(run, changed, artifacts)

    def test_duplicate_or_unexpected_platform_jobs_are_rejected(self):
        run, jobs, artifacts = fixture()
        self.assert_rejected(run, jobs + [deepcopy(jobs[0])], artifacts)
        extra = deepcopy(jobs[0])
        extra["name"] = "release (ubuntu-latest)"
        self.assert_rejected(run, jobs + [extra], artifacts)

    def test_expired_artifact_is_rejected(self):
        run, jobs, artifacts = fixture()
        artifacts[0]["expired"] = True
        self.assert_rejected(run, jobs, artifacts)

    def test_duplicate_missing_or_unexpected_artifacts_are_rejected(self):
        run, jobs, artifacts = fixture()
        self.assert_rejected(run, jobs, artifacts[:-1])
        self.assert_rejected(run, jobs, artifacts + [deepcopy(artifacts[0])])
        unexpected = deepcopy(artifacts[0])
        unexpected["name"] = "release-assets-macOS-X64"
        self.assert_rejected(run, jobs, artifacts[:-1] + [unexpected])

    def test_artifact_run_id_and_head_sha_must_match(self):
        run, jobs, artifacts = fixture()
        changed = deepcopy(artifacts)
        changed[0]["workflow_run"]["id"] += 1
        self.assert_rejected(run, jobs, changed)
        changed = deepcopy(artifacts)
        changed[0]["workflow_run"]["head_sha"] = "b" * 40
        self.assert_rejected(run, jobs, changed)

    def test_moved_remote_tag_is_rejected(self):
        local_tag_object = "c" * 40
        source_sha = "a" * 40
        with self.assertRaises(ValueError):
            validate_remote_tag(
                local_tag_object,
                source_sha,
                {"object": {"sha": "d" * 40, "type": "tag"}},
            )
        with self.assertRaises(ValueError):
            validate_remote_tag(
                local_tag_object,
                source_sha,
                {"object": {"sha": local_tag_object, "type": "tag"}},
                {"object": {"sha": "b" * 40, "type": "commit"}},
            )
        _validate_push_sha("push", SHA, SHA)
        with self.assertRaises(ValueError):
            _validate_push_sha("push", SHA, "b" * 40)
        _validate_push_sha("workflow_dispatch", SHA, "b" * 40)

    def test_workflow_has_secret_log_guard_and_pins_product_checkouts(self):
        workflow = (Path(__file__).parents[2] / ".github/workflows/release.yml").read_text(
            encoding="utf-8"
        )
        self.assertNotIn("head -c 10", workflow.lower())
        self.assertNotIn("密钥前10", workflow)
        blocks = workflow.split("uses: actions/checkout@")
        refs = []
        for block in blocks[1:]:
            prefix = block.split("\n      - name:", 1)[0]
            for line in prefix.splitlines():
                if line.strip().startswith("ref:"):
                    refs.append(line.strip().split("ref:", 1)[1].strip())
                    break
        self.assertEqual(refs.count("${{ github.sha }}"), 1)
        self.assertGreaterEqual(refs.count("${{ needs.plan.outputs.sha }}"), 2)
        self.assertEqual(len(refs), 3)


if __name__ == "__main__":
    unittest.main()
