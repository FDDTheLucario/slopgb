#!/bin/sh
# Fetch the SingleStepTests 65816 processor-test vectors into
# test-roms/65816-tests/v1/ (gitignored — NEVER commit these, they are ~2 GB).
# Each file is v1/<opcode-hex>.<e|n>.json: 10000 cases of initial->final
# regs+RAM+cycles, for emulation (.e) and native (.n) mode. These are the
# clean-room TDD oracle for slopgb-w65c816 (test data, not emulator source).
#
# Usage:
#   test-roms/download-65816-tests.sh            # all 256 opcodes x {e,n}
#   test-roms/download-65816-tests.sh ea a9 69   # just these opcodes (dev loop)
# Idempotent: an already-present, non-empty file is skipped.
set -eu
BASE="https://raw.githubusercontent.com/SingleStepTests/65816/main/v1"
cd "$(dirname "$0")"
DEST=65816-tests/v1
mkdir -p "$DEST"

# Opcode list: CLI args if given, else all 00..ff.
if [ "$#" -gt 0 ]; then
    OPCODES="$*"
else
    OPCODES=$(printf '%02x ' $(seq 0 255))
fi

fetch() { # fetch <opcode-hex> <mode-letter>
    name="$1.$2.json"
    out="$DEST/$name"
    if [ -s "$out" ]; then
        return 0
    fi
    # -f: fail (non-zero) on HTTP error instead of writing the error body.
    if ! curl -fsSL "$BASE/$name" -o "$out.tmp"; then
        echo "ERROR: failed to fetch $name" >&2
        rm -f "$out.tmp"
        exit 1
    fi
    mv "$out.tmp" "$out"
}

n=0
for op in $OPCODES; do
    fetch "$op" e
    fetch "$op" n
    n=$((n + 1))
done
echo "65816 vectors ready in $DEST ($n opcode(s) x {e,n})"
