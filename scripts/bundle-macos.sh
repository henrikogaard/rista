#!/usr/bin/env bash
# Build Rista.app with Sparkle and package a ZIP and DMG.
#
# Local builds default to ad-hoc signing. NOTARIZE=1 requires a real
# Developer ID identity and App Store Connect API-key authentication.
set -euo pipefail
cd "$(dirname "$0")/.."

# Keep the bundle path ASCII: Sparkle compares it with LaunchServices paths
# without Unicode normalization. CFBundleDisplayName retains the product name.
APP_NAME="Rista"
EXEC_NAME="rista"
VERSION="${VERSION:-$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)"/\1/')}"
VERSION="${VERSION#v}"
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || {
  echo "VERSION must be a numeric major.minor.patch version" >&2
  exit 1
}

BUILD="${BUILD:-$VERSION}"
IDENTITY="${CODESIGN_IDENTITY:--}"
NOTARIZE="${NOTARIZE:-0}"
[[ "$NOTARIZE" == "0" || "$NOTARIZE" == "1" ]] || {
  echo "NOTARIZE must be 0 or 1" >&2
  exit 1
}

if [[ "$NOTARIZE" == "1" ]]; then
  [[ -n "$IDENTITY" && "$IDENTITY" != "-" ]] || {
    echo "NOTARIZE=1 requires a real CODESIGN_IDENTITY" >&2
    exit 1
  }
  for name in APPSTORE_API_KEY_PATH APPSTORE_API_KEY_ID APPSTORE_ISSUER_ID; do
    [[ -n "${!name:-}" ]] || {
      echo "NOTARIZE=1 requires ${name}" >&2
      exit 1
    }
  done
  [[ -r "$APPSTORE_API_KEY_PATH" ]] || {
    echo "APPSTORE_API_KEY_PATH must reference a readable file" >&2
    exit 1
  }
  [[ -n "${SPARKLE_PRIVATE_ED_KEY:-}" ]] || {
    echo "NOTARIZE=1 requires SPARKLE_PRIVATE_ED_KEY" >&2
    exit 1
  }
fi

test -d vendor/sparkle/Sparkle.framework || {
  echo "Sparkle.framework missing — run scripts/fetch-sparkle.sh first" >&2
  exit 1
}

DIST="dist"
APP="${DIST}/${APP_NAME}.app"
ARCHIVE="Rista-${VERSION}.zip"
DMG="Rista-${VERSION}.dmg"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/rista-release.XXXXXX")"
OUTPUT="${WORK}/output"
trap 'rm -rf "$WORK"' EXIT
mkdir -p "$DIST" "$OUTPUT"

rm -f \
  "${DIST}/${ARCHIVE}" \
  "${DIST}/${DMG}" \
  "${DIST}/SHA256SUMS" \
  "${DIST}/appcast.xml"

NOTARY_AUTH=()
if [[ "$NOTARIZE" == "1" ]]; then
  NOTARY_AUTH=(
    --key "$APPSTORE_API_KEY_PATH"
    --key-id "$APPSTORE_API_KEY_ID"
    --issuer "$APPSTORE_ISSUER_ID"
  )
  xcrun notarytool history \
    "${NOTARY_AUTH[@]}" \
    --output-format json >"${WORK}/notary-auth.json"
fi

cargo build --locked --release --features sparkle

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Frameworks" "$APP/Contents/Resources"
cp "target/release/${EXEC_NAME}" "$APP/Contents/MacOS/${EXEC_NAME}"
ditto vendor/sparkle/Sparkle.framework "$APP/Contents/Frameworks/Sparkle.framework"
sed "s/@VERSION@/${VERSION}/g; s/@BUILD@/${BUILD}/g" \
  macos/Info.plist >"$APP/Contents/Info.plist"

cp LICENSE "$APP/Contents/Resources/LICENSE"
cp third_party/KaTeX-fonts-OFL.txt "$APP/Contents/Resources/KaTeX-fonts-OFL.txt"
ICONSET="${WORK}/AppIcon.iconset"
mkdir -p "$ICONSET"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" public/icon.png \
    --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
  double=$((size * 2))
  sips -z "$double" "$double" public/icon.png \
    --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/AppIcon.icns"

SPARKLE_FRAMEWORK="$APP/Contents/Frameworks/Sparkle.framework"
SPARKLE_VERSION="$SPARKLE_FRAMEWORK/Versions/B"
INSTALLER="$SPARKLE_VERSION/XPCServices/Installer.xpc"
DOWNLOADER="$SPARKLE_VERSION/XPCServices/Downloader.xpc"
AUTOUPDATE="$SPARKLE_VERSION/Autoupdate"
UPDATER="$SPARKLE_VERSION/Updater.app"

extract_entitlements() {
  local component="$1"
  local output="$2"
  codesign -d --entitlements :- "$component" >"$output" 2>/dev/null
  plutil -lint "$output" >/dev/null
}

sign_code() {
  local target="$1"
  local entitlements="${2:-}"
  local args=(--force --sign "$IDENTITY")

  if [[ "$IDENTITY" == "-" ]]; then
    args+=(--timestamp=none)
  else
    args+=(--options runtime --timestamp)
  fi
  if [[ -n "$entitlements" ]]; then
    args+=(--entitlements "$entitlements")
  fi
  codesign "${args[@]}" "$target"
}

INSTALLER_ENTITLEMENTS="${WORK}/Installer.entitlements"
DOWNLOADER_ENTITLEMENTS="${WORK}/Downloader.entitlements"
extract_entitlements "$INSTALLER" "$INSTALLER_ENTITLEMENTS"
extract_entitlements "$DOWNLOADER" "$DOWNLOADER_ENTITLEMENTS"

