"""Regression tests for guarded patching and the actual build/packaging pipeline.
The build tests use fake compiler/devkit tools; they do not compile ARM code.
"""
from pathlib import Path
import importlib.util
import json
import os
import shutil
import struct
import sys
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


def load_tool(name):
    spec = importlib.util.spec_from_file_location(name, ROOT / 'tools' / (name + '.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


patcher = load_tool('patch_core')
launcher = load_tool('fix_launcher')
validator = load_tool('validate')


def core_source():
    return 'use std::hash::BuildHasherDefault;\nstruct AddrHasher;\n' + '\n'.join(old for old, _ in patcher.PATCHES)


def executable_fixture():
    offset = 0x60
    raw = bytearray(offset + launcher.SMDH_SIZE + 16)
    raw[:4] = b'3DSX'
    struct.pack_into('<H', raw, 4, 0x2C)
    struct.pack_into('<III', raw, 0x20, offset, launcher.SMDH_SIZE, 0)
    raw[offset:offset + 4] = b'SMDH'
    raw[-16:] = b'UNCHANGED CODE!!'
    return raw


class CorePatchTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.core = Path(self.temp.name) / 'sys.rs'
        self.core.write_text(core_source())

    def test_exact_revision_patches_and_second_run_preserves_backup(self):
        original = self.core.read_bytes()
        self.assertTrue(patcher.apply_patch(self.core))
        backup = self.core.with_name('sys.rs.before-v49.bak')
        self.assertEqual(backup.read_bytes(), original)
        patched = self.core.read_bytes()
        self.assertFalse(patcher.apply_patch(self.core))
        self.assertEqual(self.core.read_bytes(), patched)
        self.assertEqual(backup.read_bytes(), original)

    def test_unknown_source_is_untouched(self):
        self.core.write_text('different revision')
        with self.assertRaises(RuntimeError):
            patcher.apply_patch(self.core)
        self.assertEqual(self.core.read_text(), 'different revision')
        self.assertFalse(self.core.with_name('sys.rs.before-v49.bak').exists())

    def test_partial_patch_is_untouched(self):
        self.core.write_text(core_source().replace(patcher.old_struct, patcher.new_struct))
        original = self.core.read_bytes()
        with self.assertRaises(RuntimeError):
            patcher.apply_patch(self.core)
        self.assertEqual(self.core.read_bytes(), original)

    def test_existing_backup_is_not_overwritten(self):
        backup = self.core.with_name('sys.rs.before-v49.bak')
        backup.write_text('keep this')
        original = self.core.read_bytes()
        with self.assertRaises(RuntimeError):
            patcher.apply_patch(self.core)
        self.assertEqual(backup.read_text(), 'keep this')
        self.assertEqual(self.core.read_bytes(), original)


class LauncherTests(unittest.TestCase):
    def test_all_languages_and_executable_payload_are_preserved(self):
        raw = executable_fixture()
        offset, size = launcher.get_smdh_region(raw)
        fresh = launcher.set_all_languages(raw[offset:offset + size])
        for language in range(16):
            self.assertEqual(launcher.metadata_strings(fresh, language),
                             (launcher.TITLE, launcher.DESCRIPTION, launcher.AUTHOR))
        modified = raw[:offset] + fresh + raw[offset + size:]
        self.assertEqual(modified[:offset], raw[:offset])
        self.assertEqual(modified[offset + size:], raw[offset + size:])

    def test_rejects_truncated_binary_and_bad_smdh_offsets(self):
        with self.assertRaises(RuntimeError):
            launcher.get_smdh_region(b'3DSX')
        raw = executable_fixture()
        struct.pack_into('<I', raw, 0x20, len(raw) + 1)
        with self.assertRaises(RuntimeError):
            launcher.get_smdh_region(raw)


class AssetTests(unittest.TestCase):
    def test_production_asset_inventory(self):
        validator.validate_assets()

    def test_invalid_asset_size_is_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / 'src').mkdir()
            (root / 'tools').mkdir()
            (root / 'assets').mkdir()
            (root / 'src/main.rs').write_text('include_bytes!("../assets/broken.bin");')
            (root / 'tools/asset_sizes.json').write_text('{"assets/broken.bin": 4}')
            (root / 'assets/broken.bin').write_bytes(b'bad')
            with self.assertRaises(RuntimeError):
                validator.validate_assets(root)

    def test_missing_tomllib_has_a_clear_error_without_traceback(self):
        result = subprocess.run([sys.executable, '-c',
            'import runpy, sys; sys.modules["tomllib"] = None; runpy.run_path(sys.argv[1], run_name="__main__")',
            str(ROOT / 'tools/validate.py')], text=True, capture_output=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Python 3.11+', result.stderr)
        self.assertNotIn('Traceback', result.stderr)


class BuildTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name)
        self.project = self.base / 'tg18-3ds'
        shutil.copytree(ROOT, self.project, ignore=shutil.ignore_patterns(
            'archive', 'docs', 'tests', 'artwork', '__pycache__', 'target', 'READY_FOR_FTPD'))
        core = self.base / 'tg18-emu-main/core'
        (core / 'src').mkdir(parents=True)
        (core / 'Cargo.toml').write_text('[package]\nname="tg18-core"\nversion="0.1.0"\n')
        (core / 'src/sys.rs').write_text(core_source())
        (core / 'src/lib.rs').write_text('pub probe_stats: [u64; 5]; pub idle_skipped: u64;')
        tools = self.base / 'fake-bin'
        tools.mkdir()
        cargo = tools / 'cargo'
        cargo.write_text('''#!/usr/bin/env python3
import json, os, pathlib, shutil, sys
with open(os.environ['TG18_TEST_LOG'], 'a') as log: log.write(json.dumps(sys.argv[1:])+'\\n')
if 'clean' in sys.argv: shutil.rmtree('target', ignore_errors=True)
if 'build' in sys.argv:
    if os.environ.get('TG18_FAIL_BUILD'): sys.exit(1)
    out = pathlib.Path('target/armv6k-nintendo-3ds/release')
    out.mkdir(parents=True, exist_ok=True)
    (out/'tg18-3ds.3dsx').write_bytes(bytes.fromhex(os.environ['TG18_BINARY_FIXTURE']))
''')
        cargo.chmod(0o755)
        smdhtool = tools / 'smdhtool'
        smdhtool.write_text('''#!/usr/bin/env python3
import pathlib, sys
raw=bytearray(0x36C0);raw[:4]=b'SMDH'
pathlib.Path(sys.argv[-1]).write_bytes(raw)
''')
        smdhtool.chmod(0o755)
        self.log = self.base / 'cargo-log.jsonl'
        self.env = dict(os.environ, PATH=str(tools) + os.pathsep + os.environ['PATH'],
                        TG18_TEST_LOG=str(self.log), TG18_BINARY_FIXTURE=executable_fixture().hex())
        self.env.pop('CARGO_TARGET_DIR', None)
        self.env.pop('TG18_PYTHON', None)

    def run_build(self, *args, **env):
        return subprocess.run(['bash', 'build.sh', *args], cwd=self.project,
                              env={**self.env, **env}, text=True, capture_output=True)

    def python_wrapper(self, name, compatible):
        path = self.base / 'fake-bin' / name
        path.write_text('#!' + sys.executable + '\n' +
            'import json, os, subprocess, sys\n' +
            ('if sys.argv[1:2] == ["-c"]: sys.exit(1)\n' if not compatible else '') +
            'if os.environ.get("TG18_PYTHON_LOG"):\n' +
            '    with open(os.environ["TG18_PYTHON_LOG"], "a") as log: log.write(json.dumps(sys.argv[1:])+"\\n")\n' +
            'sys.exit(subprocess.call([sys.executable, *sys.argv[1:]]))\n')
        path.chmod(0o755)
        return path

    def test_older_default_python_selects_compatible_version_for_all_helpers(self):
        self.python_wrapper('python3', False)
        self.python_wrapper('python3.12', True)
        python_log = self.base / 'python-log.jsonl'
        result = self.run_build(TG18_PYTHON_LOG=str(python_log))
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn('Build Python: python3.12', result.stdout)
        calls = [json.loads(line) for line in python_log.read_text().splitlines()]
        helpers = [args[0] for args in calls if args and args[0].startswith('tools/')]
        self.assertEqual(helpers, ['tools/validate.py', 'tools/patch_core.py',
                                  'tools/fix_launcher.py', 'tools/validate.py', 'tools/validate.py'])

    def test_explicit_python_override_is_used(self):
        override = self.python_wrapper('chosen-python', True)
        result = self.run_build('--check', '--no-core-patch', TG18_PYTHON=str(override))
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn('Build Python: ' + str(override), result.stdout)

    def test_incompatible_python_override_stops_before_compilation(self):
        override = self.python_wrapper('old-python', False)
        result = self.run_build(TG18_PYTHON=str(override))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('TG18_PYTHON must name a working Python 3.11+', result.stderr)
        self.assertNotIn('Traceback', result.stderr)
        self.assertFalse(self.log.exists())

    def test_incremental_build_keeps_cache_and_creates_verified_outputs(self):
        marker = self.project / 'target/keep-cache'
        marker.parent.mkdir()
        marker.write_text('cached')
        result = self.run_build()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertTrue(marker.is_file())
        calls = [json.loads(line) for line in self.log.read_text().splitlines()]
        self.assertFalse(any('clean' in args for args in calls))
        self.assertTrue(any('build' in args and '--locked' in args for args in calls))
        output = self.project / 'READY_FOR_FTPD/tg18-3ds.3dsx'
        raw = output.read_bytes()
        offset, size = launcher.get_smdh_region(raw)
        self.assertEqual(launcher.metadata_strings(raw[offset:offset + size]),
                         (launcher.TITLE, launcher.DESCRIPTION, launcher.AUTHOR))
        self.assertEqual((output.parent / 'tg18-3ds.smdh').read_bytes(), raw[offset:offset + size])
        self.assertEqual(raw[-16:], b'UNCHANGED CODE!!')

    def test_explicit_clean_removes_cache(self):
        marker = self.project / 'target/keep-cache'
        marker.parent.mkdir()
        marker.write_text('cached')
        result = self.run_build('--clean')
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertFalse(marker.exists())

    def test_failed_compilation_does_not_stage_a_stale_binary(self):
        out = self.project / 'target/armv6k-nintendo-3ds/release/tg18-3ds.3dsx'
        out.parent.mkdir(parents=True)
        out.write_bytes(executable_fixture())
        result = self.run_build(TG18_FAIL_BUILD='1')
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((self.project / 'READY_FOR_FTPD/tg18-3ds.3dsx').exists())

    def test_optional_audio_debug_feature_is_passed_to_build(self):
        result = self.run_build('--audio-debug')
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        calls = [json.loads(line) for line in self.log.read_text().splitlines()]
        self.assertTrue(any('build' in args and '--features' in args and 'audio-debug' in args for args in calls))

    def test_check_only_does_not_stage_outputs(self):
        result = self.run_build('--check', '--no-core-patch')
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        calls = [json.loads(line) for line in self.log.read_text().splitlines()]
        self.assertTrue(any('check' in args and '--locked' in args for args in calls))
        self.assertFalse((self.project / 'READY_FOR_FTPD').exists())
        self.assertFalse((self.base / 'tg18-emu-main/core/src/sys.rs.before-v49.bak').exists())


if __name__ == '__main__':
    unittest.main()
