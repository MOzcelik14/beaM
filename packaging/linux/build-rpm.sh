#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

cd "${REPO_ROOT}"

VERSION="${1:-$(grep -m1 '^version' Cargo.toml | awk -F '"' '{print $2}')}"
ARCH="x86_64"
PKG_NAME="beam"

echo "Building RPM package for ${PKG_NAME} v${VERSION} (${ARCH})..."

if [[ ! -f "target/release/beam" ]]; then
    echo "Release binary target/release/beam not found. Compiling..."
    cargo build --release -p beam-app
fi

mkdir -p "dist"

if command -v rpmbuild >/dev/null 2>&1; then
    RPM_TOPDIR="$(mktemp -d -t beam_rpm_topdir_XXXXXX)"
    trap 'rm -rf "${RPM_TOPDIR}"' EXIT

    mkdir -p "${RPM_TOPDIR}"/{BUILD,RPMS,SOURCES,SPECS,SRPMS}
    cp "${REPO_ROOT}/packaging/linux/beam.spec" "${RPM_TOPDIR}/SPECS/"

    # Run rpmbuild directly using current directory as build root
    rpmbuild -bb \
        --define "_topdir ${RPM_TOPDIR}" \
        --define "_sourcedir ${REPO_ROOT}" \
        --define "buildroot ${RPM_TOPDIR}/BUILDROOT" \
        "${RPM_TOPDIR}/SPECS/beam.spec"

    find "${RPM_TOPDIR}/RPMS" -type f -name "*.rpm" -exec cp {} "${REPO_ROOT}/dist/" \;
    echo "Successfully built RPM package with rpmbuild:"
    ls -lh dist/*.rpm
elif command -v cargo-generate-rpm >/dev/null 2>&1; then
    echo "Using cargo-generate-rpm..."
    cargo generate-rpm -p beam-app -o "dist/${PKG_NAME}-${VERSION}-1.${ARCH}.rpm"
    echo "Successfully built RPM package with cargo-generate-rpm:"
    ls -lh "dist/${PKG_NAME}-${VERSION}-1.${ARCH}.rpm"
else
    echo "Notice: Neither 'rpmbuild' nor 'cargo-generate-rpm' was found locally."
    echo "To build RPMs on Debian/Ubuntu, install rpm: 'sudo apt install rpm' or 'cargo install cargo-generate-rpm'."
    echo "RPM generation will be automatically executed in the GitHub Actions CI pipeline."
fi
