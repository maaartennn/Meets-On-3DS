#!/usr/bin/env python3
"""Apply the existing v49 idle-write patch only to a recognized core revision."""
from pathlib import Path
import argparse
import os
import re
import shutil
import tempfile

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_CORE = ROOT.parent / 'tg18-emu-main/core/src/sys.rs'

old_struct = '''    tracking: bool,
    write_log: Vec<(u32, u32)>,'''
new_struct = '''    tracking: bool,
    write_log: Vec<(u32, u32)>,
    // Reusable address set for safe idle-loop write deduplication.
    write_seen: std::collections::HashSet<u32, BuildHasherDefault<AddrHasher>>,'''
old_init='''            tracking: false,
            write_log: Vec::new(),'''
new_init='''            tracking: false,
            write_log: Vec::new(),
            write_seen: Default::default(),'''
old_fn='''        let log = std::mem::take(&mut self.write_log);
        let mut seen: Vec<u32> = Vec::with_capacity(log.len());
        let mut same = true;
        for &(addr, old) in &log {
            if seen.contains(&addr) {
                continue; // the first entry holds the value before the pass
            }
            seen.push(addr);
            if addr.wrapping_sub(self.rand_state & !3) < 8 {
                continue;
            }
            if self.peek_word(addr) != old {
                same = false;
                break;
            }
        }
        self.write_log = log;
        same'''
new_fn='''        let log = std::mem::take(&mut self.write_log);
        // Preserve the FIRST logged value for each word (before the pass).
        // Reusing the set avoids repeated allocations in frequent idle probes.
        self.write_seen.clear();
        let mut same = true;
        for &(addr, old) in &log {
            if !self.write_seen.insert(addr) {
                continue;
            }
            if addr.wrapping_sub(self.rand_state & !3) < 8 {
                continue;
            }
            if self.peek_word(addr) != old {
                same = false;
                break;
            }
        }
        self.write_log = log;
        same'''

PATCHES = [(old_struct, new_struct), (old_init, new_init), (old_fn, new_fn)]


def apply_patch(core):
    if not core.is_file():
        raise RuntimeError(f'Could not find tg18-core: {core}')
    source = core.read_text()
    if all(source.count(new) == 1 for _, new in PATCHES):
        return False
    if 'write_seen' in source:
        raise RuntimeError('Partial or different core patch; inspect sys.rs and its backup. No files changed.')
    for old, _ in PATCHES:
        if source.count(old) != 1:
            raise RuntimeError('Core version mismatch; expected code not found exactly once. No files changed.\nLooking for: ' + old[:75])
    # The patch uses the core's existing address hasher. Do not introduce
    # an undeclared type into another upstream revision.
    if not re.search(r'\bAddrHasher\b', source) or not re.search(r'\bBuildHasherDefault\b', source):
        raise RuntimeError('Core hasher types not found. No files changed.')
    backup = core.with_name('sys.rs.before-v49.bak')
    if backup.exists():
        raise RuntimeError('Backup already exists but source is not patched; inspect both files. No files changed.')
    updated = source
    for old, new in PATCHES:
        updated = updated.replace(old, new, 1)
    shutil.copy2(core, backup)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode='w', dir=core.parent, prefix='.sys-', suffix='.tmp', delete=False) as f:
            temporary = Path(f.name)
            f.write(updated)
            f.flush()
            os.fsync(f.fileno())
        shutil.copymode(core, temporary)
        os.replace(temporary, core)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)
    return True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--core', type=Path, default=DEFAULT_CORE)
    args = parser.parse_args()
    changed = apply_patch(args.core)
    print(('Applied v49 core patch: ' if changed else 'Core patch already installed: ') + str(args.core))


if __name__ == '__main__':
    try:
        main()
    except (OSError, RuntimeError) as exc:
        raise SystemExit(f'ERROR: {exc}') from exc
