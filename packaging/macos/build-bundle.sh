#!/usr/bin/env bash
# Builds GeminiDesk.app and GeminiDesk-<version>-macos-<arch>.dmg on macOS.
#
# Code signing & notarization (fail-closed / transparent):
# - If MACOS_SIGNING_IDENTITY is set, signs the bundle with Developer ID + Hardened Runtime.
# - If APPLE_ID, APPLE_TEAM_ID, and APPLE_APP_PASSWORD (or NOTARY_KEYCHAIN_PROFILE) are set,
#   submits the DMG for Apple notarization and staples the ticket.
# - Without signing credentials, creates an unsigned development/CI bundle and clearly warns
#   that unsigned builds are NOT production-ready for unrestricted Gatekeeper distribution.
set -euo pipefail

VERSION="${1:-0.4.0}"
ARCH="${2:-aarch64}"
BIN_PATH="${3:-}"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT_DIR="${REPO_ROOT}/dist"

if [[ -z "${BIN_PATH}" ]]; then
  BIN_PATH="${REPO_ROOT}/target/release/gemini-desk"
fi
if [[ ! -f "${BIN_PATH}" ]]; then
  echo "Release binary not found at ${BIN_PATH}" >&2
  exit 1
fi

mkdir -p "${OUT_DIR}"
APP_BUNDLE="${OUT_DIR}/GeminiDesk.app"
rm -rf "${APP_BUNDLE}"
mkdir -p "${APP_BUNDLE}/Contents/MacOS" "${APP_BUNDLE}/Contents/Resources"

cp "${BIN_PATH}" "${APP_BUNDLE}/Contents/MacOS/gemini-desk"
chmod +x "${APP_BUNDLE}/Contents/MacOS/gemini-desk"

sed "s/@VERSION@/${VERSION}/g" "${REPO_ROOT}/packaging/macos/Info.plist.template" \
  > "${APP_BUNDLE}/Contents/Info.plist"
plutil -lint "${APP_BUNDLE}/Contents/Info.plist"

cp "${REPO_ROOT}/THIRD_PARTY_LICENSES.md" "${APP_BUNDLE}/Contents/Resources/THIRD_PARTY_LICENSES.md"

# Build crisp multi-resolution GeminiDesk.icns from our monochrome 512px PNG
ICONSET_DIR="${OUT_DIR}/GeminiDesk.iconset"
rm -rf "${ICONSET_DIR}"
mkdir -p "${ICONSET_DIR}"
SRC_PNG="${REPO_ROOT}/assets/icons/geminidesk-512.png"
if command -v sips >/dev/null 2>&1 && command -v iconutil >/dev/null 2>&1; then
  sips -z 16 16     "${SRC_PNG}" --out "${ICONSET_DIR}/icon_16x16.png"      >/dev/null
  sips -z 32 32     "${SRC_PNG}" --out "${ICONSET_DIR}/icon_16x16@2x.png"   >/dev/null
  sips -z 32 32     "${SRC_PNG}" --out "${ICONSET_DIR}/icon_32x32.png"      >/dev/null
  sips -z 64 64     "${SRC_PNG}" --out "${ICONSET_DIR}/icon_32x32@2x.png"   >/dev/null
  sips -z 128 128   "${SRC_PNG}" --out "${ICONSET_DIR}/icon_128x128.png"    >/dev/null
  sips -z 256 256   "${SRC_PNG}" --out "${ICONSET_DIR}/icon_128x128@2x.png" >/dev/null
  sips -z 256 256   "${SRC_PNG}" --out "${ICONSET_DIR}/icon_256x256.png"    >/dev/null
  sips -z 512 512   "${SRC_PNG}" --out "${ICONSET_DIR}/icon_256x256@2x.png" >/dev/null
  cp "${SRC_PNG}"                      "${ICONSET_DIR}/icon_512x512.png"
  iconutil -c icns "${ICONSET_DIR}" -o "${APP_BUNDLE}/Contents/Resources/GeminiDesk.icns"
  rm -rf "${ICONSET_DIR}"
fi

if [[ -n "${MACOS_SIGNING_IDENTITY:-}" ]]; then
  echo "Signing ${APP_BUNDLE} with Developer ID + Hardened Runtime..."
  codesign --deep --force --options runtime --timestamp \
    --entitlements "${REPO_ROOT}/packaging/macos/entitlements.plist" \
    --sign "${MACOS_SIGNING_IDENTITY}" \
    "${APP_BUNDLE}"
  codesign --verify --deep --strict --verbose=2 "${APP_BUNDLE}"
else
  echo "WARNING: MACOS_SIGNING_IDENTITY is not set. Bundle is UNSIGNED (development/CI only; not Gatekeeper-ready)."
fi

# Package compressed DMG with Applications drag-install link
DMG_STAGE="${OUT_DIR}/dmg-stage"
rm -rf "${DMG_STAGE}"
mkdir -p "${DMG_STAGE}"
cp -R "${APP_BUNDLE}" "${DMG_STAGE}/GeminiDesk.app"
ln -s /Applications "${DMG_STAGE}/Applications"

DMG_PATH="${OUT_DIR}/GeminiDesk-${VERSION}-macos-${ARCH}.dmg"
rm -f "${DMG_PATH}"
hdiutil create -volname "GeminiDesk ${VERSION}" \
  -srcfolder "${DMG_STAGE}" \
  -ov -format UDZO "${DMG_PATH}"
rm -rf "${DMG_STAGE}"

if [[ -n "${MACOS_SIGNING_IDENTITY:-}" ]]; then
  codesign --force --timestamp --sign "${MACOS_SIGNING_IDENTITY}" "${DMG_PATH}"
fi

if [[ -n "${APPLE_ID:-}" && -n "${APPLE_TEAM_ID:-}" && -n "${APPLE_APP_PASSWORD:-}" ]]; then
  echo "Submitting ${DMG_PATH} to Apple notarization service..."
  xcrun notarytool submit "${DMG_PATH}" \
    --apple-id "${APPLE_ID}" \
    --team-id "${APPLE_TEAM_ID}" \
    --password "${APPLE_APP_PASSWORD}" \
    --wait
  xcrun stapler staple "${DMG_PATH}"
  spctl --assess --type open --context context:primary-signature -v "${DMG_PATH}" || true
else
  echo "INFO: Apple notarization credentials not configured; skipped notarization."
fi

echo "Built macOS DMG: ${DMG_PATH}"
