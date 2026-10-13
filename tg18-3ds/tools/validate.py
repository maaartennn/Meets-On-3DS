#!/usr/bin/env python3
"""Check embedded assets and optionally the core dependency or a built 3DSX."""
from pathlib import Path
import argparse
import hashlib
import json
import re
import sys
try:
    import tomllib
except ModuleNotFoundError:
    sys.exit('ERROR: This validator needs Python 3.11+ with tomllib. '
             'Run bash build.sh to select a compatible interpreter, or use your Homebrew Python 3.12.')

ROOT = Path(__file__).resolve().parents[1]


def validate_assets(root=ROOT):
    expected = json.loads((root / 'tools/asset_sizes.json').read_text())
    found = set()
    for source in (root / 'src').glob('*.rs'):
        for name in re.findall(r'include_bytes!\(\s*"([^"]+)"\s*\)', source.read_text()):
            asset = (source.parent / name).resolve()
            key = str(asset.relative_to(root.resolve()))
            if key not in expected:
                raise RuntimeError(f'Asset has no size entry: {key}')
            if not asset.is_file() or asset.stat().st_size != expected[key]:
                raise RuntimeError(f'Invalid asset {key}: expected {expected[key]} bytes')
            found.add(key)
    if found != set(expected):
        raise RuntimeError(f'Asset inventory differs from source: {set(expected) - found}')
    manifest = tomllib.loads((root / 'Cargo.toml').read_text())
    if 'jpeg-encoder' not in manifest['dependencies'] or 'tg18-core' not in manifest['dependencies']:
        raise RuntimeError('Missing required Cargo dependency')
    if 'romfs_dir' in manifest['package']['metadata']['cargo-3ds'] or (root / 'romfs').exists():
        raise RuntimeError('Assets are embedded; remove duplicate RomFS packaging')
    print(f'OK: {len(found)} embedded assets, correct byte sizes, Cargo manifest')


def validate_core(root=ROOT):
    manifest = tomllib.loads((root / 'Cargo.toml').read_text())
    core = (root / manifest['dependencies']['tg18-core']['path']).resolve()
    if not (core / 'Cargo.toml').is_file() or not (core / 'src/sys.rs').is_file():
        raise RuntimeError(f'Missing sibling emulator core: {core}. Restore tg18-emu-main beside tg18-3ds.')
    source = '\n'.join(p.read_text() for p in (core / 'src').rglob('*.rs'))
    for counter in ('probe_stats', 'idle_skipped'):
        if not re.search(r'\b' + counter + r'\b', source):
            raise RuntimeError(f'This frontend requires a core with the v49 {counter} counter')
    print(f'OK: core source present at {core} (API compatibility still requires compilation)')


def validate_binary(path):
    from fix_launcher import get_smdh_region, metadata_strings, TITLE, DESCRIPTION, AUTHOR
    raw = path.read_bytes()
    offset, size = get_smdh_region(raw)
    for lang in range(16):
        if metadata_strings(raw[offset:offset + size], lang) != (TITLE, DESCRIPTION, AUTHOR):
            raise RuntimeError(f'Incorrect embedded launcher metadata: language {lang}')
    print(f'OK: {path}; {len(raw)} bytes; SHA256 {hashlib.sha256(raw).hexdigest()}')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check-core', action='store_true')
    parser.add_argument('--binary', type=Path)
    args = parser.parse_args()
    validate_assets()
    if args.check_core:
        validate_core()
    if args.binary:
        validate_binary(args.binary)


if __name__ == '__main__':
    try:
        main()
    except (OSError, RuntimeError, ValueError, KeyError) as exc:
        sys.exit(f'ERROR: {exc}')
