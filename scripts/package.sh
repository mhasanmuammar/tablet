#!/bin/sh
# Builds the release binaries and packs them with the installer into
# dist/tablet-center-<arch>-linux.tar.gz, the archive attached to GitHub
# releases. The name has no version so the README can link to
# releases/latest/download/. Usage:
#   scripts/package.sh                # native build, arch = uname -m
#   scripts/package.sh aarch64        # cross-compile for aarch64
#   scripts/package.sh --target aarch64-unknown-linux-gnu
set -eu
cd "$(dirname "$0")/.."

arch="${1:-}"
target=""
case "$arch" in
    --target)
        shift
        target="$1"
        shift
        ;;
    --target=*)
        target="${arch#--target=}"
        ;;
    x86_64|aarch64|riscv64)
        # Map the release arch to a Rust target triple.
        case "$arch" in
            x86_64)   target="x86_64-unknown-linux-gnu" ;;
            aarch64)  target="aarch64-unknown-linux-gnu" ;;
            riscv64)  target="riscv64gc-unknown-linux-gnu" ;;
        esac
        ;;
    "")
        arch="$(uname -m)"
        target="x86_64-unknown-linux-gnu"
        case "$arch" in
            aarch64|arm64) target="aarch64-unknown-linux-gnu" ;;
        esac
        ;;
esac

# The release archive name uses the arch, not the triple (aarch64, not
# aarch64-unknown-linux-gnu), so install.sh can pick by uname -m.
name="tablet-center-${arch}-linux"

if [ -n "$target" ] && [ "$target" != "x86_64-unknown-linux-gnu" ]; then
    # Cross-compiling needs the target's std and the host's build deps. The
    # aarch64 std ships with rustup; the linker and libc come from the
    # cross toolchain installed by the release workflow.
    cargo build --locked --release --target "$target" -p tabletd -p tablet-center
    bindir="target/$target/release"
else
    cargo build --locked --release -p tabletd -p tablet-center
    bindir="target/release"
fi

stage="dist/tablet-center"
rm -rf "$stage"
mkdir -p "$stage/bin"
install -m 755 "$bindir/tabletd" "$bindir/tablet-center" "$stage/bin/"
cp -r packaging gnome-extension "$stage/"
cp install.sh uninstall.sh LICENSE README.md "$stage/"
tar -C dist -czf "dist/$name.tar.gz" tablet-center
echo "dist/$name.tar.gz"