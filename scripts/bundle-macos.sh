#!/usr/bin/env bash
# Build Rísta.app (release binary + Sparkle.framework) and a dist/ zip.
#
#   scripts/fetch-sparkle.sh   # once, or when bumping the pinned version
#   scripts/bundle-macos.sh
#
# Env:
#   VERSION            marketing version (default: Cargo.toml package version)
#   CODESIGN_IDENTITY  signing identity (default: "-" ad-hoc)
#   SPARKLE_PRIVATE_ED_KEY  base64 EdDSA seed; when set, also emits
#                      dist/appcast.xml signed for the feed.
set -euo pipefail
cd "$(dirname "$0")/.."

APP_NAME="Rísta"
EXEC_NAME="rista"
VERSION="${VERSION:-$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)"/\1/')}"
VERSION="${VERSION#v}"
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || {
  echo "VERSION must be a numeric major.minor.patch version" >&2
  exit 1
}
BUILD="${BUILD:-$VERSION}"
IDENTITY="${CODESIGN_IDENTITY:--}"
APP="dist/${APP_NAME}.app"
ARCHIVE="Rista-${VERSION}.zip"

test -d vendor/sparkle/Sparkle.framework || {
  echo "Sparkle.framework missing — run scripts/fetch-sparkle.sh first" >&2
  exit 1
}

cargo build --release --features sparkle

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Frameworks" "$APP/Contents/Resources"
cp "target/release/${EXEC_NAME}" "$APP/Contents/MacOS/${EXEC_NAME}"
ditto vendor/sparkle/Sparkle.framework "$APP/Contents/Frameworks/Sparkle.framework"
# The framework loads its own helpers via @rpath — XPC services ride along
# in the same copy; sign them inside-out below.
sed "s/@VERSION@/${VERSION}/g; s/@BUILD@/${BUILD}/g" \
  macos/Info.plist > "$APP/Contents/Info.plist"

cp LICENSE "$APP/Contents/Resources/LICENSE"
ICONSET="dist/AppIcon.iconset"
mkdir -p "$ICONSET"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" public/icon.png --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
  double=$((size * 2))
  sips -z "$double" "$double" public/icon.png --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/AppIcon.icns"
rm -rf "$ICONSET"

# Inside-out: helpers first, then framework, then the bundle.
find "$APP/Contents/Frameworks/Sparkle.framework" \
  \( -name "*.xpc" -o -name "*.app" -o -name Autoupdate \) -prune -type d \
  | while read -r helper; do
      codesign --force --sign "$IDENTITY" --timestamp=none "$helper"
    done
codesign --force --sign "$IDENTITY" --timestamp=none \
  "$APP/Contents/Frameworks/Sparkle.framework"
codesign --force --deep --sign "$IDENTITY" "$APP"
codesign --verify --deep --strict "$APP"

ditto -c -k --keepParent "$APP" "dist/${ARCHIVE}"
(cd dist && shasum -a 256 "$ARCHIVE" > SHA256SUMS)

# Appcast: only when the EdDSA seed is present (CI secret or local export).
if [[ -n "${SPARKLE_PRIVATE_ED_KEY:-}" ]]; then
  printf '%s' "${SPARKLE_PRIVATE_ED_KEY}" \
    | vendor/sparkle/bin/generate_appcast --ed-key-file - \
      --download-url-prefix "https://github.com/henrikogaard/rista/releases/download/v${VERSION}/" dist/
  echo "wrote dist/appcast.xml"
else
  echo "SPARKLE_PRIVATE_ED_KEY unset — skipped appcast.xml" >&2
fi

echo "built ${APP} and dist/${ARCHIVE}"
