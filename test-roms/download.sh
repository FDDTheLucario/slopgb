#!/bin/sh
# Fetch the pinned test-ROM bundles into test-roms/:
#  - the mooneye-test-suite prebuilt ROM bundle (gekkio.fi)
#  - the c-sp/game-boy-test-roms aggregate collection (GitHub release zip)
# Idempotent: a bundle whose directory already exists is skipped.
set -eu
MTS=mts-20240926-1737-443f6e1
GBTR=game-boy-test-roms-v7.0
GBTR_URL="https://github.com/c-sp/game-boy-test-roms/releases/download/v7.0/$GBTR.zip"
GBTR_SHA256=b9a9d7a1075aa35a3d07c07c34974048672d8520dca9e07a50178f5860c3832c
cd "$(dirname "$0")"

if [ -d "$MTS" ]; then
    echo "$MTS already present"
else
    curl -sLO "https://gekkio.fi/files/mooneye-test-suite/$MTS/$MTS.tar.xz"
    tar xf "$MTS.tar.xz"
    echo "extracted $MTS ($(find "$MTS" -name '*.gb' | wc -l) ROMs)"
fi

# coreutils sha256sum is missing on stock macOS, which ships perl shasum.
sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1"
    else
        shasum -a 256 "$1"
    fi | awk '{print $1}'
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

if [ -d "$GBTR" ]; then
    echo "$GBTR already present"
else
    [ -f "$GBTR.zip" ] || curl -sLO "$GBTR_URL"
    got=$(sha256_of "$GBTR.zip")
    if [ "$got" != "$GBTR_SHA256" ]; then
        echo "ERROR: $GBTR.zip sha256 mismatch:" >&2
        echo "  got  $got" >&2
        echo "  want $GBTR_SHA256" >&2
        echo "delete the zip and re-run, or update the pin" >&2
        exit 1
    fi
    # The zip has no top-level directory, so extract into an explicit
    # destination — via a temp dir, so an interrupted extraction cannot leave
    # a partial $GBTR that the presence check above would then skip forever.
    rm -rf "$GBTR.tmp"
    extract_zip "$GBTR.zip" "$GBTR.tmp"
    mv "$GBTR.tmp" "$GBTR"
    echo "extracted $GBTR ($(find "$GBTR" \( -name '*.gb' -o -name '*.gbc' \) | wc -l) ROMs)"
fi
