#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

cd "${REPO_ROOT}"

VERSION="${1:-$(grep -m1 '^version' Cargo.toml | awk -F '"' '{print $2}')}"
ARCH="x86_64"
DIST_DIR="${REPO_ROOT}/dist"

echo "========================================="
echo "Building all Linux packages for beaM. v${VERSION}"
echo "========================================="

mkdir -p "${DIST_DIR}"

# 1. Compile release binary once
echo "[1/5] Compiling optimized release binary..."
cargo build --release -p beam-app

# 2. Build portable tar.gz
echo "[2/5] Creating portable tar.gz bundle..."
PORTABLE_DIR="${DIST_DIR}/beam-v${VERSION}-linux-${ARCH}"
rm -rf "${PORTABLE_DIR}"
mkdir -p "${PORTABLE_DIR}/assets"
cp "target/release/beam" "${PORTABLE_DIR}/"
cp "README.md" "${PORTABLE_DIR}/"
cp "assets/logo.png" "${PORTABLE_DIR}/assets/"
cp "assets/icon.png" "${PORTABLE_DIR}/assets/"
cp "assets/icon_64.png" "${PORTABLE_DIR}/assets/"
(cd "${DIST_DIR}" && tar -czf "beam-v${VERSION}-linux-${ARCH}.tar.gz" "beam-v${VERSION}-linux-${ARCH}")
echo "Portable tarball: ${DIST_DIR}/beam-v${VERSION}-linux-${ARCH}.tar.gz"

# 3. Build Debian package
echo "[3/5] Building .deb package..."
"${SCRIPT_DIR}/build-deb.sh" "${VERSION}"

# 4. Build RPM package
echo "[4/5] Building .rpm package..."
"${SCRIPT_DIR}/build-rpm.sh" "${VERSION}" || true

# 5. Build AppImage
echo "[5/5] Building .AppImage package..."
"${SCRIPT_DIR}/build-appimage.sh" "${VERSION}" || true

echo "========================================="
echo "Linux package generation complete! Output files in dist/:"
ls -lh "${DIST_DIR}"
echo "========================================="
