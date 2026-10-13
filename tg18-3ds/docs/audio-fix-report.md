# Targeted audio correction — diagnosis and verification

The supplied faster frontend was compared with the previous v49 frontend and
the actual locally patched core, including ToneSynth, timer sound events and
Machine run/sleep timing. All four required handoff documents were read before
editing. Some of those documents still describe the core as absent; it is
present in this upload and was used for the tests.

## Reproduced cause

The previous optimization correctly changed NDSP nsamples from an always-fixed
735 stereo frames to the full length of generated PCM. That change exposed a
pre-existing adaptive-stretch bug in the frontend's scheduling policy.

The old code calculates stretch from wall-clock loop time divided by emulated
sound time. It follows slow frames immediately but recovers gradually (80% of
the previous stretch plus 20% of the new ratio). ToneSynth produces exactly n
samples; it preserves oscillator pitch, but fitting the events into those n
samples lengthens their note/gap durations when n exceeds the native count.
A slow frame therefore lengthens several subsequent chunks as well.

With the revised dynamic NDSP count, those full stretched chunks play. The
four-slot output ring fills, and the old frontend then discards freshly
synthesized PCM whenever its next wave is still busy. Note changes can be
omitted from playback. The old fixed 735 count hid most excess chunk duration
by playing only each generated chunk's prefix; restoring that mismatch would
retain the underlying timing error.

A deterministic 30-frame trace using the supplied ToneSynth and synthetic
buzzer changes demonstrates this, with one 100 ms wall-clock stall:

| Measurement | Uploaded faster audio | Corrected audio |
| --- | ---: | ---: |
| Generated mono samples | 39,609 | 22,050 |
| Nominal native samples for 0.5 emulated seconds | 22,050 | 22,050 |
| Excess generation beyond legacy's integer count | 398.186 ms | 0 ms |
| Rejected chunks in the FIFO wave model | 14 | 0 |
| PCM lost to the corrected FIFO in this trace | — | 0 |

The legacy scheduler counted 22,049 native samples because its repeated
floating-point span arithmetic loses one sample in this trace. The correction
uses absolute integer sample positions and generates all 22,050.

This identifies and reproduces a code-level cause of changed audio timing.
The link to your particular audible symptom is strongly supported by the
changed playback-length behavior, but cannot be completely confirmed without
a recording or real 3DS listening. No specific perceived artifact (pitch,
static, stutter, delay, etc.) is assumed or claimed as observed.

## Alternatives checked

- V49 and the uploaded faster frontend generate byte-identical PCM for the
  same inputs before playback. Larger Vec capacity does not change samples.
- ToneSynth appends exactly n samples. Its anti-aliased square-wave algorithm,
  volume, pitch and duty handling are retained; the core is unchanged.
- Wave storage was reserved before construction. The new adapter never
  exceeds its fixed capacity, so its cached DMA pointer remains stable.
- get_buffer().len() is bytes. The DSP flush argument is not multiplied again.
  Each corrected 735-frame stereo PCM16 chunk has 2,940 bytes.
- set_sample_count and cache flushing are retained. Busy waves are not mutated.
  Free waves remain valid candidates, matching the pinned API.
- RTC turbo advances the toy clock, not the timer-event sample rate. The
  existing time multiplier and emulation budget are unchanged.

## Changes

1. Add src/audio.rs: native sample positions at 44,100 Hz from emulated time;
   no wall-clock adaptive stretch. The core oscillator keeps its original
   phase, pitch, duty, anti-aliasing and volume behavior.
2. Assemble complete 735-frame (16.667 ms) chunks in a PCM FIFO. Only consume
   a chunk after successful NDSP queueing. Buffer/sample-count/queue failures
   retain PCM for retry; failures are counted rather than discarded silently.
3. Bound the pending FIFO to 5,880 mono samples (133.333 ms). Prolonged backend
   failure drops oldest pending PCM to bound latency/memory, with every dropped
   sample recorded in overrun_samples. Healthy reproduction tests drop none.
4. Keep future timer events if a fractional sample boundary or the 2,048-sample
   render cap prevents consuming them this loop. The old code cleared them all.
5. At a power cut, append silence at the cut's actual emulated time, preserving
   the note prefix. Sleep/wake regression tests match core PCM exactly.
6. Queue two silent chunks after the cinematic fades, not before them. This
   supplies approximately 33 ms of buffering lead for routine frame jitter.
   A physical CPU stall beyond available queued audio can still underrun;
   changing future note lengths cannot repair already missed playback time.
