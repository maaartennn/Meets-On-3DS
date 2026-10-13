# Meets On 3DS

This folder contains the 3DS frontend, embedded assets, tools and host tests.
The included core is at `../tg18-emu-main/core`; keep the folders beside each other.

See the [repository README](../README.md#source-code-and-building) for toolchain
requirements, installation and controls.

From this folder, with the required toolchain installed:

```bash
bash build.sh
```

For devkitPro installed at `/opt/devkitpro`:

```bash
PATH="/opt/devkitpro/tools/bin:$PATH" bash build.sh
```

Outputs are `READY_FOR_FTPD/tg18-3ds.3dsx` and
`READY_FOR_FTPD/tg18-3ds.smdh`.

The engineering reports under `docs/` describe earlier development sessions.
References to handoff files, archives and backups do not imply those files are
included in this snapshot. The checksum manifest records the original bundle
before publication documentation changes.

## Current layout

- `src/audio.rs`: native-rate PCM renderer and bounded FIFO.
- `src/main.rs`: app loop and NDSP output adapter.
- `src/lcd.rs`: retained faster LCD renderer.
- `src/firmware_picker.rs`, `photo.rs`, `diagnostics.rs`: retained modules.
- `assets/runtime/`: all 53 original embedded runtime assets.
- `assets/artwork/`: supplied editable artwork.
- `tools/`: current validation, launcher metadata and guarded core patch tools.
- `tests/`: host regressions, real-core audio reproducer and original LCD reference.
- `docs/`: reports, source checksums and reviewable diffs.

No romfs/ directory or duplicate asset packaging is used. Firmware filenames,
SD paths, per-firmware save compatibility, cheats, stereo graphics, 3D Camera
screenshots and diagnostics controls remain as before.

## Build options

```bash
bash build.sh                         # incremental release build
bash build.sh --clean                 # explicit cache clean
bash build.sh --no-core-patch         # don't edit a compatible core
bash build.sh --check --no-core-patch  # compile check without staging outputs
bash build.sh --audio-debug           # optional counters saved once at exit
```

The guarded v49 core patch is still recognized as already installed in the
provided core. Unknown or partial revisions stop rather than being overwritten.
The app lockfile and dependency versions are unchanged. Python 3.11+ is needed
by the validators; build.sh selects a compatible installed interpreter. Your
Homebrew Python 3.12 is supported. Set TG18_PYTHON to an interpreter path to
override selection.

## Host verification

```bash
python3 tools/validate.py --check-core
python3 -m unittest discover -s tests -p 'test_*.py' -v
cargo test --manifest-path tests/host/Cargo.toml --release --locked
cargo run --manifest-path tests/host/Cargo.toml --release --locked --bin audio-reproduction
```

The Python pipeline tests mock compiler/devkit commands. Rust host tests use
the supplied real core and test PCM timing, buffering, renderer equivalence,
photos and diagnostics. Neither replaces a fresh 3DS build and hardware test.