sign_code "$INSTALLER" "$INSTALLER_ENTITLEMENTS"
sign_code "$DOWNLOADER" "$DOWNLOADER_ENTITLEMENTS"
sign_code "$AUTOUPDATE"
sign_code "$UPDATER"
sign_code "$SPARKLE_FRAMEWORK"
sign_code "$APP"
codesign --verify --deep --strict --verbose=2 "$APP"

json_value() {
  local path="$1"
  local key="$2"
  /usr/bin/python3 - "$path" "$key" <<'PY'
import json
import sys

try:
    with open(sys.argv[1], encoding="utf-8") as handle:
        value = json.load(handle).get(sys.argv[2], "")
except (OSError, json.JSONDecodeError, AttributeError):
    value = ""
if value is not None:
    print(value)
PY
}

fetch_notary_log() {
  local submission_id="$1"
  local label="$2"
  local log_path="${WORK}/notary-${label}-log.json"

  [[ -n "$submission_id" ]] || return 0
  if xcrun notarytool log \
    "${NOTARY_AUTH[@]}" \
    "$submission_id" "$log_path" >/dev/null 2>&1; then
    echo "notarytool log for ${label}:" >&2
    cat "$log_path" >&2
  else
    echo "notarytool log is not available for ${label} submission ${submission_id}" >&2
  fi
}

notarize_file() {
  local path="$1"
  local label="$2"
  local result="${WORK}/notary-${label}-result.json"
  local errors="${WORK}/notary-${label}-stderr.log"
  local exit_code
  local status
  local submission_id

  set +e
  xcrun notarytool submit "$path" \
    "${NOTARY_AUTH[@]}" \
    --wait \
    --timeout 30m \
    --output-format json >"$result" 2>"$errors"
  exit_code=$?
  set -e

  if [[ -s "$result" ]]; then
    echo "notarytool result for ${label}:" >&2
    cat "$result" >&2
  fi
  status="$(json_value "$result" status)"
  submission_id="$(json_value "$result" id)"

  if [[ "$exit_code" -ne 0 || "$status" != "Accepted" ]]; then
    if [[ -s "$errors" ]]; then
      cat "$errors" >&2
    fi
    fetch_notary_log "$submission_id" "$label"
    echo "Notarization failed for ${label}; status=${status:-unavailable}; submission=${submission_id:-unavailable}" >&2
    return 1
  fi
}

if [[ "$NOTARIZE" == "1" ]]; then
  INTERMEDIATE_ZIP="${WORK}/Rista-${VERSION}-notary.zip"
  ditto -c -k --keepParent "$APP" "$INTERMEDIATE_ZIP"
  notarize_file "$INTERMEDIATE_ZIP" app
  xcrun stapler staple "$APP"
  xcrun stapler validate "$APP"
  spctl --assess --type execute --verbose=4 "$APP"
fi

FINAL_ZIP="${OUTPUT}/${ARCHIVE}"
ditto -c -k --keepParent "$APP" "$FINAL_ZIP"

DMG_STAGE="${WORK}/dmg"
mkdir -p "$DMG_STAGE"
ditto "$APP" "$DMG_STAGE/${APP_NAME}.app"
ln -s /Applications "$DMG_STAGE/Applications"
codesign --verify --deep --strict "$DMG_STAGE/${APP_NAME}.app"

FINAL_DMG="${OUTPUT}/${DMG}"
hdiutil create \
  -volname "Rísta" \
  -srcfolder "$DMG_STAGE" \
  -format UDZO \
  -imagekey zlib-level=9 \
  -o "$FINAL_DMG"

if [[ "$NOTARIZE" == "1" ]]; then
  codesign --force --sign "$IDENTITY" --timestamp "$FINAL_DMG"
  notarize_file "$FINAL_DMG" dmg
  xcrun stapler staple "$FINAL_DMG"
  xcrun stapler validate "$FINAL_DMG"
  spctl --assess \
    --type open \
    --context context:primary-signature \
    --verbose=4 \
    "$FINAL_DMG"
fi
hdiutil verify "$FINAL_DMG"

(
  cd "$OUTPUT"
  shasum -a 256 "$ARCHIVE" "$DMG" >SHA256SUMS
)

if [[ -n "${SPARKLE_PRIVATE_ED_KEY:-}" ]]; then
  FEED_INPUT="${WORK}/feed"
  mkdir -p "$FEED_INPUT"
  cp "$FINAL_ZIP" "$FEED_INPUT/$ARCHIVE"
  printf '%s' "$SPARKLE_PRIVATE_ED_KEY" \
    | vendor/sparkle/bin/generate_appcast \
      --ed-key-file - \
      --download-url-prefix "https://github.com/henrikogaard/rista/releases/download/v${VERSION}/" \
      "$FEED_INPUT"
  mv "$FEED_INPUT/appcast.xml" "$OUTPUT/appcast.xml"
  echo "wrote dist/appcast.xml"
else
  echo "SPARKLE_PRIVATE_ED_KEY unset — skipped appcast.xml" >&2
fi

mv "$FINAL_ZIP" "${DIST}/${ARCHIVE}"
mv "$FINAL_DMG" "${DIST}/${DMG}"
mv "$OUTPUT/SHA256SUMS" "${DIST}/SHA256SUMS"
if [[ -f "$OUTPUT/appcast.xml" ]]; then
  mv "$OUTPUT/appcast.xml" "${DIST}/appcast.xml"
fi

echo "built ${APP}, dist/${ARCHIVE}, and dist/${DMG}"
