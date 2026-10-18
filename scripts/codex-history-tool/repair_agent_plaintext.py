"""Explicit, offline repair of legacy CCSM agent-message plaintext envelopes.

Preview first, then apply a pinned plan against an existing complete backup.
Every JSONL line keeps its byte length, preserving paginated history offsets.
This tool never reads login credentials or sends model requests.
"""

import argparse
import base64
import binascii
import hashlib
import json
import os
from pathlib import Path
import tomllib


ROUTER_PROVIDERS = {'codex_model_router', 'codex_model_router_v2', 'cc_switch_codex_router'}


def sha(data):
    return hashlib.sha256(data).hexdigest()


def compact(value):
    return json.dumps(value, ensure_ascii=False, separators=(',', ':')).encode('utf-8')


def opaque(value):
    # Match CCSM's existing OAuth replay boundary conservatively. Ambiguous
    # base64 payloads remain opaque; their content cannot be recovered here.
    if len(value) < 64 or not value.isascii():
        return False
    if value.startswith('gAAAAA'):
        return True
    padded = value + '=' * (-len(value) % 4)
    for alphabet in (None, b'-_'):
        try:
            if len(base64.b64decode(padded, altchars=alphabet, validate=True)) >= 32:
                return True
        except (binascii.Error, ValueError):
            pass
    return False


def repair_rollout(data: bytes):
    lines = data.splitlines(keepends=True)
    header = next((json.loads(line) for line in lines if line.strip()), {})
    if header.get('type') != 'session_meta' or header.get('payload', {}).get('model_provider') not in ROUTER_PROVIDERS:
        return data, []
    changes = []
    offset = 0
    for index, line in enumerate(lines):
        if not line.strip():
            offset += len(line)
            continue
        record = json.loads(line)
        payload = record.get('payload', {})
        if record.get('type') != 'response_item' or payload.get('type') != 'agent_message':
            offset += len(line)
            continue
        if not str(payload.get('id', '')).startswith('amsg_'):
            raise ValueError('Agent message identity cannot be established')
        expected = json.loads(line)
        updated = line
        for part_index, part in enumerate(payload.get('content', [])):
            if not isinstance(part, dict) or part.get('type') != 'encrypted_content':
                continue
            value = part.get('encrypted_content')
            if not isinstance(value, str) or not value or opaque(value):
                continue
            if set(part) != {'type', 'encrypted_content'}:
                raise ValueError('Unknown fields in a plaintext envelope; manual review required')
            old = compact(part)
            new_part = {'type': 'input_text', 'text': value}
            new = compact(new_part)
            if updated.count(old) != 1 or len(new) > len(old):
                raise ValueError('Cannot identify one exact fixed-length replacement')
            updated = updated.replace(old, new + b' ' * (len(old) - len(new)), 1)
            expected['payload']['content'][part_index] = new_part
            changes.append({'line': index + 1, 'byte_offset': offset, 'item_id': payload['id'], 'part_index': part_index})
        if len(updated) != len(line) or json.loads(updated) != expected:
            raise ValueError('History content or byte boundaries changed unexpectedly')
        lines[index] = updated
        offset += len(line)
    result = b''.join(lines)
    if len(result) != len(data):
        raise ValueError('History length changed')
    return result, changes


def repair_stored(data, compressed=False):
    if not compressed:
        return repair_rollout(data)
    # Python 3.14+ provides the standard-library codec. Offsets in paginated
    # compressed history address the decompressed JSONL stream; that stream's
    # line boundaries stay identical, and the path and IDs remain unchanged.
    from compression import zstd
    decoded = zstd.decompress(data)
    repaired, changes = repair_rollout(decoded)
    if not changes:
        return data, []
    encoded = zstd.compress(repaired)
    if zstd.decompress(encoded) != repaired:
        raise ValueError('Compressed history roundtrip failed')
    return encoded, changes


def build_plan(home, backup):
    manifest = json.loads((backup / 'manifest.json').read_text(encoding='utf-8'))
    if Path(manifest['codex_home']).resolve() != home:
        raise ValueError('Backup belongs to another Codex home')
    ledger = {entry['relative_path']: entry for entry in manifest['files']}
    files = []
    scanned = 0
    for folder in ('sessions', 'archived_sessions'):
        for path in sorted((home / folder).rglob('*')):
            if not path.is_file() or not path.name.endswith(('.jsonl', '.jsonl.zst')):
                continue
            if path.is_symlink() or not path.resolve().is_relative_to(home / folder):
                raise ValueError('Linked history is not supported')
            before = path.read_bytes()
            compressed = path.name.endswith('.zst')
            after, changes = repair_stored(before, compressed)
            scanned += 1
            if not changes:
                continue
            relative = str(path.relative_to(home))
            entry = ledger.get(relative)
            old_backup = backup / 'codex' / relative
            if not entry or entry['sha256'] != sha(before) or old_backup.read_bytes() != before:
                raise ValueError('Affected history changed since backup; create a fresh backup')
            files.append({'relative_path': relative, 'bytes': len(before), 'after_bytes': len(after), 'compressed': compressed, 'before_sha256': sha(before),
                          'after_sha256': sha(after), 'changes': changes})
    return {'codex_home': str(home), 'backup': str(backup), 'scanned_jsonl': scanned, 'files': files}


