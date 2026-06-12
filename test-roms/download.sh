#!/bin/sh
# Fetch the pinned test-ROM bundles into test-roms/:
#  - the mooneye-test-suite prebuilt ROM bundle (gekkio.fi)
#  - the c-sp/game-boy-test-roms aggregate collection (GitHub release zip)
# Idempotent: a bundle whose directory already exists is skipped. Both
# bundles are sha256-pinned; a mismatch deletes the archive so the next run
# re-downloads instead of staying wedged on a poisoned file.
set -eu
MTS=mts-20240926-1737-443f6e1
MTS_SHA256=d9ab11a01351e0eb2dea485237027c4dd66c0528c707d21ff78604157b967837
GBTR=game-boy-test-roms-v7.0
GBTR_URL="https://github.com/c-sp/game-boy-test-roms/releases/download/v7.0/$GBTR.zip"
GBTR_SHA256=b9a9d7a1075aa35a3d07c07c34974048672d8520dca9e07a50178f5860c3832c
cd "$(dirname "$0")"

# coreutils sha256sum is missing on stock macOS, which ships perl shasum.
# Resolve the tool up front so a host with neither fails loudly instead of
# producing an empty hash (POSIX sh has no pipefail to catch it later).
if command -v sha256sum >/dev/null 2>&1; then
    SHA_TOOL=sha256sum
elif command -v shasum >/dev/null 2>&1; then
    SHA_TOOL="shasum -a 256"
else
    echo "ERROR: need sha256sum or shasum" >&2
    exit 1
fi
sha256_of() { $SHA_TOOL "$1" | awk '{print $1}'; }

# check_sha256 <file> <want>: on mismatch, remove the file (self-heal) and die.
check_sha256() {
    got=$(sha256_of "$1")
    if [ "$got" != "$2" ]; then
        echo "ERROR: $1 sha256 mismatch:" >&2
        echo "  got  $got" >&2
        echo "  want $2" >&2
        rm -f "$1"
        echo "removed $1; re-run to re-download, or update the pin" >&2
        exit 1
    fi
}

# Extraction fallback chain: unzip (absent from Git Bash on the windows CI
# runner) -> python3/python -m zipfile -> tar -xf. bsdtar (macOS /usr/bin/tar,
# the Windows system tar) reads zip archives natively; GNU tar does not,
# hence tar is the last resort.
extract_zip() { # extract_zip <archive.zip> <dest-dir>
    mkdir -p "$2"
    if command -v unzip >/dev/null 2>&1; then
        unzip -q "$1" -d "$2"
    elif command -v python3 >/dev/null 2>&1; then
        python3 -m zipfile -e "$1" "$2"
    elif command -v python >/dev/null 2>&1; then
        python -m zipfile -e "$1" "$2"
    else
        tar -xf "$1" -C "$2"
    fi
}

if [ -d "$MTS" ]; then
    echo "$MTS already present"
else
    # curl -f: an HTTP error (404/5xx) aborts instead of saving the error
    # body as the artifact; -S surfaces curl's own message despite -s.
    [ -f "$MTS.tar.xz" ] || curl -fsSLO "https://gekkio.fi/files/mooneye-test-suite/$MTS/$MTS.tar.xz"
    check_sha256 "$MTS.tar.xz" "$MTS_SHA256"
    # Extract via a temp dir + atomic rename so an interrupted extraction
    # cannot leave a partial $MTS that the presence check would skip forever.
    rm -rf "$MTS.tmp"
    mkdir "$MTS.tmp"
    tar xf "$MTS.tar.xz" -C "$MTS.tmp"
    mv "$MTS.tmp/$MTS" "$MTS"
    rmdir "$MTS.tmp"
    echo "extracted $MTS ($(find "$MTS" -name '*.gb' | wc -l) ROMs)"
fi

if [ -d "$GBTR" ]; then
    echo "$GBTR already present"
else
    [ -f "$GBTR.zip" ] || curl -fsSLO "$GBTR_URL"
    check_sha256 "$GBTR.zip" "$GBTR_SHA256"
    # The zip has no top-level directory, so extract into an explicit
    # destination — via a temp dir, so an interrupted extraction cannot leave
    # a partial $GBTR that the presence check above would then skip forever.
    rm -rf "$GBTR.tmp"
    extract_zip "$GBTR.zip" "$GBTR.tmp"
    mv "$GBTR.tmp" "$GBTR"
    echo "extracted $GBTR ($(find "$GBTR" \( -name '*.gb' -o -name '*.gbc' \) | wc -l) ROMs)"
fi
