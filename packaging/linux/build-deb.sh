#!/usr/bin/env bash
set -euo pipefail

# Determine repository root
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

cd "${REPO_ROOT}"

# Extract version from Cargo.toml or use override
VERSION="${1:-$(grep -m1 '^version' Cargo.toml | awk -F '"' '{print $2}')}"
ARCH="amd64"
PKG_NAME="beam"
DEB_FILE="${REPO_ROOT}/dist/${PKG_NAME}_${VERSION}_${ARCH}.deb"

echo "Building Debian package for ${PKG_NAME} v${VERSION} (${ARCH})..."

# Ensure release binary is compiled
if [[ ! -f "target/release/beam" ]]; then
    echo "Release binary target/release/beam not found. Compiling..."
    cargo build --release -p beam-app
fi

mkdir -p "dist"
STAGING_DIR="$(mktemp -d -t beam_deb_staging_XXXXXX)"
trap 'rm -rf "${STAGING_DIR}"' EXIT

# Create directory structure
mkdir -p "${STAGING_DIR}/DEBIAN"
mkdir -p "${STAGING_DIR}/usr/bin"
mkdir -p "${STAGING_DIR}/usr/share/applications"
mkdir -p "${STAGING_DIR}/usr/share/metainfo"
mkdir -p "${STAGING_DIR}/usr/share/mime/packages"
mkdir -p "${STAGING_DIR}/usr/share/doc/${PKG_NAME}"

# Install binary
cp "target/release/beam" "${STAGING_DIR}/usr/bin/${PKG_NAME}"
chmod 0755 "${STAGING_DIR}/usr/bin/${PKG_NAME}"
if command -v strip >/dev/null 2>&1; then
    strip --strip-all "${STAGING_DIR}/usr/bin/${PKG_NAME}" || true
fi

# Install desktop file, metainfo, mime
cp "packaging/beam.desktop" "${STAGING_DIR}/usr/share/applications/beam.desktop"
chmod 0644 "${STAGING_DIR}/usr/share/applications/beam.desktop"

cp "packaging/com.github.mozcelik14.beam.metainfo.xml" "${STAGING_DIR}/usr/share/metainfo/"
chmod 0644 "${STAGING_DIR}/usr/share/metainfo/com.github.mozcelik14.beam.metainfo.xml"

cp "packaging/beam-mime.xml" "${STAGING_DIR}/usr/share/mime/packages/"
chmod 0644 "${STAGING_DIR}/usr/share/mime/packages/beam-mime.xml"

# Install hicolor icons
for s in 16 32 48 64 128 256 512; do
    icon_src="packaging/icons/hicolor/${s}x${s}/apps/beam.png"
    if [[ -f "${icon_src}" ]]; then
        dest_dir="${STAGING_DIR}/usr/share/icons/hicolor/${s}x${s}/apps"
        mkdir -p "${dest_dir}"
        cp "${icon_src}" "${dest_dir}/beam.png"
        chmod 0644 "${dest_dir}/beam.png"
    fi
done

# Install documentation & license
cp "LICENSE" "${STAGING_DIR}/usr/share/doc/${PKG_NAME}/copyright"
chmod 0644 "${STAGING_DIR}/usr/share/doc/${PKG_NAME}/copyright"
if [[ -f "README.md" ]]; then
    cp "README.md" "${STAGING_DIR}/usr/share/doc/${PKG_NAME}/"
    chmod 0644 "${STAGING_DIR}/usr/share/doc/${PKG_NAME}/README.md"
fi

# Calculate installed size in KB
INSTALLED_SIZE=$(du -sk "${STAGING_DIR}/usr" | cut -f1)

# Write DEBIAN/control
cat << EOF > "${STAGING_DIR}/DEBIAN/control"
Package: ${PKG_NAME}
Version: ${VERSION}
Section: graphics
Priority: optional
Architecture: ${ARCH}
Installed-Size: ${INSTALLED_SIZE}
Maintainer: beaM. Contributors <https://github.com/MOzcelik14/beaM>
Depends: libc6 (>= 2.31)
Recommends: shared-mime-info, hicolor-icon-theme
Homepage: https://mozcelik14.github.io/beaM/
Description: Professional, non-destructive RAW photo editor and library manager
 beaM. is an independent, high-performance desktop photographic darkroom
 and asset manager built in 100% pure Rust, powered by wgpu and egui.
EOF

# Write postinst script (update mime, desktop, icons)
cat << 'EOF' > "${STAGING_DIR}/DEBIAN/postinst"
#!/bin/sh
set -e
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database -q /usr/share/applications || true
fi
if command -v update-mime-database >/dev/null 2>&1; then
    update-mime-database /usr/share/mime || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -q -t -f /usr/share/icons/hicolor || true
fi
exit 0
EOF
chmod 0755 "${STAGING_DIR}/DEBIAN/postinst"

# Write postrm script
cat << 'EOF' > "${STAGING_DIR}/DEBIAN/postrm"
#!/bin/sh
set -e
if [ "$1" = "remove" ] || [ "$1" = "purge" ]; then
    if command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database -q /usr/share/applications || true
    fi
    if command -v update-mime-database >/dev/null 2>&1; then
        update-mime-database /usr/share/mime || true
    fi
    if command -v gtk-update-icon-cache >/dev/null 2>&1; then
        gtk-update-icon-cache -q -t -f /usr/share/icons/hicolor || true
    fi
fi
exit 0
EOF
chmod 0755 "${STAGING_DIR}/DEBIAN/postrm"

# Build package
dpkg-deb --build --root-owner-group "${STAGING_DIR}" "${DEB_FILE}"

echo "Successfully built Debian package: ${DEB_FILE}"
ls -lh "${DEB_FILE}"
