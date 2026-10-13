#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
clean=false
patch_core=true
check_only=false
audio_debug=false
for arg in "$@"; do
  case "$arg" in
    --clean) clean=true ;;
    --no-core-patch) patch_core=false ;;
    --check) check_only=true ;;
    --audio-debug) audio_debug=true ;;
    --help)
      echo 'Usage: bash build.sh [--clean] [--no-core-patch] [--check] [--audio-debug]'
      echo 'Default: incremental release build, guarded v49 core patch, verified launcher metadata.'
      exit 0 ;;
    *) echo "ERROR: unknown option: $arg" >&2; exit 2 ;;
  esac
done
# macOS may resolve python3 to Apple's older interpreter even when Homebrew
# Python is installed. All build helpers need Python 3.11+ and tomllib.
supports_build_python() {
  "$1" -c 'import sys, tomllib; sys.exit(0 if sys.version_info >= (3, 11) else 1)' >/dev/null 2>&1
}
tg18_python=""
if [ -n "${TG18_PYTHON:-}" ]; then
  if supports_build_python "$TG18_PYTHON"; then
    tg18_python="$TG18_PYTHON"
  else
    echo 'ERROR: TG18_PYTHON must name a working Python 3.11+ interpreter with tomllib.' >&2
    exit 1
  fi
else
  for candidate in python3 python3.12 python3.11 python3.13 python3.14 \
    /opt/homebrew/opt/python@3.12/libexec/bin/python3 \
    /usr/local/opt/python@3.12/libexec/bin/python3 \
    /opt/homebrew/bin/python3 /usr/local/bin/python3; do
    if command -v "$candidate" >/dev/null 2>&1 && supports_build_python "$candidate"; then
      tg18_python="$candidate"
      break
    fi
  done
  if [ -z "$tg18_python" ]; then
    echo 'ERROR: Python 3.11+ with tomllib was not found.' >&2
    echo 'Set TG18_PYTHON to your installed Homebrew Python, then run bash build.sh again.' >&2
    exit 1
  fi
fi
echo "Build Python: $tg18_python"
# Prefer the working devkitPro converter over the incompatible cxitool copy.
if [ -x /opt/devkitpro/tools/bin/3dsxtool ]; then
  export PATH="/opt/devkitpro/tools/bin:$PATH"
fi
cargo_flags=(--release --locked)
if "$audio_debug"; then cargo_flags+=(--features audio-debug); fi
"$tg18_python" tools/validate.py --check-core
if ! command -v cargo >/dev/null 2>&1; then
  echo 'ERROR: cargo is missing; install your Rust nightly / devkitPro 3DS toolchain.' >&2
  exit 1
fi
cargo +nightly 3ds --version >/dev/null
if ! "$check_only" && ! command -v smdhtool >/dev/null 2>&1; then
  echo 'ERROR: smdhtool is missing from PATH (devkitPro).' >&2
  exit 1
fi
if "$patch_core"; then "$tg18_python" tools/patch_core.py; fi
if "$clean"; then cargo +nightly clean; fi
if "$check_only"; then
  cargo +nightly 3ds check "${cargo_flags[@]}"
  exit 0
fi
cargo +nightly 3ds build "${cargo_flags[@]}"
out_dir="${CARGO_TARGET_DIR:-target}/armv6k-nintendo-3ds/release"
out="$out_dir/tg18-3ds.3dsx"
if [ ! -s "$out" ]; then
  echo 'ERROR: cargo did not generate a .3dsx.' >&2
  exit 1
fi
"$tg18_python" tools/fix_launcher.py --out-dir "$out_dir"
"$tg18_python" tools/validate.py --binary "$out"
mkdir -p READY_FOR_FTPD
cp "$out" READY_FOR_FTPD/tg18-3ds.3dsx
cp "$out_dir/tg18-3ds.smdh" READY_FOR_FTPD/tg18-3ds.smdh
"$tg18_python" tools/validate.py --binary READY_FOR_FTPD/tg18-3ds.3dsx
echo 'Build ready: READY_FOR_FTPD/tg18-3ds.3dsx and tg18-3ds.smdh'
