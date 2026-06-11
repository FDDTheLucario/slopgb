#!/bin/sh
# Fetch the pinned mooneye-test-suite prebuilt ROM bundle into test-roms/.
set -eu
MTS=mts-20240926-1737-443f6e1
cd "$(dirname "$0")"
[ -d "$MTS" ] && { echo "$MTS already present"; exit 0; }
curl -sLO "https://gekkio.fi/files/mooneye-test-suite/$MTS/$MTS.tar.xz"
tar xf "$MTS.tar.xz"
echo "extracted $MTS ($(find "$MTS" -name '*.gb' | wc -l) ROMs)"
