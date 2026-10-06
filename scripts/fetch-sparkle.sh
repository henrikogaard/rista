#!/usr/bin/env bash
# Fetch Sparkle.framework + signing tools into vendor/sparkle (gitignored).
# Version is pinned to the same release LinkRouter uses.
set -euo pipefail
cd "$(dirname "$0")/.."

SPARKLE_VERSION="${SPARKLE_VERSION:-2.6.4}"
SPARKLE_SHA256="${SPARKLE_SHA256:-50612a06038abc931f16011d7903b8326a362c1074dabccb718404ce8e585f0b}"
TARBALL="$(mktemp /tmp/sparkle.XXXXXX.tar.xz)"
trap 'rm -f "$TARBALL"' EXIT

curl -fsSL -o "$TARBALL" \
  "https://github.com/sparkle-project/Sparkle/releases/download/${SPARKLE_VERSION}/Sparkle-${SPARKLE_VERSION}.tar.xz"
echo "${SPARKLE_SHA256}  ${TARBALL}" | shasum -a 256 -c -

WORK="$(mktemp -d /tmp/sparkle-extract.XXXXXX)"
trap 'rm -f "$TARBALL"; rm -rf "$WORK"' EXIT
tar -xJf "$TARBALL" -C "$WORK"

mkdir -p vendor/sparkle
rm -rf vendor/sparkle/Sparkle.framework vendor/sparkle/bin
cp -a "$WORK/Sparkle.framework" vendor/sparkle/
cp -a "$WORK/bin" vendor/sparkle/
echo "Sparkle ${SPARKLE_VERSION} vendored at vendor/sparkle"
