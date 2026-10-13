#!/usr/bin/env python3
"""Patch the SMDH embedded in an already-built .3dsx, preserving the RomFS.

Does NOT require the ELF, 3dsxtool, recompilation, or changes to Rust/Cargo.
Requires devkitPro smdhtool (already installed for cargo-3ds builds).
"""
from pathlib import Path
import argparse
import os
import shutil
import struct
import subprocess
import tempfile

TITLE = "Meets On 3DS"
DESCRIPTION = "Tamagotchi Meets/ON emulator for Nintendo 3DS"
AUTHOR = "maaartennn"
SMDH_SIZE = 0x36C0


def metadata_strings(raw, lang=1):
    base = 8 + lang * 0x200
    def field(rel, length):
        return raw[base + rel:base + rel + length].decode('utf-16-le').split('\0')[0]
    return field(0, 0x80), field(0x80, 0x100), field(0x180, 0x80)


def put_field(buf, base, size, value):
    data = value.encode('utf-16-le')
    if len(data) > size - 2:
        raise RuntimeError(f'SMDH metadata too long: {value!r}')
    buf[base:base + size] = data + bytes(size - len(data))


def set_all_languages(raw):
    if len(raw) != SMDH_SIZE or raw[:4] != b'SMDH':
        raise RuntimeError('smdhtool produced an invalid SMDH')
    modified = bytearray(raw)
    for lang in range(16):
        base = 8 + lang * 0x200
        put_field(modified, base, 0x80, TITLE)
        put_field(modified, base + 0x80, 0x100, DESCRIPTION)
        put_field(modified, base + 0x180, 0x80, AUTHOR)
    result = bytes(modified)
    for lang in range(16):
        if metadata_strings(result, lang) != (TITLE, DESCRIPTION, AUTHOR):
            raise RuntimeError(f'Metadata verification failed for language {lang}')
    return result


def get_smdh_region(data):
    if len(data) < 0x2C or data[:4] != b'3DSX':
        raise RuntimeError('The compiled file is not a valid extended-header 3DSX')
    header_size = struct.unpack_from('<H', data, 4)[0]
    if header_size < 0x2C:
        raise RuntimeError('3DSX has no SMDH extended header')
    offset, size, romfs_offset = struct.unpack_from('<III', data, 0x20)
    if size != SMDH_SIZE or offset < header_size or offset + size > len(data):
        raise RuntimeError('Unexpected SMDH size or offset inside the 3DSX')
    if data[offset:offset + 4] != b'SMDH':
        raise RuntimeError('Embedded SMDH signature missing')
    if romfs_offset and (romfs_offset < offset + size or romfs_offset > len(data)):
        raise RuntimeError('Unexpected RomFS offset; refusing to modify the 3DSX')
    return offset, size


def backup_once(path):
    backup = path.with_suffix(path.suffix + '.before-launcher-v23.bak')
    if not backup.exists():
        shutil.copy2(path, backup)
        print('Backup:', backup)


def main():
    root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out-dir", type=Path, default=root / "target/armv6k-nintendo-3ds/release")
    args = parser.parse_args()
    icon = root / 'icon.png'
    outdir = args.out_dir
    executable = outdir / 'tg18-3ds.3dsx'
    external_smdh = outdir / 'tg18-3ds.smdh'
    for path in [icon, executable]:
        if not path.is_file():
            raise RuntimeError(f'Missing {path}')
    png = icon.read_bytes()
    if png[:8] != b'\x89PNG\r\n\x1a\n' or len(png) < 24:
        raise RuntimeError('icon.png is not a PNG file')
    width, height = struct.unpack_from('>II', png, 16)
    if (width, height) != (48, 48):
        raise RuntimeError(f'icon.png must be 48x48 pixels (got {width}x{height})')
    smdhtool = shutil.which('smdhtool')
    if not smdhtool:
        raise RuntimeError('smdhtool not found on PATH. Please check your devkitPro installation.')

    # Validate the existing 3DSX before touching anything.
    old_3dsx = executable.read_bytes()
    offset, size = get_smdh_region(old_3dsx)

    with tempfile.TemporaryDirectory(prefix='tama-launcher-', dir=outdir) as temp:
        work = Path(temp)
        generated = work / 'generated.smdh'
        # smdhtool embeds both the icon and text into the SMDH.
        command = [smdhtool, '--create', TITLE, DESCRIPTION, AUTHOR, str(icon), str(generated)]
        print('+', ' '.join(command))
        subprocess.run(command, check=True)
        fresh_smdh = set_all_languages(generated.read_bytes())
        modified = old_3dsx[:offset] + fresh_smdh + old_3dsx[offset + size:]
        if len(modified) != len(old_3dsx):
            raise RuntimeError('3DSX length unexpectedly changed')
        if modified[:offset] != old_3dsx[:offset] or modified[offset + size:] != old_3dsx[offset + size:]:
            raise RuntimeError('Other executable content was modified')
        check_offset, check_size = get_smdh_region(modified)
        assert (check_offset, check_size) == (offset, size)
        assert metadata_strings(modified[offset:offset + size]) == (TITLE, DESCRIPTION, AUTHOR)

        temp_3dsx = work / 'patched.3dsx'
        temp_smdh = work / 'patched.smdh'
        temp_3dsx.write_bytes(modified)
        temp_smdh.write_bytes(fresh_smdh)
        # Back up before overwriting either output.
        backup_once(executable)
        if external_smdh.exists():
            backup_once(external_smdh)
        os.replace(temp_3dsx, executable)
        os.replace(temp_smdh, external_smdh)

    verified = executable.read_bytes()
    actual_offset, _ = get_smdh_region(verified)
    actual = metadata_strings(verified[actual_offset:actual_offset + SMDH_SIZE])
    if actual != (TITLE, DESCRIPTION, AUTHOR):
        raise RuntimeError(f'Embedded SMDH verification failed: {actual!r}')
    if external_smdh.read_bytes() != verified[actual_offset:actual_offset + SMDH_SIZE]:
        raise RuntimeError('External and embedded SMDH differ')
    print('\nSUCCESS — confirmed INSIDE the 3DSX:')
    print('Title:', actual[0])
    print('Description:', actual[1])
    print('Author:', actual[2])
    print('Icon: 48x48 custom PNG (via smdhtool)')
    print('\n3DSX:', executable)
    print('SMDH:', external_smdh)
    print('\nCopy the patched .3dsx to the SAME SD card location as before.')
    print('If a .smdh is also present alongside the SD-card .3dsx, replace it too.')
    print('Re-run this script AFTER each future cargo 3ds build.')

if __name__ == '__main__':
    try:
        main()
    except (OSError, RuntimeError, subprocess.CalledProcessError) as exc:
        raise SystemExit(f'ERROR: {exc}') from exc
