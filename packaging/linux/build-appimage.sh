#!/usr/bin/env bash
# Builds a lean Linux AppImage (or tarball fallback) for GeminiDesk.
#
# Portability note (honest by design):
# GeminiDesk relies on the host system's native WebKitGTK 4.1, GTK3, OpenSSL,
# and xdg-desktop-portal libraries rather than bundling a 200+ MB Chromium runtime.
# Required host packages:
#   - Arch / CachyOS: webkit2gtk-4.1 gtk3 openssl xdg-desktop-portal (plus xdg-desktop-portal-kde on KDE)
#   - Ubuntu 24.04 / Debian: libwebkit2gtk-4.1-0 libgtk-3-0 xdg-desktop-portal
#   - Fedora: webkit2gtk4.1 gtk3 xdg-desktop-portal
set -euo pipefail

VERSION="${1:-0.4.0}"
ARCH="${2:-x86_64}"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
BIN_PATH="${REPO_ROOT}/target/release/gemini-desk"
OUT_DIR="${REPO_ROOT}/dist"
APPDIR="${OUT_DIR}/GeminiDesk.AppDir"

if [[ ! -f "${BIN_PATH}" ]]; then
  echo "Release binary not found at ${BIN_PATH}. Run 'cargo build --release' first." >&2
  exit 1
fi

rm -rf "${APPDIR}"
mkdir -p \
  "${APPDIR}/usr/bin" \
  "${APPDIR}/usr/share/applications" \
  "${APPDIR}/usr/share/metainfo" \
  "${APPDIR}/usr/share/icons/hicolor/scalable/apps" \
  "${APPDIR}/usr/share/icons/hicolor/256x256/apps"

install -m755 "${BIN_PATH}" "${APPDIR}/usr/bin/gemini-desk"
install -m644 "${REPO_ROOT}/packaging/linux/io.geminidesk.GeminiDesk.desktop" \
  "${APPDIR}/usr/share/applications/io.geminidesk.GeminiDesk.desktop"
install -m644 "${REPO_ROOT}/packaging/linux/io.geminidesk.GeminiDesk.desktop" \
  "${APPDIR}/io.geminidesk.GeminiDesk.desktop"
install -m644 "${REPO_ROOT}/packaging/linux/io.geminidesk.GeminiDesk.metainfo.xml" \
  "${APPDIR}/usr/share/metainfo/io.geminidesk.GeminiDesk.metainfo.xml"
install -m644 "${REPO_ROOT}/assets/icons/geminidesk.svg" \
  "${APPDIR}/usr/share/icons/hicolor/scalable/apps/io.geminidesk.GeminiDesk.svg"
install -m644 "${REPO_ROOT}/assets/icons/geminidesk-256.png" \
  "${APPDIR}/usr/share/icons/hicolor/256x256/apps/io.geminidesk.GeminiDesk.png"
install -m644 "${REPO_ROOT}/assets/icons/geminidesk-256.png" \
  "${APPDIR}/io.geminidesk.GeminiDesk.png"

cat > "${APPDIR}/AppRun" <<'EOF'
#!/usr/bin/env bash
HERE="$(dirname "$(readlink -f "${0}")")"
exec "${HERE}/usr/bin/gemini-desk" "$@"
EOF
chmod +x "${APPDIR}/AppRun"

OUT_FILE="${OUT_DIR}/GeminiDesk-${VERSION}-Linux-${ARCH}.AppImage"
TOOL="${APPIMAGETOOL:-$(command -v appimagetool || true)}"
if [[ -n "${TOOL}" && -x "${TOOL}" ]]; then
  # Support containers/runners without FUSE by setting APPIMAGE_EXTRACT_AND_RUN=1 when needed
  APPIMAGE_EXTRACT_AND_RUN=1 ARCH="${ARCH}" "${TOOL}" --no-appstream "${APPDIR}" "${OUT_FILE}"
  chmod +x "${OUT_FILE}"
  echo "Built AppImage: ${OUT_FILE}"
else
  echo "appimagetool not found; skipping AppImage build."
fi

TAR_FILE="${OUT_DIR}/GeminiDesk-${VERSION}-Linux-${ARCH}.tar.gz"
tar -czf "${TAR_FILE}" -C "${APPDIR}/usr/bin" gemini-desk -C "${REPO_ROOT}" README.md THIRD_PARTY_LICENSES.md
echo "Built tarball: ${TAR_FILE}"

rm -rf "${APPDIR}"
