# Meets On 3DS cleanup and performance review

Reviewed the four active Rust files in the upload: main.rs, firmware_picker.rs,
diagnostics.rs and photo.rs; Cargo.toml/Cargo.lock; the v49 build and its
launcher/core patch tools; and every embedded asset. This package contains
revised source, documentation and tests. No fresh ARM / 3DS binary was built.

## Changes made

| Change | Reason and effect |
| --- | --- |
| One incremental build.sh | Removes the unconditional cargo clean from normal builds. --clean remains available. --locked retains dependency resolution. |
| Current tools separated from historical scripts | Replaces chained version-string validators with asset, manifest, core-presence and binary checks. Historical material is preserved under archive/. |
| Runtime assets separated from artwork | All 53 include_bytes assets remain byte-identical. Removes an additional approximately 4.42 MiB RomFS bundle containing runtime assets and editable PNGs. Actual rebuilt binary size is not measured. |
| LCD compositor extracted to src/lcd.rs | Transparent shell pixels copy RGB directly, avoiding alpha arithmetic. Destination writes visit framebuffer columns consecutively. Coordinates, stereo padding and blend rounding remain byte-identical in regression tests. |
| Two JPEG encodes per photo instead of four | Encodes each eye once and reuses the resulting JPEGs for MPO, Camera JPG and native eye files. Quality remains 93 and native size remains 400×240. |
| Picker redraws only when selection changes | Removes full-screen redraws on every unchanged nonempty-list loop. The existing single-buffer setup keeps the image visible. |
| Diagnostics Y input isolated from the game | Uses the same filtered press/held keys for highlighting and emulated key transitions. Previously the page switch still pressed the toy's Y mapping. |
| NDSP sample length corrected on refill | Resizing the Vec did not update the Wave's initial 735-sample count. Calls set_sample_count with the actual rendered length before queueing. |
| DSP cache flushed after refilling a wave | Wave::new only flushes initial data. New PCM data is flushed before requeueing; the service return is handled like the library's initial flush. |
| Free audio waves accepted as well as finished waves | Avoids permanently blocking the ring if the initial queue failed and left a wave Free. |
| Audio scratch capacity allocated once | Reserves AUDIO_BUF_CAP (24,000 mono samples), covering the existing maximum stretched output and avoiding growth during a slow frame. This costs about 44 KiB more initial scratch capacity. |
| Profiler begins after startup fades | Removes the intro from the first rolling FPS denominator. The existing ten-second buckets and idle-counter meanings remain. |
| Failed automatic saves show the existing failure toast | Makes failed background writes or thread creation visible while retaining retry behavior. |
| Core patch guard strengthened | Retains the original v49 HashSet algorithm, checks all three already-patched sections, rejects partial/unknown revisions, preserves backups, and writes through a temporary file. The core itself was absent. |
| Rust formatting and dead frontend code removed | Removes the unused present_bottom wrapper, obsolete bottom save-notice parameters and redundant initial assignments. |

The existing firmware filenames, GP address/range checks, per-firmware save
paths, minimum key hold, RTC catch-up, emulation budget and idle-skip decisions
are retained. Broader changes to CPU emulation and RTC/audio timing require the
missing core and hardware measurements.

## Verification completed

- 9 optimized host Rust regression tests passed, compiling the actual LCD,
  photo and profiler modules with Rust 1.99.0 and the exact jpeg-encoder 0.6.1
  source/checksum from Cargo.lock.
- Renderer comparisons cover all three actual shell overlays, eight LCD
  patterns, every possible alpha value, all output pixels, and untouched
  background bytes against the original v49 algorithm.
- Photo tests cover MPF image lengths and offsets, exact reuse of eye JPEG
  data, invalid input, leap-day conversion, and exclusive file creation.
- Pillow independently decoded the generated native JPG, Camera JPG and
  two-eye MPO: 400×240, correct eye count, RGB channel order and orientation.
- 12 Python regression tests passed: matching/unknown/partial core patches,
  backup preservation and rerun behavior, launcher metadata in 16 languages,
  invalid binary/asset rejection, incremental/clean/check builds and failure
  staging. Build tests use fake compiler/devkit tools; they are not ARM tests.
- All 53 active asset sizes passed validation. Preserved moved files were
  compared byte-for-byte with the upload. Original binaries match the upload.
- Rust formatting, syntax parsing, Python compilation and shell syntax checks
  passed. The original dependency lockfile is preserved.
- An actual build preflight correctly stopped at the missing sibling core.
  Full frontend type checking/linking, NDSP behavior, picker display persistence,
  SD save behavior and Camera album compatibility remain hardware checks.

## Isolated desktop render benchmark

One optimized desktop run, 1,000 draws per view, in microseconds:

| View | Original | Revised | Reduced time |
| --- | ---: | ---: | ---: |
| Flat LCD | 77,592 | 34,160 | 56.0% |
| Left stereo LCD | 97,854 | 50,556 | 48.3% |
| Right stereo LCD | 95,927 | 50,891 | 46.9% |

These timings measure only the LCD compositor on the desktop CPU in this
session. They do not establish a 3DS speedup or whole-emulator FPS. JPEG encode
calls are halved structurally; whole-photo time also includes filesystem writes
and metadata work, so a 50% total saving is not claimed.

## Follow-up areas requiring the core or hardware

1. Measure emulation versus top/bottom draw time with diagnostics closed for
   ten seconds before opening the overlay. Compare identical scenes on your
   original and revised builds, both in 2D and 3D.
2. Inspect Machine::lcd_rgb for a caller-owned output API; the frontend still
   receives a newly allocated Vec for each real LCD conversion.
3. Confirm the core's ToneSynth event consumption and audio-time behavior when
   AUDIO_FRAME_CAP is reached. The event handling and stretch policy were
   deliberately retained because their contracts were unavailable.
4. Investigate CPU/idle-skip hotspots only after measuring the current core.
   The provided v49 patch already replaces a quadratic address scan with a
   reusable set; this cleanup does not invent another core optimization.
5. Improve fault reporting once Outcome's actual definition is available.
   The existing frontend still pauses when the core reports an unexpected
   outcome, without displaying its reason.

For a full core review, provide tg18-emu-main/core (Cargo.toml and src/). Your
existing nightly/devkitPro setup and a 3DS are needed for final platform checks.

## File organization and rollback

The approximately 252 MiB target/ build cache, macOS resource-fork metadata and
empty Git scaffolding were omitted. Historical scripts, previews, unused binary
assets and backups remain in archive/. The four original active Rust files and
manifests are additionally preserved in archive/source-baseline-v49/. Original
installable binaries are in archive/prebuilt-original/ and contain no new fixes.

file-moves.json maps the original extracted project files to their new locations.
code-changes.patch compares the active Rust files after normalizing formatting,
so semantic changes are easier to review. The root README explains the new
build entry point, current folder layout and validation commands.

## API references checked

- [Pinned ctru-rs Wave source](https://github.com/rust3ds/ctru-rs/blob/9576b695897a7474829f490d696807b26a208747/ctru-rs/src/services/ndsp/wave.rs): buffer construction, status and set_sample_count.
- [Pinned ctru-rs channel source](https://github.com/rust3ds/ctru-rs/blob/9576b695897a7474829f490d696807b26a208747/ctru-rs/src/services/ndsp/mod.rs): queue_wave does not resize or refresh the sample count.
- [cargo-3ds source](https://github.com/rust3ds/cargo-3ds/blob/master/src/lib.rs): RomFS is bundled when the configured/default directory exists.
