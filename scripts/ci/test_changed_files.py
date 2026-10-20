"""Behavior tests with real Git commits, plus malformed/raw-mode fixtures."""

import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

from changed_files import classify_event, classify_raw_diff, is_document


class RawDiffTests(unittest.TestCase):
    def raw(self, path, old="100644", new="100644", status="M"):
        return f":{old} {new} {'a' * 40} {'b' * 40} {status}\0{path}\0".encode()

    def test_document_allowlist(self):
        for path in ("README.md", "README_ZH.md", "LICENSE", "docs/guide.md", "docs/截图.png"):
            with self.subTest(path=path):
                self.assertTrue(is_document(path))
        for path in (
            "src/help.md", "assets/icon.png", ".github/workflows/ci.yml",
            "docs/example.py", "docs/example.rs", "docs/example.ts", "docs/install.sh",
            "docs/install.ps1", "docs/example.mdx", "docs/script.svg", "requirements.txt",
            "docs/../src/help.md", "docs//help.md", "docs/new\nname.md", "docs\\help.md",
        ):
            with self.subTest(path=path):
                self.assertFalse(is_document(path))

    def test_normal_doc_edit_add_delete(self):
        for old, new, status in (
            ("100644", "100644", "M"), ("000000", "100644", "A"), ("100644", "000000", "D"),
        ):
            self.assertEqual(classify_raw_diff(self.raw("docs/a.md", old, new, status)), "docs")

    def test_executable_symlink_and_submodule_are_full(self):
        for old, new in (("100644", "100755"), ("100755", "000000"), ("120000", "120000"), ("160000", "160000")):
            self.assertEqual(classify_raw_diff(self.raw("docs/a.md", old, new)), "full")

    def test_empty_or_malformed_diff_is_full(self):
        for raw in (b"", b"junk", b"junk\0x\0", self.raw("docs/a.md")[:-1], self.raw("docs/a.md") + b"extra\0", self.raw("docs/a.md", status="R100")):
            with self.subTest(raw=raw):
                self.assertEqual(classify_raw_diff(raw), "full")

    def test_invalid_utf8_is_full(self):
        self.assertEqual(classify_raw_diff(self.raw("docs/a.md").replace(b"a.md", b"\xff.md")), "full")

    def test_mixed_diff_is_full(self):
        self.assertEqual(classify_raw_diff(self.raw("docs/a.md") + self.raw("src/a.ts")), "full")


class GitDiffTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="ci-scope-")
        self.addCleanup(self.temp.cleanup)
        self.repo = Path(self.temp.name)
        self.git("init", "-q")
        self.git("config", "core.autocrlf", "false")
        self.write("docs/guide.md", "guide\n")
        self.write("docs/空 格.md", "guide\n")
        self.write("src/app.ts", "export const value = 1;\n")
        self.base = self.commit()

    def git(self, *args):
        return subprocess.run(
            ["git", "-c", "user.name=CI Scope Tests", "-c", "user.email=ci-scope@example.invalid",
             "-c", "commit.gpgsign=false", "-c", "core.hooksPath=.no-hooks", *args],
            cwd=self.repo, check=True, capture_output=True,
        ).stdout.decode().strip()

    def write(self, path, contents):
        target = self.repo / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(contents, encoding="utf-8")

    def commit(self):
        self.git("add", "--all")
        self.git("commit", "-qm", "fixture")
        return self.git("rev-parse", "HEAD")

    def classify(self, head, base=None, event_name="push"):
        base = base or self.base
        event = {"before": base, "after": head} if event_name == "push" else {
            "pull_request": {"base": {"sha": base}, "head": {"sha": head}}
        }
        return classify_event(event_name, event, cwd=self.repo)

    def test_docs_only_and_unicode_spaces(self):
        self.write("docs/空 格.md", "updated\n")
        self.write("README.md", "readme\n")
        self.assertEqual(self.classify(self.commit()), "docs")

    def test_source_workflow_and_docs_scripts_are_full(self):
        for path in ("src/app.ts", ".github/workflows/ci.yml", "docs/example.py", "docs/install.sh"):
            with self.subTest(path=path):
                self.git("reset", "--hard", self.base)
                self.write(path, "changed\n")
                self.assertEqual(self.classify(self.commit()), "full")

    def test_mixed_changes_are_full(self):
        self.write("docs/guide.md", "updated\n")
        self.write("src/app.ts", "changed\n")
        self.assertEqual(self.classify(self.commit()), "full")

    def test_code_renamed_into_docs_is_full(self):
        self.git("mv", "src/app.ts", "docs/example.md")
        self.assertEqual(self.classify(self.commit()), "full")

    def test_doc_renamed_into_code_is_full(self):
        self.git("mv", "docs/guide.md", "src/example.ts")
        self.assertEqual(self.classify(self.commit()), "full")

    def test_document_rename_and_delete_are_docs(self):
        self.git("mv", "docs/guide.md", "docs/new name.md")
        (self.repo / "docs/空 格.md").unlink()
        self.assertEqual(self.classify(self.commit()), "docs")

    def test_code_deletion_is_full(self):
        (self.repo / "src/app.ts").unlink()
        self.assertEqual(self.classify(self.commit()), "full")

    def test_executable_document_is_full(self):
        self.git("update-index", "--chmod=+x", "docs/guide.md")
        self.git("commit", "-qm", "make executable")
        self.assertEqual(self.classify(self.git("rev-parse", "HEAD")), "full")

    def test_empty_diff_is_full(self):
        self.assertEqual(self.classify(self.base), "full")

    def test_unknown_zero_or_option_revision_is_full(self):
        for base in ("0" * 40, "f" * 40, "--output=bad", None):
            self.assertEqual(classify_event("push", {"before": base, "after": self.base}, cwd=self.repo), "full")

    def test_pr_uses_merge_base_not_new_base_branch_changes(self):
        self.git("checkout", "-qb", "feature")
        self.write("docs/guide.md", "feature guide\n")
        head = self.commit()
        self.git("checkout", "-q", "--detach", self.base)
        self.write("src/app.ts", "unrelated base update\n")
        new_base = self.commit()
        self.assertEqual(self.classify(head, new_base, "pull_request"), "docs")
        self.assertEqual(self.classify(head, new_base, "push"), "full")

    def test_push_checks_all_commits_not_just_latest(self):
        self.write("src/app.ts", "source update\n")
        self.commit()
        self.write("docs/guide.md", "latest docs\n")
        self.assertEqual(self.classify(self.commit()), "full")

    def test_unknown_event_and_malformed_payload_are_full(self):
        for name, event in (("workflow_dispatch", {}), ("push", {}), ("pull_request", {"pull_request": None}), ("push", [])):
            self.assertEqual(classify_event(name, event, cwd=self.repo), "full")

    def test_git_unavailable_is_full(self):
        with patch("changed_files.git", side_effect=OSError("git unavailable")):
            self.assertEqual(self.classify(self.base), "full")

    def test_cli_bad_event_still_emits_full(self):
        event_path = self.repo / "event.json"
        output_path = self.repo / "output.txt"
        event_path.write_text("{invalid json", encoding="utf-8")
        env = dict(os.environ, GITHUB_EVENT_PATH=str(event_path), GITHUB_OUTPUT=str(output_path), GITHUB_EVENT_NAME="push")
        script = Path(__file__).with_name("changed_files.py")
        subprocess.run([sys.executable, "-B", str(script)], env=env, check=True, capture_output=True)
        self.assertEqual(output_path.read_text(encoding="utf-8"), "scope=full\n")


if __name__ == "__main__":
    unittest.main()