7. Use four fixed 735-frame linear waves: 11,760 bytes in total instead of
   384,000 bytes previously reserved. The mono FIFO and render scratch use
   another 15,856 sample-storage bytes, plus small event/counter structures.
   Normal audio work/storage is bounded; the huge stretched chunks are gone.
8. Optional --audio-debug build writes one counter summary at exit to
   sdmc:/3ds/tg18-3ds/audio-status.txt. No logging or SD writes occur inside
   the audio loop. This reports queue/cache/size/buffer errors and overruns,
   not physical DSP underruns or a captured waveform.
9. build.sh prefers the supplied working /opt/devkitpro/tools/bin converter
   and supports --audio-debug, retaining incremental +nightly/--locked builds.

## Preservation

The supplied core is byte-identical, including its local idle-loop patch.
All 53 runtime assets are byte-identical and stay under assets/runtime/.
The LCD, firmware picker, photo and diagnostics modules are byte-identical.
The application's Cargo.lock is byte-identical. Main-loop edits are confined
to audio setup/output and the optional audio-status summary. Firmware choice,
RTC/emulation budget, cheats, saves, screen rendering and screenshots retain
their existing implementation. All nine firmware routes use this same
firmware-independent buzzer path; hardware boot/listening across all nine
versions is not claimed as completed.

## Verification

- 22 Rust frontend host tests passed against the full supplied tg18-core
  dependency, with registry versions/checksums matching the app lockfile.
  Thirteen tests cover audio: reproduced regression, exact core PCM, stall
  recovery, routine jitter with priming, busy/error retention, render-cap
  event preservation, fractional boundaries, sleep/wake, stereo packing and
  pointer stability, bounded failure, silent fallback and 60-second sample
  accounting. Existing nine rendering/photo/profiler tests also pass.
- The two supplied core sound detector tests additionally passed when compiling
  the exact sound module in the standalone harness.
- 17 Python tooling/build tests passed, including the --audio-debug argument
  and compatible Python discovery, explicit override, incompatible override
  rejection and a clean missing-tomllib error.
  These mock compiler/devkit binaries to test actual build-script staging and
  error behavior; they are not cross-compilation tests.
- Independent Pillow decoding verifies the existing 400×240 JPG and two-eye
  MPO dimensions, orientation and RGB channel order.
- All 53 asset sizes and core API-presence preflight pass. The existing core
  patch is recognized as already installed, with no changes needed.
- Formatting, Rust parsing, Python and shell syntax, app/core/asset byte
  preservation, source-only ZIP contents and ZIP CRC checks were completed.
- Host compilation used Rust 1.99.0. The production frontend retains its
  existing edition, allocator_api feature and pinned ctru-rs API/dependencies.

## Build and hardware boundary

No devkitPro, cargo-3ds or compatible nightly cross-compilation installation
was available here. The production 3DS frontend was not cross-compiled and its
NDSP adapter has not been exercised on physical hardware. This ZIP contains
no .3dsx/.smdh binaries. Exact Mac commands are in the bundle's root README.

Use PATH="/opt/devkitpro/tools/bin:$PATH" bash build.sh on your Mac. Verify and
hash the new READY_FOR_FTPD pair before transfer. Listen at normal 1× speed,
compare with the known-good v49 or physical toy, then compare 2D/3D and slow
transitions. Keep your current frontend/binary backup and the previous working
backup for rollback. If the result still differs, build --audio-debug and return
its status file plus a description/recording of the specific artifact; host PCM
and NDSP playback can then be distinguished without changing rendering or saves.

## Build-log follow-up: missing tomllib

The attached Mac log stops in tools/validate.py before Rust compilation:
ModuleNotFoundError: No module named 'tomllib'. Adding only devkitPro to PATH
left an older python3 selected. The earlier short build command should also
have included the user's Homebrew Python 3.12 directory.

build.sh now probes Python compatibility and uses one selected interpreter for
all five helper invocations. It checks python3, named versions and known
Homebrew paths. TG18_PYTHON can explicitly select an interpreter; invalid
explicit choices stop before compilation. Direct validator invocation under
an older Python reports a clear requirement instead of a traceback.

Four new build/tool regression tests cover this failure and correction. All
17 Python tooling tests pass. Audio and emulator source, the core, assets and
the app Cargo.lock are unchanged by this build-only follow-up. The earlier
Rust/audio results still apply; no new cross-compiled binary is claimed.
