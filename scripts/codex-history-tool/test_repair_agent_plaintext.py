import base64
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

spec = importlib.util.spec_from_file_location('repair_agent_plaintext', Path(__file__).with_name('repair_agent_plaintext.py'))
repair = importlib.util.module_from_spec(spec)
spec.loader.exec_module(repair)


def encode(value):
    return json.dumps(value, ensure_ascii=False, separators=(',', ':')).encode('utf-8') + b'\n'


def fixture(provider='codex_model_router_v2', text='已核验，完整结果如下。'):
    return (encode({'ordinal': 0, 'type': 'session_meta', 'payload': {'id': 'thread', 'model_provider': provider, 'history_mode': 'paginated'}})
            + encode({'ordinal': 1, 'type': 'response_item', 'payload': {'type': 'agent_message', 'id': 'amsg_test', 'author': '/root/worker', 'recipient': '/root', 'content': [
                {'type': 'input_text', 'text': 'Message Type: FINAL_ANSWER\nPayload:\n'},
                {'type': 'encrypted_content', 'encrypted_content': text}]}})
            + encode({'ordinal': 2, 'type': 'response_item', 'payload': {'type': 'reasoning', 'encrypted_content': 'gAAAAA' + 'A' * 160, 'summary': []}}))


class RepairTests(unittest.TestCase):
    def transaction(self, root, count=1):
        home, backup = root / 'home', root / 'backup'
        home.mkdir()
        backup.mkdir()
        (home / 'config.toml').write_bytes(b'model_provider = "openai"\n')
        entries = []
        for index in range(count):
            path = home / 'sessions' / f'rollout-{index}.jsonl'
            path.parent.mkdir(exist_ok=True)
            before = fixture(text=f'完整历史 {index} 保留。')
            path.write_bytes(before)
            saved = backup / 'codex' / path.relative_to(home)
            saved.parent.mkdir(parents=True, exist_ok=True)
            saved.write_bytes(before)
            after, changes = repair.repair_rollout(before)
            entries.append({'relative_path': str(path.relative_to(home)), 'bytes': len(before),
                            'before_sha256': repair.sha(before), 'after_sha256': repair.sha(after), 'changes': changes})
        return {'codex_home': str(home), 'backup': str(backup), 'files': entries}

    def test_recovers_text_with_identical_offsets_and_other_records(self):
        before = fixture()
        after, changes = repair.repair_rollout(before)
        old_lines, new_lines = before.splitlines(keepends=True), after.splitlines(keepends=True)
        self.assertEqual(len(changes), 1)
        self.assertEqual(len(before), len(after))
        self.assertEqual([len(x) for x in old_lines], [len(x) for x in new_lines])
        self.assertEqual(old_lines[0], new_lines[0])
        self.assertEqual(old_lines[2], new_lines[2])
        old, new = json.loads(old_lines[1]), json.loads(new_lines[1])
        expected = json.loads(old_lines[1])
        expected['payload']['content'][1] = {'type': 'input_text', 'text': old['payload']['content'][1]['encrypted_content']}
        self.assertEqual(new, expected)
        self.assertEqual(repair.repair_rollout(after), (after, []))

    def test_preserves_ciphertext_and_native_history(self):
        for value in ('gAAAAA' + 'A' * 160, base64.b64encode(bytes(range(96))).decode()):
            before = fixture(text=value)
            self.assertEqual(repair.repair_rollout(before), (before, []))
        before = fixture(provider='openai')
        self.assertEqual(repair.repair_rollout(before), (before, []))

    def test_ascii_plaintext_and_crlf_are_preserved_losslessly(self):
        before = fixture(text='The original task is complete. Full details remain available.').replace(b'\n', b'\r\n')
        after, changes = repair.repair_rollout(before)
        self.assertEqual(len(changes), 1)
        self.assertEqual(len(before), len(after))
        self.assertEqual(before.count(b'\r\n'), after.count(b'\r\n'))

    def test_unknown_part_fields_fail_closed(self):
        before = fixture().replace(b'"encrypted_content":"', b'"future_field":true,"encrypted_content":"', 1)
        with self.assertRaises(ValueError):
            repair.repair_rollout(before)

    def test_apply_reads_back_full_history_and_preserves_backup(self):
        with tempfile.TemporaryDirectory() as root:
            plan = self.transaction(Path(root))
            path = Path(plan['codex_home']) / plan['files'][0]['relative_path']
            before = path.read_bytes()
            expected, _ = repair.repair_rollout(before)
            repair.apply_plan(plan)
            self.assertEqual(path.read_bytes(), expected)
            saved = Path(plan['backup']) / 'codex' / plan['files'][0]['relative_path']
            self.assertEqual(saved.read_bytes(), before)

    def test_stale_plan_and_busy_batch_never_write(self):
        with tempfile.TemporaryDirectory() as root:
            plan = self.transaction(Path(root), 2)
            paths = [Path(plan['codex_home']) / x['relative_path'] for x in plan['files']]
            before = [x.read_bytes() for x in paths]
            with mock.patch.object(repair, 'exclusive_file', side_effect=[paths[0].open('r+b'), PermissionError('busy')]):
                with self.assertRaises(PermissionError):
                    repair.apply_plan(plan)
            self.assertEqual([x.read_bytes() for x in paths], before)
            plan['files'][0]['before_sha256'] = 'stale'
            with self.assertRaises(ValueError):
                repair.apply_plan(plan)
            self.assertEqual([x.read_bytes() for x in paths], before)

    def test_write_failure_restores_every_original(self):
        class FailOnce:
            def __init__(self, stream):
                self.stream, self.failed = stream, False
            def __getattr__(self, name):
                return getattr(self.stream, name)
            def write(self, value):
                if not self.failed:
                    self.failed = True
                    self.stream.write(value[:8])
                    raise OSError('simulated partial write')
                return self.stream.write(value)
        with tempfile.TemporaryDirectory() as root:
            plan = self.transaction(Path(root), 2)
            paths = [Path(plan['codex_home']) / x['relative_path'] for x in plan['files']]
            before = [x.read_bytes() for x in paths]
            handles = [paths[0].open('r+b'), FailOnce(paths[1].open('r+b'))]
            with mock.patch.object(repair, 'exclusive_file', side_effect=handles):
                with self.assertRaises(OSError):
                    repair.apply_plan(plan)
            self.assertEqual([x.read_bytes() for x in paths], before)

    def test_persistent_sync_failure_attempts_every_restoration(self):
        with tempfile.TemporaryDirectory() as root:
            plan = self.transaction(Path(root), 2)
            paths = [Path(plan['codex_home']) / x['relative_path'] for x in plan['files']]
            before = [x.read_bytes() for x in paths]
            with mock.patch.object(repair.os, 'fsync', side_effect=[None, OSError('disk failed'), OSError('still failed'), OSError('still failed')]) as sync:
                with self.assertRaises(BaseExceptionGroup) as caught:
                    repair.apply_plan(plan)
                self.assertEqual(sync.call_count, 4)
                self.assertEqual(len(caught.exception.exceptions), 3)
            self.assertEqual([x.read_bytes() for x in paths], before)

    def test_compressed_history_keeps_decompressed_offsets_and_full_text(self):
        from compression import zstd
        before = fixture()
        stored = zstd.compress(before)
        after, changes = repair.repair_stored(stored, True)
        expected, expected_changes = repair.repair_rollout(before)
        self.assertEqual(changes, expected_changes)
        self.assertEqual(zstd.decompress(after), expected)
        self.assertEqual(repair.repair_stored(after, True), (after, []))

    def test_compressed_apply_updates_length_and_retains_exact_original_backup(self):
        from compression import zstd
        with tempfile.TemporaryDirectory() as root:
            plan = self.transaction(Path(root))
            entry = plan['files'][0]
            path = Path(plan['codex_home']) / entry['relative_path']
            old_path = path
            path = path.with_suffix('.jsonl.zst')
            before = zstd.compress(old_path.read_bytes())
            path.write_bytes(before)
            entry['relative_path'] = str(path.relative_to(Path(plan['codex_home'])))
            entry['compressed'] = True
            after, changes = repair.repair_stored(before, True)
            entry.update(before_sha256=repair.sha(before), after_sha256=repair.sha(after), changes=changes)
            saved = Path(plan['backup']) / 'codex' / entry['relative_path']
            saved.write_bytes(before)
            repair.apply_plan(plan)
            self.assertEqual(path.read_bytes(), after)
            self.assertEqual(saved.read_bytes(), before)


if __name__ == '__main__':
    unittest.main()
