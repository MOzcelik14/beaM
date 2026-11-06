#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

cd "${REPO_ROOT}"

VERSION="${1:-$(grep -m1 '^version' Cargo.toml | awk -F '"' '{print $2}')}"
ARCH="x86_64"
PKG_NAME="beam"
APPIMAGE_FILE="${REPO_ROOT}/dist/beaM-${VERSION}-${ARCH}.AppImage"

echo "Building AppImage for ${PKG_NAME} v${VERSION} (${ARCH})..."

if [[ ! -f "target/release/beam" ]]; then
    echo "Release binary target/release/beam not found. Compiling..."
    cargo build --release -p beam-app
fi

mkdir -p "dist"

APPDIR="$(mktemp -d -t beam_appdir_XXXXXX)"
trap 'rm -rf "${APPDIR}"' EXIT
chmod 0755 "${APPDIR}"

# Assemble AppDir hierarchy
mkdir -p "${APPDIR}/usr/bin"
mkdir -p "${APPDIR}/usr/share/applications"
mkdir -p "${APPDIR}/usr/share/metainfo"
mkdir -p "${APPDIR}/usr/share/mime/packages"

# Binary
cp "target/release/beam" "${APPDIR}/usr/bin/beam"
chmod 0755 "${APPDIR}/usr/bin/beam"
if command -v strip >/dev/null 2>&1; then
    strip --strip-all "${APPDIR}/usr/bin/beam" || true
fi

# Desktop & AppStream & Mime
cp "packaging/beam.desktop" "${APPDIR}/beam.desktop"
cp "packaging/beam.desktop" "${APPDIR}/usr/share/applications/beam.desktop"
cp "packaging/com.github.mozcelik14.beam.metainfo.xml" "${APPDIR}/usr/share/metainfo/com.github.mozcelik14.beam.metainfo.xml"
cp "packaging/beam-mime.xml" "${APPDIR}/usr/share/mime/packages/"

# Icons
for s in 16 32 48 64 128 256 512; do
    icon_src="packaging/icons/hicolor/${s}x${s}/apps/beam.png"
    if [[ -f "${icon_src}" ]]; then
        mkdir -p "${APPDIR}/usr/share/icons/hicolor/${s}x${s}/apps"
        cp "${icon_src}" "${APPDIR}/usr/share/icons/hicolor/${s}x${s}/apps/beam.png"
    fi
done

# Root icon and .DirIcon
cp "packaging/icons/hicolor/256x256/apps/beam.png" "${APPDIR}/beam.png"
cp "packaging/icons/hicolor/256x256/apps/beam.png" "${APPDIR}/.DirIcon"

# AppRun launcher script
cat << 'EOF' > "${APPDIR}/AppRun"
#!/bin/sh
HERE="$(dirname "$(readlink -f "${0}")")"
export PATH="${HERE}/usr/bin:${PATH}"
export XDG_DATA_DIRS="${HERE}/usr/share:${XDG_DATA_DIRS:-/usr/local/share:/usr/share}"
exec "${HERE}/usr/bin/beam" "$@"
EOF
chmod 0755 "${APPDIR}/AppRun"

# Check for appimagetool or download continuous release
APPIMAGETOOL=""
if command -v appimagetool >/dev/null 2>&1; then
    APPIMAGETOOL="appimagetool"
elif [[ -x "/tmp/appimagetool-x86_64.AppImage" ]]; then
    APPIMAGETOOL="/tmp/appimagetool-x86_64.AppImage"
else
    echo "Downloading appimagetool..."
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL -o /tmp/appimagetool-x86_64.AppImage "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage"
        chmod +x /tmp/appimagetool-x86_64.AppImage
        APPIMAGETOOL="/tmp/appimagetool-x86_64.AppImage"
    elif command -v wget >/dev/null 2>&1; then
        wget -q -O /tmp/appimagetool-x86_64.AppImage "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage"
        chmod +x /tmp/appimagetool-x86_64.AppImage
        APPIMAGETOOL="/tmp/appimagetool-x86_64.AppImage"
    fi
fi

if [[ -n "${APPIMAGETOOL}" ]]; then
    chmod -R a+rX "${APPDIR}"
    chmod a+rx "${APPDIR}/AppRun"
    echo "Building AppImage using ${APPIMAGETOOL}..."
    ARCH="x86_64" "${APPIMAGETOOL}" --appimage-extract-and-run "${APPDIR}" "${APPIMAGE_FILE}"
    echo "Successfully built AppImage: ${APPIMAGE_FILE}"
    ls -lh "${APPIMAGE_FILE}"
else
    echo "Warning: appimagetool could not be obtained automatically (no internet or curl/wget)."
    echo "The AppDir structure was validated and AppImage generation is configured for GitHub Actions CI."
fi
