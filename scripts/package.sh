#!/bin/sh
# Builds the release binaries and packs them with the installer into
# dist/tablet-center-<arch>-linux.tar.gz, the archive attached to GitHub
# releases. The name has no version so the README can link to
# releases/latest/download/. Usage:
#   scripts/package.sh                # native build, arch = uname -m
set -eu
cd "$(dirname "$0")/.."

arch="$(uname -m)"
case "$arch" in
    x86_64|amd64)    arch=x86_64 ;;
    aarch64|arm64)   arch=aarch64 ;;
    *) echo "Unsupported architecture: $arch" >&2; exit 1 ;;
esac

target="x86_64-unknown-linux-gnu"
case "$arch" in
    aarch64) target="aarch64-unknown-linux-gnu" ;;
esac

name="tablet-center-${arch}-linux"

cargo build --release -p tabletd -p tablet-center
bindir="target/release"

stage="dist/tablet-center"
rm -rf "$stage"
mkdir -p "$stage/bin"
install -m 755 "$bindir/tabletd" "$bindir/tablet-center" "$stage/bin/"
cp -r packaging gnome-extension "$stage/"
cp install.sh uninstall.sh LICENSE README.md "$stage/"
tar -C dist -czf "dist/$name.tar.gz" tablet-center
echo "dist/$name.tar.gz"