def exclusive_file(path):
    if os.name == 'nt':
        import ctypes
        from ctypes import wintypes
        import msvcrt
        create = ctypes.WinDLL('kernel32', use_last_error=True).CreateFileW
        create.argtypes = (wintypes.LPCWSTR, wintypes.DWORD, wintypes.DWORD, wintypes.LPVOID,
                           wintypes.DWORD, wintypes.DWORD, wintypes.HANDLE)
        create.restype = wintypes.HANDLE
        handle = create(str(path), 0xC0000000, 0, None, 3, 0x80, None)
        if handle == wintypes.HANDLE(-1).value:
            raise OSError(ctypes.get_last_error(), 'History is busy; close Codex before applying')
        descriptor = msvcrt.open_osfhandle(handle, os.O_RDWR | os.O_BINARY)
        return os.fdopen(descriptor, 'r+b')
    import fcntl
    stream = path.open('r+b')
    try:
        fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BaseException:
        stream.close()
        raise
    return stream


def apply_plan(plan):
    home, backup = Path(plan['codex_home']).resolve(), Path(plan['backup']).resolve()
    config = tomllib.loads((home / 'config.toml').read_text(encoding='utf-8-sig'))
    if config.get('model_provider', 'openai') != 'openai' or 'openai_base_url' in config or 'model_catalog_json' in config:
        raise ValueError('Repair requires native official Codex configuration')
    opened = []
    wrote = False
    try:
        # Lock and validate every affected file before the first write. An active
        # app-server writer makes the whole batch fail without partial repair.
        for entry in plan['files']:
            path = home / entry['relative_path']
            if path.is_symlink() or not path.resolve().is_relative_to(home):
                raise ValueError('History path escaped Codex home')
            stream = exclusive_file(path)
            opened.append((stream, None, None, entry.get('compressed', False)))
            before = stream.read()
            after, changes = repair_stored(before, entry.get('compressed', False))
            if sha(before) != entry['before_sha256'] or sha(after) != entry['after_sha256'] or changes != entry['changes']:
                raise ValueError('Repair plan is stale')
            if (backup / 'codex' / entry['relative_path']).read_bytes() != before:
                raise ValueError('Exact original backup is unavailable')
            opened[-1] = (stream, before, after, entry.get('compressed', False))
        wrote = True
        for stream, before, after, compressed in opened:
            stream.seek(0)
            if stream.write(after) != len(after):
                raise OSError('Incomplete history write')
            stream.truncate(len(after))
            stream.flush()
            os.fsync(stream.fileno())
            stream.seek(0)
            if stream.read() != after or repair_stored(after, compressed) != (after, []):
                raise ValueError('History readback failed')
    except BaseException as original:
        failures = []
        if wrote:
            for stream, before, _, _ in opened:
                try:
                    stream.seek(0)
                    if stream.write(before) != len(before):
                        raise OSError('Incomplete history restoration')
                    stream.truncate(len(before))
                    stream.flush()
                    os.fsync(stream.fileno())
                    stream.seek(0)
                    if stream.read() != before:
                        raise OSError('History restoration readback failed')
                except BaseException as failure:
                    failures.append(failure)
        if failures:
            raise BaseExceptionGroup('Repair failed; some restorations could not be verified. Exact backups remain available.', [original, *failures]) from original
        raise
    finally:
        for stream, _, _, _ in opened:
            stream.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--codex-home', required=True, type=Path)
    parser.add_argument('--backup', required=True, type=Path)
    parser.add_argument('--apply', action='store_true')
    parser.add_argument('--expected-plan-sha256')
    args = parser.parse_args()
    home, backup = args.codex_home.resolve(), args.backup.resolve()
    plan_path = backup / 'repair-plan.json'
    if args.apply:
        raw = plan_path.read_bytes()
        if not args.expected_plan_sha256 or sha(raw) != args.expected_plan_sha256:
            raise ValueError('A previewed, pinned repair plan is required')
        plan = json.loads(raw)
        current = build_plan(home, backup)
        if any(plan[key] != current[key] for key in ('codex_home', 'backup', 'files')):
            raise ValueError('History changed after preview')
        apply_plan(plan)
        (backup / 'repair-result.json').write_text(json.dumps({'status': 'applied', 'plan_sha256': sha(raw), 'plan': plan}, indent=2), encoding='utf-8')
    else:
        plan = build_plan(home, backup)
        raw = json.dumps(plan, indent=2).encode('utf-8')
        plan_path.write_bytes(raw)
    print(json.dumps({'status': 'applied' if args.apply else 'preview', 'scanned_jsonl': plan['scanned_jsonl'],
                      'affected_files': len(plan['files']), 'plaintext_parts': sum(len(x['changes']) for x in plan['files']),
                      'plan_sha256': sha(raw), 'backup': str(backup)}))


if __name__ == '__main__':
    main()
